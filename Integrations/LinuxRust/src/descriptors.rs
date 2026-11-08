//! Provider display descriptors: name, 2-letter tag, accent colour.
//! Names mirror `Usage.js providerName()`; accents come from the dashboard
//! fixture (`tests/fixtures/dashboard.json`, `display.accentColor`).

/// (id, display name, tag, accent hex) for providers with a known descriptor.
const TABLE: &[(&str, &str, &str, Option<&str>)] = &[
    ("codex", "Codex", "CX", Some("#49A3B0")),
    ("claude", "Claude", "CL", Some("#CC7C5E")),
    ("copilot", "GitHub Copilot", "GH", None),
    ("gemini", "Gemini", "GE", None),
    ("cursor", "Cursor", "CU", None),
    ("antigravity", "Antigravity", "AG", Some("#60BA7E")),
    ("openrouter", "OpenRouter", "OR", None),
    ("kiro", "Kiro", "KI", None),
    ("deepseek", "DeepSeek", "DS", Some("#4D6BFE")),
    ("muse", "Muse Code", "MU", Some("#0668E1")),
];

fn lookup(
    id: &str,
) -> Option<&'static (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
)> {
    TABLE.iter().find(|(pid, _, _, _)| *pid == id)
}

/// Display name; fallback title-cases the id like `Usage.js providerName()`
/// (first letter upper, rest unchanged), empty id -> "Unknown provider".
pub fn display_name(id: &str) -> String {
    if let Some((_, name, _, _)) = lookup(id) {
        return name.to_string();
    }
    if let Some((_, name)) = CATALOG_NAMES
        .read()
        .ok()
        .and_then(|n| n.iter().find(|(pid, _)| pid == id).cloned())
    {
        return name;
    }
    let mut chars = id.chars();
    match chars.next() {
        None => "Unknown provider".to_string(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Brand colours extracted from `Sources/CodexBarCore/Providers/*/*ProviderDescriptor.swift`
/// (`branding.color`). Regenerate when upstream adds providers.
const ACCENTS: &[(&str, &str)] = &[
    ("codex", "#49A3B0"),
    ("claude", "#CC7C5E"),
    ("cursor", "#F54E00"),
    ("opencode", "#3B82F6"),
    ("factory", "#FF6B35"),
    ("gemini", "#AB87EA"),
    ("antigravity", "#60BA7E"),
    ("copilot", "#A855F7"),
    ("zai", "#E85A6A"),
    ("minimax", "#FE603C"),
    ("kimi", "#FE603C"),
    ("kiro", "#9046FF"),
    ("vertexai", "#4285F4"),
    ("augment", "#1AA049"),
    ("jetbrains", "#FF3399"),
    ("amp", "#F34E3F"),
    ("synthetic", "#141414"),
    ("warp", "#938BB4"),
    ("opencodego", "#3B82F6"),
    ("alibaba", "#FF6A00"),
    ("kilo", "#F27027"),
    ("ollama", "#888888"),
    ("openrouter", "#6467F2"),
    ("perplexity", "#20B2AA"),
    ("abacus", "#814EE8"),
    ("openai", "#0F826E"),
    ("manus", "#34322D"),
    ("moonshot", "#205DEB"),
    ("windsurf", "#34E8BB"),
    ("mimo", "#FF6900"),
    ("doubao", "#3370FF"),
    ("mistral", "#FF5229"),
    ("deepseek", "#4D6BFE"),
    ("codebuff", "#00FF95"),
    ("venice", "#3C8FDD"),
    ("commandcode", "#8C4EDD"),
    ("stepfun", "#2196F2"),
    ("bedrock", "#01A88D"),
    ("elevenlabs", "#EBEBE6"),
    ("grok", "#10A37F"),
    ("groq", "#F56844"),
    ("llmproxy", "#24B47E"),
    ("deepgram", "#6467F2"),
    ("azureopenai", "#0078D4"),
    ("alibabatokenplan", "#FF6A00"),
    ("devin", "#317CFF"),
    ("t3chat", "#F56647"),
    ("zed", "#084EFF"),
    ("sakana", "#2975DB"),
    ("qoder", "#10B981"),
    ("litellm", "#4C89F0"),
    ("poe", "#5D5CDE"),
    ("chutes", "#3184FF"),
    ("clawrouter", "#596EF6"),
    ("sub2api", "#14B8A6"),
    ("wayfinder", "#10A37F"),
    ("clinepass", "#5487C8"),
    ("deepinfra", "#2A3275"),
    ("neuralwatt", "#D55934"),
    ("longcat", "#29E154"),
    ("zenmux", "#6C5CE7"),
    ("aiand", "#E25C2B"),
    ("qwencloud", "#615CED"),
    ("zoommate", "#0B5CFF"),
    ("fireworks", "#F25B1C"),
    ("xai", "#8E8E93"),
    ("notion", "#337EA9"),
    ("ibmbob", "#0E61FA"),
    ("helmcode", "#4934E1"),
    ("nous", "#D6A55C"),
    ("muse", "#0668E1"),
    ("coderabbit", "#FF5C35"),
    ("replicate", "#000000"),
    ("huggingface", "#FFD21E"),
    ("pi", "#4D9ABF"),
    ("v0", "#111111"),
    ("typesafe", "#111111"),
    ("bifrost", "#33C09E"),
    ("hyper", "#FF60FF"),
    ("gitkraken", "#179287"),
    ("devpass", "#2563EB"),
    ("atlascloud", "#5975F5"),
    ("vercel", "#FFFFFF"),
    ("llmman", "#6CC5B0"),
    ("aixy", "#123650"),
    ("raycast", "#FF6363"),
    ("xkiro", "#52C99B"),
    ("langdock", "#5A4AE7"),
    ("xapi", "#71767B"),
    ("museai", "#0668E1"),
    ("lithosai", "#6B7280"),
    ("workbuddy", "#0DC8A6"),
    ("tavily", "#78B0A1"),
    ("linkup", "#202020"),
    ("tinyapi", "#F97316"),
    ("exa", "#0143D9"),
    ("cosmic", "#29ABE2"),
    ("aerostack", "#6366F1"),
    ("sailresearch", "#2C4681"),
    ("sofya", "#B0B820"),
    ("fal", "#7C3AED"),
    ("flora", "#111111"),
    ("parallel", "#1D1C1A"),
    ("claudeplatform", "#CC7C5E"),
    ("soniox", "#253772"),
    ("futureagi", "#7857FC"),
    ("munito", "#702CF6"),
    ("traecn", "#4D7CFE"),
    ("chatbox", "#228BE6"),
    ("abliteration", "#1B1B1B"),
    ("opendesign", "#202020"),
    ("siliconflow", "#6E29F5"),
    ("tripo", "#F8CF00"),
];

pub fn accent_hex(id: &str) -> Option<&'static str> {
    ACCENTS
        .iter()
        .find(|(pid, _)| *pid == id)
        .map(|(_, hex)| *hex)
        .or_else(|| lookup(id).and_then(|(_, _, _, accent)| *accent))
}

/// Display names from `codexbar config providers` (covers every provider, incl. plugins).
static CATALOG_NAMES: std::sync::RwLock<Vec<(String, String)>> = std::sync::RwLock::new(Vec::new());

pub fn register_catalog(catalog: &[(String, String)]) {
    if let Ok(mut names) = CATALOG_NAMES.write() {
        *names = catalog.to_vec();
    }
}

/// Two-letter tag: known descriptor tag, else first two chars uppercased.
pub fn tag(id: &str) -> String {
    if let Some((_, _, tag, _)) = lookup(id) {
        return tag.to_string();
    }
    id.chars().take(2).collect::<String>().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_providers() {
        assert_eq!(display_name("codex"), "Codex");
        assert_eq!(display_name("claude"), "Claude");
        assert_eq!(display_name("copilot"), "GitHub Copilot");
        assert_eq!(accent_hex("codex"), Some("#49A3B0"));
        assert_eq!(accent_hex("claude"), Some("#CC7C5E"));
        assert_eq!(accent_hex("deepseek"), Some("#4D6BFE"));
        assert_eq!(tag("codex"), "CX");
        assert_eq!(tag("claude"), "CL");
    }
    #[test]
    fn fallback_titlecase() {
        assert_eq!(display_name("opencodego"), "Opencodego");
        assert_eq!(display_name(""), "Unknown provider");
        assert_eq!(accent_hex("nope"), None);
        assert_eq!(tag("gemini"), "GE");
    }
}
