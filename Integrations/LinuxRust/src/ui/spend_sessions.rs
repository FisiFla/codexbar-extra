//! Session and tool detail sections of Usage & Spend (token mix, hourly activity, session
//! performance, tool activity, and the Projects / Sessions tabs), fed by `crate::scan`.
//!
//! Same unknown discipline as `spend_view`: a cost that was never priced is "—", a partly priced
//! total is "~$x" (lower bound), a family-fallback price is "est. $x" (estimate), and a percentile
//! needs enough samples before it is claimed.

use super::spend_view::{bar_fraction, count, money_lb, share_bar, swatch, ACCENT, EM_DASH};
use crate::charts::{self, ChartBar};
use crate::scan::aggregate::{
    Money, Perf, SessionRow, ToolRow, View, Zone, P95_MIN_SAMPLES, SLOW_TOOL_MS,
};
use crate::scan::Provider;
use crate::state::{App, Message, SessionSort};
use chrono::{Local, TimeZone, Utc};
use iced::widget::canvas::Canvas;
use iced::widget::{column, container, horizontal_space, row, text, Space};
use iced::{Color, Element, Length, Theme};

const TOOL_ROWS_COLLAPSED: usize = 8;
const SESSION_ROWS: usize = 40;
const MUTED: Color = Color::from_rgb(0.62, 0.64, 0.68);

// ---------------------------------------------------------------------------
// Pure formatting / filtering helpers (unit-tested below)
// ---------------------------------------------------------------------------

/// "~$1.20" lower bound, "est. $1.20" fallback-priced estimate, "—" never priced.
pub fn fmt_cost(m: &Money) -> String {
    if m.unknown {
        EM_DASH.to_string()
    } else if m.lower_bound {
        money_lb(Some(m.usd), true)
    } else if m.estimated {
        format!("est. {}", money_lb(Some(m.usd), false))
    } else {
        money_lb(Some(m.usd), false)
    }
}

/// 1.2K / 3.4M / 5.6B; exact below 1,000.
pub fn fmt_compact(n: i64) -> String {
    let a = n.unsigned_abs() as f64;
    let (v, s) = if a >= 1e9 {
        (a / 1e9, "B")
    } else if a >= 1e6 {
        (a / 1e6, "M")
    } else if a >= 1e3 {
        (a / 1e3, "K")
    } else {
        return n.to_string();
    };
    let sign = if n < 0 { "-" } else { "" };
    if v >= 100.0 {
        format!("{sign}{v:.0}{s}")
    } else if v >= 10.0 {
        format!("{sign}{v:.1}{s}")
    } else {
        format!("{sign}{v:.2}{s}")
    }
}

/// 850 ms / 6.5 s / 2m 03s / 1h 05m.
pub fn fmt_duration_ms(ms: f64) -> String {
    if !ms.is_finite() || ms < 0.0 {
        return EM_DASH.to_string();
    }
    if ms < 1000.0 {
        return format!("{ms:.0} ms");
    }
    let s = ms / 1000.0;
    if s < 60.0 {
        return format!("{s:.1} s");
    }
    let secs = s.round() as i64;
    if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub fn fmt_opt_duration(ms: Option<f64>) -> String {
    ms.map(fmt_duration_ms)
        .unwrap_or_else(|| EM_DASH.to_string())
}

pub fn fmt_ts(ms: i64, zone: Zone) -> String {
    let f = "%b %-d %H:%M";
    match zone {
        Zone::Utc => Utc
            .timestamp_millis_opt(ms)
            .single()
            .map(|d| d.format(f).to_string()),
        Zone::Local => Local
            .timestamp_millis_opt(ms)
            .single()
            .map(|d| d.format(f).to_string()),
    }
    .unwrap_or_else(|| EM_DASH.to_string())
}

pub fn zone_label(zone: Zone) -> String {
    match zone {
        Zone::Utc => "UTC".into(),
        Zone::Local => format!("local time, UTC{}", Local::now().format("%:z")),
    }
}

pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Case-insensitive substring match over provider, project, id and model names.
pub fn session_matches(s: &SessionRow, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let hay = format!(
        "{} {} {} {}",
        s.provider.label(),
        s.project.as_deref().unwrap_or(""),
        s.id,
        s.models
            .iter()
            .map(|m| m.model.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();
    q.split_whitespace().all(|w| hay.contains(w))
}

/// Filtered and ordered session list (cost sorts unknown last).
pub fn browse_sessions<'a>(
    sessions: &'a [SessionRow],
    query: &str,
    sort: SessionSort,
) -> Vec<&'a SessionRow> {
    let mut v: Vec<&SessionRow> = sessions
        .iter()
        .filter(|s| session_matches(s, query))
        .collect();
    match sort {
        SessionSort::Recent => {
            v.sort_by(|a, b| b.end_ms.cmp(&a.end_ms).then_with(|| a.id.cmp(&b.id)))
        }
        SessionSort::Cost => v.sort_by(|a, b| {
            let key = |s: &SessionRow| if s.cost.unknown { -1.0 } else { s.cost.usd };
            key(b).total_cmp(&key(a)).then_with(|| a.id.cmp(&b.id))
        }),
        SessionSort::Tokens => v.sort_by(|a, b| {
            b.tokens
                .total()
                .cmp(&a.tokens.total())
                .then_with(|| a.id.cmp(&b.id))
        }),
        SessionSort::Tools => v.sort_by(|a, b| {
            b.tool_calls
                .cmp(&a.tool_calls)
                .then_with(|| a.id.cmp(&b.id))
        }),
    }
    v
}

pub fn filter_tools<'a>(tools: &'a [ToolRow], query: &str) -> Vec<&'a ToolRow> {
    let q = query.trim().to_lowercase();
    tools
        .iter()
        .filter(|t| q.is_empty() || t.name.to_lowercase().contains(&q))
        .collect()
}

/// Relative width of a token-mix segment: parts per thousand, at least 1 so tiny shares stay visible.
pub fn portion(part: i64, total: i64) -> u16 {
    if total <= 0 {
        return 1;
    }
    ((part as f64 / total as f64 * 1000.0).round() as u16).max(1)
}

pub fn pct(part: i64, total: i64) -> String {
    if total <= 0 {
        return EM_DASH.to_string();
    }
    format!("{:.1}%", part as f64 / total as f64 * 100.0)
}

fn provider_color(p: Provider) -> Color {
    charts::segment_color(match p {
        Provider::Claude => 1,
        Provider::Codex => 0,
        Provider::OpenCode => 2,
        Provider::Pi => 3,
    })
}

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

fn heading(s: &str) -> Element<'static, Message> {
    text(s.to_string()).size(16).into()
}

fn note(s: impl Into<String>) -> Element<'static, Message> {
    text(s.into()).size(12).color(MUTED).into()
}

fn stat(label: &str, value: String) -> Element<'static, Message> {
    container(
        column![
            text(label.to_string()).size(12).color(MUTED),
            text(value).size(17)
        ]
        .spacing(2),
    )
    .padding(8)
    .style(crate::ui::style::card)
    .into()
}

fn cell(s: impl Into<String>, w: f32) -> Element<'static, Message> {
    text(s.into()).size(13).width(Length::Fixed(w)).into()
}

/// Stacked input / output / cache read / cache write bar with a count + share legend.
pub fn token_mix(mix: &crate::scan::aggregate::TokenMix) -> Element<'static, Message> {
    let parts = [
        ("Input", mix.input),
        ("Output", mix.output),
        ("Cache read", mix.cache_read),
        ("Cache write", mix.cache_write),
    ];
    let total = mix.total();
    let mut bar = row![].height(Length::Fixed(14.0));
    for (i, (_, v)) in parts.iter().enumerate() {
        if *v > 0 {
            let c = charts::segment_color(i);
            bar = bar.push(
                container(Space::new(Length::Fill, Length::Fill))
                    .width(Length::FillPortion(portion(*v, total)))
                    .height(Length::Fill)
                    .style(move |_: &Theme| container::Style {
                        background: Some(c.into()),
                        ..Default::default()
                    }),
            );
        }
    }
    let mut legend = row![].spacing(14);
    for (i, (name, v)) in parts.iter().enumerate() {
        legend = legend.push(
            row![
                swatch(charts::segment_color(i)),
                text(format!("{name} {} ({})", fmt_compact(*v), pct(*v, total))).size(12),
            ]
            .spacing(5),
        );
    }
    column![bar, legend].spacing(6).into()
}

/// 24-hour estimated-spend bars in the selected time zone, with the zone toggle.
pub fn hourly(app: &App) -> Element<'static, Message> {
    let v = &app.scan_view;
    let any = v.hourly.iter().any(|h| h.requests > 0);
    let bars: Vec<ChartBar> = v
        .hourly
        .iter()
        .enumerate()
        .map(|(h, b)| ChartBar {
            date: format!("{h:02}:00"),
            value: Some(b.cost),
            segments: Vec::new(),
        })
        .collect();
    let zone_btn = |label: &str, z: Zone| -> Element<'static, Message> {
        let b = crate::ui::style::button(text(label.to_string()).size(12))
            .on_press(Message::SetScanZone(z));
        if z == app.scan_zone {
            b.style(crate::ui::style::segment(true)).into()
        } else {
            b.style(crate::ui::style::segment(false)).into()
        }
    };
    let mut col = column![row![
        heading("Hourly activity"),
        horizontal_space(),
        text("Time zone").size(12).color(MUTED),
        zone_btn("Local", Zone::Local),
        zone_btn("UTC", Zone::Utc),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)]
    .spacing(6);
    if !any {
        return col.push(note("No requests in this period.")).into();
    }
    let peak = v
        .hourly
        .iter()
        .enumerate()
        .max_by_key(|(_, b)| b.tokens)
        .map(|(h, b)| (h, *b));
    let chart: Element<'static, Message> = Canvas::new(charts::BarChart {
        key: "scan-hourly".into(),
        bars,
        color: ACCENT,
        peak_cap: false,
        hover: app
            .chart_hover
            .as_ref()
            .filter(|(k, _)| k == "scan-hourly")
            .map(|(_, i)| *i),
        fmt: charts::money,
        axis: charts::axis_money,
        y_max: None,
    })
    .width(Length::Fill)
    .height(Length::Fixed(110.0))
    .into();
    col = col.push(chart);
    let mut line = format!(
        "Estimated spend by hour of day, {}.",
        zone_label(app.scan_zone)
    );
    if let Some((h, b)) = peak {
        line.push_str(&format!(
            " Busiest hour {h:02}:00 · {} requests · {} tokens.",
            count(Some(b.requests as i64)),
            fmt_compact(b.tokens)
        ));
    }
    col.push(note(line)).into()
}

pub fn performance(perf: &Perf) -> Element<'static, Message> {
    let mut col = column![heading("Session performance")].spacing(6);
    if perf.samples == 0 {
        return col
            .push(note(
                "No timed turns in this period. Turn timing is recorded by Codex rollouts only.",
            ))
            .into();
    }
    let p95 = match perf.p95_duration_ms {
        Some(v) => fmt_duration_ms(v),
        None => format!("{EM_DASH} (needs {P95_MIN_SAMPLES}+)"),
    };
    col = col.push(
        row![
            stat("Timed turns", count(Some(perf.samples as i64))),
            stat("Median first token", fmt_opt_duration(perf.median_ttft_ms)),
            stat(
                "Median speed",
                perf.median_tps
                    .map(|t| format!("{t:.0} tok/s"))
                    .unwrap_or_else(|| EM_DASH.into())
            ),
        ]
        .spacing(8),
    );
    col = col.push(
        row![
            stat("Median duration", fmt_opt_duration(perf.median_duration_ms)),
            stat("P95 duration", p95),
            stat(
                "Cache hit",
                perf.cache_hit_pct
                    .map(|p| format!("{p:.0}%"))
                    .unwrap_or_else(|| EM_DASH.into())
            ),
        ]
        .spacing(8),
    );
    if !perf.groups.is_empty() {
        col = col.push(
            row![
                text("Model / effort")
                    .size(12)
                    .color(MUTED)
                    .width(Length::Fixed(230.0)),
                text("Turns")
                    .size(12)
                    .color(MUTED)
                    .width(Length::Fixed(60.0)),
                text("Speed")
                    .size(12)
                    .color(MUTED)
                    .width(Length::Fixed(90.0)),
                text("First token")
                    .size(12)
                    .color(MUTED)
                    .width(Length::Fixed(100.0)),
                text("Duration").size(12).color(MUTED),
            ]
            .spacing(8),
        );
        for g in perf.groups.iter().take(6) {
            col = col.push(
                row![
                    cell(
                        match &g.effort {
                            Some(e) => format!("{} · {e}", g.model),
                            None => g.model.clone(),
                        },
                        230.0
                    ),
                    cell(g.samples.to_string(), 60.0),
                    cell(
                        g.median_tps
                            .map(|t| format!("{t:.0} tok/s"))
                            .unwrap_or_else(|| EM_DASH.into()),
                        90.0
                    ),
                    cell(fmt_opt_duration(g.median_ttft_ms), 100.0),
                    text(fmt_duration_ms(g.median_duration_ms)).size(13),
                ]
                .spacing(8),
            );
        }
    }
    col.into()
}

pub fn tool_table(app: &App, tools: &[ToolRow], title: &str) -> Element<'static, Message> {
    let mut col = column![row![
        heading(title),
        horizontal_space(),
        crate::ui::style::text_input("Filter tools", &app.tool_filter)
            .on_input(Message::ToolFilter)
            .size(13)
            .width(Length::Fixed(180.0)),
    ]
    .align_y(iced::Alignment::Center)]
    .spacing(6);
    if tools.is_empty() {
        return col
            .push(note("No tool calls recorded in this period."))
            .into();
    }
    let shown_all = filter_tools(tools, &app.tool_filter);
    let max_calls = shown_all.iter().map(|t| t.calls).max().unwrap_or(1) as f64;
    col = col.push(
        row![
            text("Tool")
                .size(12)
                .color(MUTED)
                .width(Length::Fixed(230.0)),
            text("Calls")
                .size(12)
                .color(MUTED)
                .width(Length::Fixed(70.0)),
            text("Errors")
                .size(12)
                .color(MUTED)
                .width(Length::Fixed(70.0)),
            text("Slow")
                .size(12)
                .color(MUTED)
                .width(Length::Fixed(60.0)),
            text("Longest").size(12).color(MUTED),
        ]
        .spacing(8),
    );
    let limit = if app.tools_expanded || !app.tool_filter.trim().is_empty() {
        usize::MAX
    } else {
        TOOL_ROWS_COLLAPSED
    };
    for t in shown_all.iter().take(limit) {
        col = col.push(
            column![
                row![
                    cell(t.name.clone(), 230.0),
                    cell(count(Some(t.calls as i64)), 70.0),
                    text(count(Some(t.errors as i64)))
                        .size(13)
                        .width(Length::Fixed(70.0))
                        .color(if t.errors > 0 {
                            Color::from_rgb(0.92, 0.45, 0.35)
                        } else {
                            Color::from_rgb(0.85, 0.86, 0.88)
                        }),
                    cell(t.slow.to_string(), 60.0),
                    text(fmt_opt_duration(t.longest_ms.map(|v| v as f64))).size(13),
                ]
                .spacing(8),
                share_bar(bar_fraction(t.calls as f64, max_calls), ACCENT),
            ]
            .spacing(2),
        );
    }
    if shown_all.is_empty() {
        col = col.push(note("No tool matches the filter."));
    } else if shown_all.len() > TOOL_ROWS_COLLAPSED && app.tool_filter.trim().is_empty() {
        let label = if app.tools_expanded {
            "Show fewer".to_string()
        } else {
            format!("Show {} more", shown_all.len() - TOOL_ROWS_COLLAPSED)
        };
        col = col.push(
            crate::ui::style::button(text(label).size(12))
                .style(crate::ui::style::push)
                .on_press(Message::ToggleToolsExpanded),
        );
    }
    col.push(note(format!(
        "Native tool calls only. Duration is the gap between a call and its result; slow means {} s or more; errors are failed results.",
        SLOW_TOOL_MS / 1000
    )))
    .into()
}

/// Sessions detail extras (Mac `SessionUsageBrowser`): hourly activity, performance, tools.
pub fn sections(app: &App) -> Element<'static, Message> {
    if !app.scan_loaded {
        return note("Scanning local sessions…");
    }
    let v: &View = &app.scan_view;
    if v.requests == 0 && v.tools.is_empty() && v.sessions.is_empty() {
        return note("No local session history in this period.");
    }
    column![
        crate::ui::spend_charts::panel(hourly(app)),
        crate::ui::spend_charts::panel(performance(&v.perf)),
        crate::ui::spend_charts::panel(tool_table(app, &v.tools, "Tool activity")),
    ]
    .spacing(12)
    .into()
}

fn session_button(app: &App, s: &SessionRow) -> Element<'static, Message> {
    let selected = app.selected_session.as_ref() == Some(&(s.provider, s.id.clone()));
    let line = row![
        container(Space::new(Length::Fixed(4.0), Length::Fixed(16.0))).style({
            let c = provider_color(s.provider);
            move |_: &Theme| container::Style {
                background: Some(c.into()),
                ..Default::default()
            }
        }),
        text(s.provider.label()).size(12).width(Length::Fixed(52.0)),
        text(s.project.clone().unwrap_or_else(|| EM_DASH.into()))
            .size(13)
            .width(Length::FillPortion(2)),
        text(s.primary_model().to_string())
            .size(13)
            .width(Length::FillPortion(3)),
        text(fmt_ts(s.start_ms, app.scan_zone))
            .size(12)
            .width(Length::Fixed(84.0)),
        text(fmt_compact(s.tokens.total()))
            .size(13)
            .width(Length::Fixed(56.0)),
        text(fmt_cost(&s.cost)).size(13).width(Length::Fixed(96.0)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    let b = crate::ui::style::button(line)
        .width(Length::Fill)
        .padding([4, 8])
        .on_press(Message::SelectSession(if selected {
            None
        } else {
            Some((s.provider, s.id.clone()))
        }));
    if selected {
        b.style(crate::ui::style::segment(true)).into()
    } else {
        b.style(crate::ui::style::segment(false)).into()
    }
}

fn sort_button(label: &str, sort: SessionSort, current: SessionSort) -> Element<'static, Message> {
    let b = crate::ui::style::button(text(label.to_string()).size(12))
        .on_press(Message::SessionSort(sort));
    if sort == current {
        b.style(crate::ui::style::segment(true)).into()
    } else {
        b.style(crate::ui::style::segment(false)).into()
    }
}

pub fn sessions_tab(app: &App) -> Element<'static, Message> {
    if !app.scan_loaded {
        return note("Scanning local sessions…");
    }
    let all = &app.scan_view.sessions;
    if all.is_empty() {
        return note("No local sessions in this period.");
    }
    let list = browse_sessions(all, &app.session_query, app.session_sort);
    let mut col = column![row![
        crate::ui::style::text_input("Search sessions, projects, models", &app.session_query)
            .on_input(Message::SessionQuery)
            .size(13)
            .width(Length::Fill),
        text("Sort").size(12).color(MUTED),
        sort_button("Recent", SessionSort::Recent, app.session_sort),
        sort_button("Cost", SessionSort::Cost, app.session_sort),
        sort_button("Tokens", SessionSort::Tokens, app.session_sort),
        sort_button("Tools", SessionSort::Tools, app.session_sort),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)]
    .spacing(8);
    col = col.push(note(format!(
        "{} of {} sessions · start times in {}",
        list.len(),
        all.len(),
        zone_label(app.scan_zone)
    )));
    for s in list.iter().take(SESSION_ROWS) {
        col = col.push(session_button(app, s));
    }
    if list.len() > SESSION_ROWS {
        col = col.push(note(format!(
            "Showing the first {SESSION_ROWS}; refine the search to see others."
        )));
    }
    if list.is_empty() {
        col = col.push(note("No session matches the search."));
    }
    if let Some(sel) = &app.selected_session {
        if let Some(s) = all.iter().find(|s| (s.provider, &s.id) == (sel.0, &sel.1)) {
            col = col.push(session_detail(app, s));
        }
    }
    col.into()
}

fn session_detail(app: &App, s: &SessionRow) -> Element<'static, Message> {
    let span = if s.end_ms > s.start_ms {
        fmt_duration_ms((s.end_ms - s.start_ms) as f64)
    } else {
        EM_DASH.to_string()
    };
    let mut col = column![
        row![
            text(format!(
                "{} · {} · {}",
                s.provider.label(),
                s.project
                    .clone()
                    .unwrap_or_else(|| "(unknown project)".into()),
                short_id(&s.id)
            ))
            .size(18),
            horizontal_space(),
            crate::ui::style::button(text("Close").size(12))
                .style(crate::ui::style::push)
                .on_press(Message::SelectSession(None)),
        ],
        row![
            stat("Cost", fmt_cost(&s.cost)),
            stat("Tokens", fmt_compact(s.tokens.total())),
            stat("Requests", count(Some(s.requests as i64))),
            stat("Span", span),
            stat(
                "Tool calls",
                format!("{} ({} failed)", s.tool_calls, s.tool_errors)
            ),
        ]
        .spacing(8),
        note(format!(
            "{} to {} ({})",
            fmt_ts(s.start_ms, app.scan_zone),
            fmt_ts(s.end_ms, app.scan_zone),
            zone_label(app.scan_zone)
        )),
        heading("Token mix"),
        token_mix(&s.tokens),
        performance(&s.perf),
        tool_table(app, &s.tools, "Tool activity"),
        heading("Models"),
    ]
    .spacing(10);
    if s.models.is_empty() {
        col = col.push(note("No billed requests in this period."));
    }
    for m in &s.models {
        col = col.push(row![
            text(m.model.clone()).size(13),
            horizontal_space(),
            text(format!(
                "{} requests · {} tokens · {}",
                m.requests,
                fmt_compact(m.tokens.total()),
                fmt_cost(&m.cost)
            ))
            .size(13),
        ]);
    }
    if s.cost.unknown {
        col = col.push(note(
            "No list price is known for this session's model, so its cost is unknown (—), not $0.",
        ));
    } else if s.cost.lower_bound {
        col = col.push(note(
            "~ marks a lower bound: some requests use an unpriced model.",
        ));
    } else if s.cost.estimated {
        col = col.push(note(
            "est. marks an estimate from the newest known price of the same model family.",
        ));
    }
    container(col)
        .padding(12)
        .width(Length::Fill)
        .style(crate::ui::style::card)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::aggregate::{ModelRow, TokenMix};

    fn sess(
        id: &str,
        project: &str,
        end: i64,
        usd: f64,
        unknown: bool,
        tools: usize,
    ) -> SessionRow {
        SessionRow {
            provider: Provider::Claude,
            id: id.into(),
            project: Some(project.into()),
            start_ms: 0,
            end_ms: end,
            requests: 1,
            tokens: TokenMix {
                input: usd as i64 * 10,
                ..Default::default()
            },
            cost: Money {
                usd,
                unknown,
                ..Default::default()
            },
            models: vec![ModelRow {
                model: "fake-model".into(),
                requests: 1,
                tokens: TokenMix::default(),
                cost: Money::default(),
            }],
            tools: vec![],
            tool_calls: tools,
            tool_errors: 0,
            perf: Perf::default(),
        }
    }

    #[test]
    fn cost_formatting_keeps_unknown_bound_and_estimate_distinct() {
        assert_eq!(
            fmt_cost(&Money {
                usd: 1.5,
                ..Default::default()
            }),
            "$1.50"
        );
        assert_eq!(
            fmt_cost(&Money {
                usd: 1.5,
                lower_bound: true,
                ..Default::default()
            }),
            "~$1.50"
        );
        assert_eq!(
            fmt_cost(&Money {
                usd: 1.5,
                estimated: true,
                ..Default::default()
            }),
            "est. $1.50"
        );
        assert_eq!(
            fmt_cost(&Money {
                unknown: true,
                ..Default::default()
            }),
            "—"
        );
    }

    #[test]
    fn compact_and_duration_formats() {
        assert_eq!(fmt_compact(999), "999");
        assert_eq!(fmt_compact(1_234), "1.23K");
        assert_eq!(fmt_compact(12_345_678), "12.3M");
        assert_eq!(fmt_compact(2_500_000_000), "2.50B");
        assert_eq!(fmt_duration_ms(850.0), "850 ms");
        assert_eq!(fmt_duration_ms(6500.0), "6.5 s");
        assert_eq!(fmt_duration_ms(123_000.0), "2m 03s");
        assert_eq!(fmt_duration_ms(3_900_000.0), "1h 05m");
        assert_eq!(fmt_duration_ms(f64::NAN), "—");
        assert_eq!(pct(1, 4), "25.0%");
        assert_eq!(pct(1, 0), "—");
        assert_eq!(portion(1, 4), 250);
        assert_eq!(portion(1, 1_000_000), 1);
        assert_eq!(portion(5, 0), 1);
    }

    #[test]
    fn session_search_and_sorting() {
        let v = vec![
            sess("aaaa1111", "alpha", 10, 5.0, false, 1),
            sess("bbbb2222", "beta", 30, 0.0, true, 9),
            sess("cccc3333", "gamma", 20, 9.0, false, 4),
        ];
        let ids = |s: SessionSort, q: &str| -> Vec<String> {
            browse_sessions(&v, q, s)
                .iter()
                .map(|x| x.id.clone())
                .collect()
        };
        assert_eq!(
            ids(SessionSort::Recent, ""),
            ["bbbb2222", "cccc3333", "aaaa1111"]
        );
        // unknown cost sorts last, never as the cheapest-known
        assert_eq!(
            ids(SessionSort::Cost, ""),
            ["cccc3333", "aaaa1111", "bbbb2222"]
        );
        assert_eq!(
            ids(SessionSort::Tools, ""),
            ["bbbb2222", "cccc3333", "aaaa1111"]
        );
        assert_eq!(ids(SessionSort::Recent, "BETA"), ["bbbb2222"]);
        assert_eq!(
            ids(SessionSort::Recent, "claude fake"),
            ["bbbb2222", "cccc3333", "aaaa1111"]
        );
        assert!(ids(SessionSort::Recent, "nomatch").is_empty());
    }

    #[test]
    fn tool_filter_is_case_insensitive() {
        let t = vec![
            ToolRow {
                name: "Bash".into(),
                calls: 2,
                ..Default::default()
            },
            ToolRow {
                name: "Read".into(),
                calls: 1,
                ..Default::default()
            },
        ];
        assert_eq!(filter_tools(&t, "bA").len(), 1);
        assert_eq!(filter_tools(&t, "").len(), 2);
    }

    #[test]
    fn short_id_truncates() {
        assert_eq!(short_id("0123456789"), "01234567");
        assert_eq!(short_id("ab"), "ab");
    }
}
