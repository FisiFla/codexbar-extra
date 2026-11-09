//! Embeds the macOS app's provider icons (`Sources/CodexBar/Resources/*ProviderIcon-*`) so the
//! Linux screens show the same logos. Generates `$OUT_DIR/provider_icons.rs`.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Sources/CodexBar/Resources");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut template = Vec::new();
    let mut brand = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("Sources/CodexBar/Resources") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let abs = path.canonicalize().unwrap().display().to_string();
        if let Some(id) = name.strip_prefix("ProviderIcon-").and_then(|n| n.strip_suffix(".svg")) {
            template.push((id.to_string(), abs));
        } else if let Some(rest) = name.strip_prefix("Brand-ProviderIcon-") {
            if let Some(id) = rest.strip_suffix(".svg") {
                brand.push((id.to_string(), abs, true));
            } else if let Some(id) = rest.strip_suffix(".png") {
                brand.push((id.to_string(), abs, false));
            }
        }
    }
    template.sort();
    brand.sort();
    let mut out = String::from("pub static TEMPLATE: &[(&str, &[u8])] = &[\n");
    for (id, abs) in &template {
        writeln!(out, "    ({id:?}, include_bytes!({abs:?})),").unwrap();
    }
    out.push_str("];\n/// (id, bytes, is_svg)\npub static BRAND: &[(&str, &[u8], bool)] = &[\n");
    for (id, abs, svg) in &brand {
        writeln!(out, "    ({id:?}, include_bytes!({abs:?}), {svg}),").unwrap();
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("provider_icons.rs");
    std::fs::write(dest, out).unwrap();
}
