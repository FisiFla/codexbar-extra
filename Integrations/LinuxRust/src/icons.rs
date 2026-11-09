//! Provider logos, the same files the macOS app ships (`Sources/CodexBar/Resources`, embedded
//! by `build.rs`). Template icons are monochrome and tinted like macOS template images; brand
//! icons keep their own colours (the Usage & Spend dashboard uses them).

use iced::widget::{container, image, svg, text};
use iced::{Color, Element, Length};

mod generated {
    include!(concat!(env!("OUT_DIR"), "/provider_icons.rs"));
}

/// Icon file stem for a provider id. Mirrors the Swift descriptors' `iconResourceName`, which
/// equals the id except for these providers.
pub fn icon_key(id: &str) -> &str {
    match id {
        "openai" | "azureopenai" => "codex",
        "alibabatokenplan" => "alibaba",
        "moonshot" => "kimi",
        other => other,
    }
}

fn template_bytes(id: &str) -> Option<&'static [u8]> {
    let key = icon_key(id);
    generated::TEMPLATE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, b)| *b)
}

#[cfg(test)]
pub fn has_icon(id: &str) -> bool {
    template_bytes(id).is_some()
}

/// Monochrome provider logo tinted `tint`; falls back to the provider's 2-letter tag.
pub fn icon<'a, M: 'a>(id: &str, size: f32, tint: Color) -> Element<'a, M> {
    match template_bytes(id) {
        Some(bytes) => svg(svg::Handle::from_memory(bytes))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .style(move |_, _| svg::Style { color: Some(tint) })
            .into(),
        None => fallback(id, size, tint),
    }
}

/// Full-colour brand logo when the macOS app has one, else the template logo tinted `tint`.
pub fn brand<'a, M: 'a>(id: &str, size: f32, tint: Color) -> Element<'a, M> {
    let key = icon_key(id);
    match generated::BRAND.iter().find(|(k, _, _)| *k == key) {
        Some((_, bytes, true)) => svg(svg::Handle::from_memory(*bytes))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
        Some((_, bytes, false)) => image(image::Handle::from_bytes(*bytes))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
        None => icon(id, size, tint),
    }
}

fn fallback<'a, M: 'a>(id: &str, size: f32, tint: Color) -> Element<'a, M> {
    container(text(crate::descriptors::tag(id)).size(size * 0.5).color(tint))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .center(Length::Fixed(size))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mac_provider_icon_is_embedded() {
        assert!(generated::TEMPLATE.len() >= 100);
        for id in ["codex", "claude", "openai", "azureopenai", "cursor", "gemini", "moonshot"] {
            assert!(has_icon(id), "{id}");
        }
        assert!(!has_icon("no-such-provider"));
        assert!(generated::BRAND.iter().any(|(k, _, svg)| *k == "claude" && *svg));
        assert!(generated::BRAND.iter().any(|(k, _, svg)| *k == "codex" && !*svg));
    }
}
