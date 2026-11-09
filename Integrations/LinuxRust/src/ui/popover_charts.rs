//! Quick View chart helpers: menu-row glyphs (SF-symbol stand-ins) and the "selected bar" detail
//! line the macOS chart menus show under each chart (`CostHistoryChartMenuView.detailContent`:
//! the hovered day, defaulting to the latest one).

use crate::charts::{short_date, ChartBar};
use iced::widget::svg;
use iced::{Color, Element, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Refresh,
    Dashboard,
    Sessions,
    Settings,
    Info,
    Quit,
    Chart,
    ChevronRight,
    ChevronDown,
    Plus,
    Status,
    Copy,
    External,
}

impl Glyph {
    /// 24x24 stroke icons (white stroke, recoloured at draw time).
    fn svg(self) -> &'static str {
        macro_rules! s {
            ($body:literal) => {
                concat!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                    $body,
                    "</svg>"
                )
            };
        }
        match self {
            Glyph::Refresh => s!(r#"<path d="M20 12a8 8 0 1 1-2.6-5.9"/><path d="M20 4v5h-5"/>"#),
            Glyph::Dashboard => s!(r#"<path d="M3 3v18h18"/><path d="M7 15l4-4 3 3 5-6"/>"#),
            Glyph::Sessions => s!(
                r#"<path d="M9 6h12M9 12h12M9 18h12"/><path d="M4 6h.01M4 12h.01M4 18h.01"/>"#
            ),
            Glyph::Settings => s!(
                r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>"#
            ),
            Glyph::Info => s!(r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.01"/>"#),
            Glyph::Quit => s!(
                r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M9.5 9.5l5 5M14.5 9.5l-5 5"/>"#
            ),
            Glyph::Chart => s!(r#"<path d="M5 20V11M12 20V4M19 20v-6"/>"#),
            Glyph::ChevronRight => s!(r#"<path d="M9 5l7 7-7 7"/>"#),
            Glyph::ChevronDown => s!(r#"<path d="M5 9l7 7 7-7"/>"#),
            Glyph::Plus => s!(r#"<path d="M12 5v14M5 12h14"/>"#),
            Glyph::Status => s!(r#"<path d="M2 12h4l3-8 4 16 3-8h6"/>"#),
            Glyph::Copy => s!(
                r#"<rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>"#
            ),
            Glyph::External => s!(
                r#"<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14L21 3"/>"#
            ),
        }
    }
}

pub fn glyph<'a, M: 'a>(g: Glyph, size: f32, tint: Color) -> Element<'a, M> {
    svg(svg::Handle::from_memory(g.svg().as_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(tint) })
        .into()
}

/// Bar shown in the detail line: the hovered one, else the latest bar with data.
pub fn selected_index(bars: &[ChartBar], hover: Option<usize>) -> Option<usize> {
    hover
        .filter(|i| *i < bars.len())
        .or_else(|| bars.iter().rposition(|b| b.value.is_some()))
}

/// `Oct 16: $12.34` for the selected bar; the Mac hint when nothing is selectable.
pub fn detail_line(bars: &[ChartBar], hover: Option<usize>, fmt: fn(f64) -> String) -> String {
    let Some(i) = selected_index(bars, hover) else {
        return "Hover a bar for details".to_string();
    };
    let bar = &bars[i];
    match bar.value {
        Some(v) => format!("{}: {}", short_date(&bar.date), fmt(v)),
        None => format!("{}: No data", short_date(&bar.date)),
    }
}

/// Segment rows (model breakdown) of the selected bar, largest first, at most `max`.
pub fn detail_rows(
    bars: &[ChartBar],
    hover: Option<usize>,
    fmt: fn(f64) -> String,
    max: usize,
) -> Vec<(usize, String, String)> {
    let Some(i) = selected_index(bars, hover) else {
        return Vec::new();
    };
    bars[i]
        .segments
        .iter()
        .take(max)
        .map(|s| (s.color_index, s.name.clone(), fmt(s.value)))
        .collect()
}

/// Cost chart detail: `Oct 16: $12.34 · 15K tokens` (tokens omitted when unknown/zero).
pub fn cost_detail(date: &str, cost: f64, tokens: i64) -> String {
    let mut s = format!("{}: {}", short_date(date), crate::charts::money(cost));
    if tokens > 0 {
        s.push_str(&format!(" · {} tokens", crate::throughput::count(tokens as f64)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::Segment;

    fn bar(date: &str, v: Option<f64>) -> ChartBar {
        ChartBar {
            date: date.into(),
            value: v,
            segments: Vec::new(),
        }
    }

    #[test]
    fn selection_defaults_to_latest_known_bar() {
        let bars = vec![bar("2026-11-07", Some(1.0)), bar("2026-11-08", Some(2.0)), bar("2026-11-09", None)];
        assert_eq!(selected_index(&bars, None), Some(1));
        assert_eq!(selected_index(&bars, Some(0)), Some(0));
        assert_eq!(selected_index(&bars, Some(9)), Some(1));
        assert_eq!(selected_index(&[], None), None);
    }

    #[test]
    fn detail_line_formats_hover_and_default() {
        let bars = vec![bar("2026-11-07", Some(1.5)), bar("2026-11-08", Some(2.0))];
        assert_eq!(detail_line(&bars, None, crate::charts::money), "Nov 8: $2.00");
        assert_eq!(detail_line(&bars, Some(0), crate::charts::money), "Nov 7: $1.50");
        assert_eq!(detail_line(&[], None, crate::charts::money), "Hover a bar for details");
        let unknown = vec![bar("2026-11-07", None)];
        assert_eq!(detail_line(&unknown, Some(0), crate::charts::money), "Nov 7: No data");
    }

    #[test]
    fn detail_rows_cap_and_keep_colour_slots() {
        let mut b = bar("2026-11-07", Some(3.0));
        b.segments = vec![
            Segment { name: "a".into(), color_index: 0, value: 2.0 },
            Segment { name: "b".into(), color_index: 6, value: 1.0 },
        ];
        let rows = detail_rows(&[b], None, crate::charts::money, 1);
        assert_eq!(rows, vec![(0, "a".to_string(), "$2.00".to_string())]);
    }

    #[test]
    fn cost_detail_includes_tokens_when_known() {
        assert_eq!(cost_detail("2026-11-09", 0.04, 15_000), "Nov 9: $0.04 · 15K tokens");
        assert_eq!(cost_detail("2026-11-09", 0.0, 0), "Nov 9: $0.00");
    }

    #[test]
    fn every_glyph_is_valid_svg() {
        for g in [
            Glyph::Refresh, Glyph::Dashboard, Glyph::Sessions, Glyph::Settings, Glyph::Info,
            Glyph::Quit, Glyph::Chart, Glyph::ChevronRight, Glyph::ChevronDown,
            Glyph::Plus, Glyph::Status, Glyph::Copy, Glyph::External,
        ] {
            let s = g.svg();
            assert!(s.starts_with("<svg") && s.ends_with("</svg>"), "{g:?}");
        }
    }
}
