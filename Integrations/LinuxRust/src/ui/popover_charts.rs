//! Quick View chart helpers: menu-row glyphs (SF-symbol stand-ins) and the "selected bar" detail
//! line the macOS chart menus show under each chart (`CostHistoryChartMenuView.detailContent`:
//! the hovered day, defaulting to the latest one).

use crate::charts::{short_date, ChartBar};
use iced::widget::svg;
use iced::{Color, Element, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Refresh,
    Chart,
    ChevronRight,
    ChevronDown,
    Copy,
    Shield,
    Warning,
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
            Glyph::Chart => s!(r#"<path d="M5 20V11M12 20V4M19 20v-6"/>"#),
            Glyph::ChevronRight => s!(r#"<path d="M9 5l7 7-7 7"/>"#),
            Glyph::ChevronDown => s!(r#"<path d="M5 9l7 7 7-7"/>"#),
            Glyph::External => s!(r#"<path d="M7 17L17 7M8 7h9v9"/>"#),
            Glyph::Copy => s!(
                r#"<rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>"#
            ),
            Glyph::Shield => s!(
                r#"<path d="M12 22s8-3.6 8-10V5l-8-3-8 3v7c0 6.4 8 10 8 10z"/><path d="M9.2 11.6l2 2 3.6-4.2"/>"#
            ),
            Glyph::Warning => s!(
                r#"<path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/><path d="M12 9v4M12 17h.01"/>"#
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
            Glyph::Refresh,
            Glyph::Chart, Glyph::ChevronRight, Glyph::ChevronDown,
            Glyph::Copy, Glyph::External,
        ] {
            let s = g.svg();
            assert!(s.starts_with("<svg") && s.ends_with("</svg>"), "{g:?}");
        }
    }
}
