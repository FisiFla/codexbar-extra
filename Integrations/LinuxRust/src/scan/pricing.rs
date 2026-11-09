//! List-price tables ported from `Sources/CodexBarCore/Vendored/CostUsage/CostUsagePricing.swift`
//! (bundled fallback tables only: the mac app additionally consults a downloaded models.dev
//! catalog, which has no Linux counterpart here). A model with no entry is *unpriced*: its cost
//! stays unknown (`None`) rather than becoming zero. Bump `PRICING_VERSION` whenever a rate
//! changes so cached per-file facts are re-derived.

pub const PRICING_VERSION: u32 = 1;

const CODEX_LONG_CONTEXT: i64 = 272_000;
const CLAUDE_LONG_CONTEXT: i64 = 200_000;
/// GPT-5.6 Terra/Luna historical rates apply before this instant (Unix seconds).
const GPT56_CUTOFF_SECS: i64 = 1_785_369_600;
/// GPT-5.6 Sol repricing instant (2026-08-21).
const SOL_CUTOFF_SECS: i64 = 1_787_270_400;
/// Claude 4.6 models dropped the long-context surcharge at this instant (2026-03-13).
const CLAUDE_FULL_CONTEXT_CUTOFF_SECS: i64 = 1_773_360_000;

/// Per-token USD rates. `*_hi` apply to a request whose prompt exceeds `threshold`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub threshold: Option<i64>,
    pub input_hi: Option<f64>,
    pub output_hi: Option<f64>,
    pub cache_read_hi: Option<f64>,
    pub cache_write_hi: Option<f64>,
}

const fn flat(input: f64, output: f64, cache_read: Option<f64>) -> Rates {
    Rates {
        input,
        output,
        cache_read,
        cache_write: None,
        threshold: None,
        input_hi: None,
        output_hi: None,
        cache_read_hi: None,
        cache_write_hi: None,
    }
}

/// Swift `longContextPricing`: above 272K prompt tokens input/cache are 2x and output is `out_hi`.
fn long_ctx(input: f64, cached: Option<f64>, write: Option<f64>, out: f64, out_hi: f64) -> Rates {
    Rates {
        input,
        output: out,
        cache_read: cached,
        cache_write: write,
        threshold: Some(CODEX_LONG_CONTEXT),
        input_hi: Some(input * 2.0),
        output_hi: Some(out_hi),
        cache_read_hi: cached.map(|c| c * 2.0),
        cache_write_hi: write.map(|w| w * 2.0),
    }
}

/// Strip a trailing `-YYYY-MM-DD`.
fn strip_codex_date(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    if b.len() < 11 {
        return None;
    }
    let tail = &s[s.len() - 11..];
    let t = tail.as_bytes();
    let ok = t[0] == b'-'
        && t[1..5].iter().all(u8::is_ascii_digit)
        && t[5] == b'-'
        && t[6..8].iter().all(u8::is_ascii_digit)
        && t[8] == b'-'
        && t[9..11].iter().all(u8::is_ascii_digit);
    ok.then(|| &s[..s.len() - 11])
}

fn codex_known(name: &str) -> bool {
    codex_base(name).is_some()
}

/// Standard (current) Codex/OpenAI rates by canonical model id.
fn codex_base(name: &str) -> Option<Rates> {
    let r = match name {
        "gpt-5" | "gpt-5-codex" | "gpt-5.1" | "gpt-5.1-codex" | "gpt-5.1-codex-max" => {
            flat(1.25e-6, 1e-5, Some(1.25e-7))
        }
        "gpt-5-mini" | "gpt-5.1-codex-mini" => flat(2.5e-7, 2e-6, Some(2.5e-8)),
        "gpt-5-nano" => flat(5e-8, 4e-7, Some(5e-9)),
        "gpt-5-pro" => flat(1.5e-5, 1.2e-4, None),
        "gpt-5.2" | "gpt-5.2-codex" | "gpt-5.3-codex" => flat(1.75e-6, 1.4e-5, Some(1.75e-7)),
        "gpt-5.2-pro" => flat(2.1e-5, 1.68e-4, None),
        // Research preview: priced at zero by design.
        "gpt-5.3-codex-spark" => flat(0.0, 0.0, Some(0.0)),
        "gpt-5.4" => long_ctx(2.5e-6, Some(2.5e-7), None, 1.5e-5, 2.25e-5),
        "gpt-5.4-mini" => flat(7.5e-7, 4.5e-6, Some(7.5e-8)),
        "gpt-5.4-nano" => flat(2e-7, 1.25e-6, Some(2e-8)),
        "gpt-5.4-pro" | "gpt-5.5-pro" => long_ctx(3e-5, None, None, 1.8e-4, 2.7e-4),
        "gpt-5.5" => long_ctx(5e-6, Some(5e-7), None, 3e-5, 4.5e-5),
        "gpt-6-astra" => long_ctx(1e-5, Some(1e-6), Some(1.25e-5), 5e-5, 7.5e-5),
        "gpt-6.1-sol" => long_ctx(2e-6, Some(1e-7), Some(2.5e-6), 1e-5, 1.5e-5),
        "gpt-6-sol" => long_ctx(2e-6, Some(2e-7), Some(2.5e-6), 1e-5, 1.5e-5),
        "gpt-6-luna" => long_ctx(1e-7, Some(1e-8), Some(1.25e-7), 5e-7, 7.5e-7),
        "gpt-5.6-sol" => long_ctx(4e-6, Some(4e-7), Some(5e-6), 2e-5, 3e-5),
        "gpt-5.6-terra" => long_ctx(2e-6, Some(2e-7), Some(2.5e-6), 1.2e-5, 1.8e-5),
        "gpt-5.6-luna" => long_ctx(2e-7, Some(2e-8), Some(2.5e-7), 1.2e-6, 1.8e-6),
        "gpt-5.6-cyber" => Rates {
            cache_write: Some(1.5625e-5),
            ..flat(1.25e-5, 7.5e-5, Some(1.25e-6))
        },
        "gpt-5.5-cyber" => flat(1.25e-5, 7.5e-5, Some(1.25e-6)),
        _ => return None,
    };
    Some(r)
}

/// Swift `normalizeCodexModel`.
pub fn normalize_codex_model(raw: &str) -> String {
    let mut m = raw.trim();
    if let Some(rest) = m.strip_prefix("openai/") {
        m = rest;
    }
    match m {
        "gpt-5.6" | "gpt-daybreak-blue-latest" => return "gpt-5.6-sol".into(),
        "gpt-reserve" => return "gpt-5.6-luna".into(),
        "gpt-daybreak-red-latest" => return "gpt-5.6-cyber".into(),
        _ => {}
    }
    if codex_known(m) {
        return m.to_string();
    }
    if let Some(base) = strip_codex_date(m) {
        if codex_known(base) {
            return base.to_string();
        }
    }
    m.to_string()
}

/// Rates for a Codex model at a point in time (`ts_ms`), or `None` when unpriced.
pub fn codex_rates(model: &str, ts_ms: i64) -> Option<Rates> {
    let name = normalize_codex_model(model);
    let secs = ts_ms.div_euclid(1000);
    match name.as_str() {
        "gpt-5.6-sol" if secs < SOL_CUTOFF_SECS => {
            return Some(long_ctx(5e-6, Some(5e-7), Some(6.25e-6), 3e-5, 4.5e-5))
        }
        "gpt-5.6-terra" if secs < GPT56_CUTOFF_SECS => {
            return Some(long_ctx(
                2.5e-6,
                Some(2.5e-7),
                Some(3.125e-6),
                1.5e-5,
                2.25e-5,
            ))
        }
        "gpt-5.6-luna" if secs < GPT56_CUTOFF_SECS => {
            return Some(long_ctx(1e-6, Some(1e-7), Some(1.25e-6), 6e-6, 9e-6))
        }
        _ => {}
    }
    codex_base(&name)
}

/// Swift `codexCostUSD`. OpenAI `input` is the whole prompt; cached reads (and cache writes) are
/// subsets of it, clamped so tokens are never invented or double-billed.
pub fn codex_cost(r: &Rates, input: i64, cached: i64, cache_write: i64, output: i64) -> f64 {
    let total = input.max(0);
    let cached = cached.max(0).min(total);
    let after_cache = total - cached;
    let write = cache_write.max(0).min(after_cache);
    let fresh = after_cache - write;
    let hi = r.threshold.is_some_and(|t| total > t);
    let input_rate = if hi {
        r.input_hi.unwrap_or(r.input)
    } else {
        r.input
    };
    let cached_rate = if hi {
        r.cache_read_hi.or(r.cache_read).unwrap_or(input_rate)
    } else {
        r.cache_read.unwrap_or(r.input)
    };
    let write_rate = if hi {
        r.cache_write_hi.or(r.cache_write).unwrap_or(input_rate)
    } else {
        r.cache_write.unwrap_or(input_rate)
    };
    let out_rate = if hi {
        r.output_hi.unwrap_or(r.output)
    } else {
        r.output
    };
    fresh as f64 * input_rate
        + cached as f64 * cached_rate
        + write as f64 * write_rate
        + output.max(0) as f64 * out_rate
}

fn claude(input: f64, output: f64, write: f64, read: f64) -> Rates {
    Rates {
        input,
        output,
        cache_read: Some(read),
        cache_write: Some(write),
        ..Rates::default()
    }
}

fn claude_with_long(input: f64, output: f64, write: f64, read: f64) -> Rates {
    Rates {
        threshold: Some(CLAUDE_LONG_CONTEXT),
        input_hi: Some(input * 2.0),
        output_hi: Some(output * 1.5),
        cache_read_hi: Some(read * 2.0),
        cache_write_hi: Some(write * 2.0),
        ..claude(input, output, write, read)
    }
}

/// Swift `normalizeClaudeModel`: drop `anthropic.` / vendor prefixes, Bedrock `-vN:M` suffix and
/// a `-YYYYMMDD` snapshot suffix when the undated id is a known model.
pub fn normalize_claude_model(raw: &str) -> String {
    let mut m = raw.trim().to_string();
    if let Some(rest) = m.strip_prefix("anthropic.") {
        m = rest.to_string();
    }
    if m.contains("claude-") {
        if let Some(dot) = m.rfind('.') {
            let tail = &m[dot + 1..];
            if tail.starts_with("claude-") {
                m = tail.to_string();
            }
        }
    }
    // `-v1:0`
    if let Some(pos) = m.rfind("-v") {
        let tail = &m[pos + 2..];
        if let Some((a, b)) = tail.split_once(':') {
            if !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())
            {
                m.truncate(pos);
            }
        }
    }
    if claude_base(&m).is_some() {
        return m;
    }
    if m.len() > 9 {
        let (base, tail) = m.split_at(m.len() - 9);
        if tail.starts_with('-')
            && tail[1..].bytes().all(|c| c.is_ascii_digit())
            && claude_base(base).is_some()
        {
            return base.to_string();
        }
    }
    m
}

fn claude_base(name: &str) -> Option<Rates> {
    let r = match name {
        "claude-fable-5" | "claude-mythos-5" => claude(1e-5, 5e-5, 1.25e-5, 1e-6),
        "claude-mythos-5-1" => claude(1e-5, 5e-5, 1.25e-5, 2.5e-7),
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => claude(1e-6, 5e-6, 1.25e-6, 1e-7),
        "claude-opus-4-5"
        | "claude-opus-4-5-20251101"
        | "claude-opus-4-6"
        | "claude-opus-4-6-20260205"
        | "claude-opus-4-7"
        | "claude-opus-4-8" => claude(5e-6, 2.5e-5, 6.25e-6, 5e-7),
        "claude-opus-4-1" | "claude-opus-4-20250514" => claude(1.5e-5, 7.5e-5, 1.875e-5, 1.5e-6),
        "claude-sonnet-4-5" | "claude-sonnet-4-5-20250929" | "claude-sonnet-4-20250514" => {
            claude_with_long(3e-6, 1.5e-5, 3.75e-6, 3e-7)
        }
        "claude-sonnet-4-6" => claude(3e-6, 1.5e-5, 3.75e-6, 3e-7),
        _ => return None,
    };
    Some(r)
}

/// Rates for a Claude model. The bool is true when the id is not in the bundled table and the
/// rates are the newest known rates of the same family (opus/sonnet/haiku/fable/mythos), so the
/// resulting cost is an estimate. `None` = unpriced.
pub fn claude_rates(model: &str, ts_ms: i64) -> Option<(Rates, bool)> {
    let name = normalize_claude_model(model);
    let secs = ts_ms.div_euclid(1000);
    if secs < CLAUDE_FULL_CONTEXT_CUTOFF_SECS {
        match name.as_str() {
            "claude-opus-4-6" => {
                return Some((claude_with_long(5e-6, 2.5e-5, 6.25e-6, 5e-7), false))
            }
            "claude-sonnet-4-6" => {
                return Some((claude_with_long(3e-6, 1.5e-5, 3.75e-6, 3e-7), false))
            }
            _ => {}
        }
    }
    if let Some(r) = claude_base(&name) {
        return Some((r, false));
    }
    let fam = if name.contains("opus") {
        claude_base("claude-opus-4-8")
    } else if name.contains("sonnet") {
        claude_base("claude-sonnet-4-6")
    } else if name.contains("haiku") {
        claude_base("claude-haiku-4-5")
    } else if name.contains("fable") || name.contains("mythos") {
        claude_base("claude-fable-5")
    } else {
        None
    };
    fam.map(|r| (r, true))
}

/// Swift `claudeCostUSD(pricing:tokens:)`: `input`/`cache_*` are disjoint counts.
pub fn claude_cost(
    r: &Rates,
    input: i64,
    cache_read: i64,
    cache_create: i64,
    cache_create_1h: i64,
    output: i64,
) -> f64 {
    let input = input.max(0);
    let read = cache_read.max(0);
    let create = cache_create.max(0);
    let create_1h = cache_create_1h.max(0).min(create);
    let create_5m = create - create_1h;
    let hi = r
        .threshold
        .is_some_and(|t| input.saturating_add(read).saturating_add(create) > t);
    let in_rate = if hi {
        r.input_hi.unwrap_or(r.input)
    } else {
        r.input
    };
    let read_rate = if hi {
        r.cache_read_hi.or(r.cache_read).unwrap_or(in_rate)
    } else {
        r.cache_read.unwrap_or(in_rate)
    };
    let write_rate = if hi {
        r.cache_write_hi.or(r.cache_write).unwrap_or(in_rate)
    } else {
        r.cache_write.unwrap_or(in_rate)
    };
    let out_rate = if hi {
        r.output_hi.unwrap_or(r.output)
    } else {
        r.output
    };
    input as f64 * in_rate
        + read as f64 * read_rate
        + create_5m as f64 * write_rate
        + create_1h as f64 * in_rate * 2.0
        + output.max(0) as f64 * out_rate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn codex_cost_splits_cached_subset_of_input() {
        let r = codex_rates("gpt-5.4", 0).unwrap();
        // 1000 prompt tokens, 400 cached: 600 fresh @2.5e-6 + 400 @2.5e-7 + 100 out @1.5e-5
        approx(
            codex_cost(&r, 1000, 400, 0, 100),
            600.0 * 2.5e-6 + 400.0 * 2.5e-7 + 100.0 * 1.5e-5,
        );
        // cached > input is clamped, never negative fresh tokens
        approx(codex_cost(&r, 100, 500, 0, 0), 100.0 * 2.5e-7);
    }

    #[test]
    fn codex_long_context_applies_above_272k() {
        let r = codex_rates("gpt-5.4", 0).unwrap();
        approx(codex_cost(&r, 272_000, 0, 0, 0), 272_000.0 * 2.5e-6);
        approx(
            codex_cost(&r, 272_001, 0, 0, 10),
            272_001.0 * 5e-6 + 10.0 * 2.25e-5,
        );
    }

    #[test]
    fn codex_model_aliases_and_dates() {
        assert_eq!(normalize_codex_model("openai/gpt-5.4"), "gpt-5.4");
        assert_eq!(normalize_codex_model("gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(normalize_codex_model("gpt-5.6"), "gpt-5.6-sol");
        assert_eq!(normalize_codex_model("gpt-unknown-1"), "gpt-unknown-1");
        assert!(codex_rates("gpt-unknown-1", 0).is_none());
        // spark is a priced zero, not unknown
        approx(
            codex_cost(
                &codex_rates("gpt-5.3-codex-spark", 0).unwrap(),
                1000,
                0,
                0,
                1000,
            ),
            0.0,
        );
    }

    #[test]
    fn codex_historical_sol_pricing_switches_at_cutoff() {
        let before = codex_rates("gpt-5.6-sol", (SOL_CUTOFF_SECS - 1) * 1000).unwrap();
        let after = codex_rates("gpt-5.6-sol", SOL_CUTOFF_SECS * 1000).unwrap();
        approx(before.input, 5e-6);
        approx(after.input, 4e-6);
    }

    #[test]
    fn claude_cost_components_and_1h_cache() {
        let (r, est) = claude_rates("claude-opus-4-8", 0).unwrap();
        assert!(!est);
        // 100 in, 1000 read, 200 create (50 of them 1h), 10 out
        let expect =
            100.0 * 5e-6 + 1000.0 * 5e-7 + 150.0 * 6.25e-6 + 50.0 * 5e-6 * 2.0 + 10.0 * 2.5e-5;
        approx(claude_cost(&r, 100, 1000, 200, 50, 10), expect);
    }

    #[test]
    fn claude_normalizes_snapshot_and_bedrock_ids() {
        assert_eq!(
            normalize_claude_model("claude-opus-4-5-20251101"),
            "claude-opus-4-5-20251101"
        );
        assert_eq!(
            normalize_claude_model("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5-20250929"
        );
        assert_eq!(
            normalize_claude_model("claude-haiku-4-5-20260101"),
            "claude-haiku-4-5"
        );
        assert_eq!(
            normalize_claude_model("anthropic.claude-sonnet-4-6-v1:0"),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            normalize_claude_model("eu.anthropic.claude-opus-4-8-v2:0"),
            "claude-opus-4-8"
        );
    }

    #[test]
    fn claude_unknown_family_is_estimate_and_unknown_model_is_unpriced() {
        let (r, est) = claude_rates("claude-opus-9-9", 0).unwrap();
        assert!(est);
        approx(r.input, 5e-6);
        assert!(claude_rates("<synthetic>", 0).is_none());
        assert!(claude_rates("totally-different", 0).is_none());
    }

    #[test]
    fn claude_sonnet_long_context_and_historical_46() {
        let (r, _) = claude_rates("claude-sonnet-4-5", 0).unwrap();
        approx(claude_cost(&r, 250_000, 0, 0, 0, 0), 250_000.0 * 6e-6);
        let (old, _) = claude_rates("claude-sonnet-4-6", 1_700_000_000_000).unwrap();
        assert_eq!(old.threshold, Some(200_000));
        let (new, _) = claude_rates("claude-sonnet-4-6", 1_800_000_000_000).unwrap();
        assert_eq!(new.threshold, None);
    }
}
