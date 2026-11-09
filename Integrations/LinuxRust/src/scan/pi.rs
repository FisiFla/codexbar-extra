//! Pi / Oh-My-Pi agent sessions (`~/.pi/agent/sessions/**/*.jsonl`).
//!
//! Header line `{type:"session", id, cwd}`, then `{type:"message", message:{role, model,
//! provider, usage{input, output, cacheRead, cacheWrite}, content[{type:"toolCall", id, name}]}}`;
//! `toolResult` messages carry `toolCallId` and `isError`. Written from the format documented by
//! the Swift scanner; not verified against real Pi files (none present on this machine).

use super::pricing::{claude_cost, claude_rates, codex_cost, codex_rates};
use super::*;
use serde_json::Value;
use std::collections::HashMap;

fn field<'a>(o: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| o.get(*k))
}

pub fn parse_file(path: &Path) -> std::io::Result<FileFacts> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut facts = FileFacts::new(Provider::Pi, stem);
    let mut calls: HashMap<String, usize> = HashMap::new();
    for_each_line(path, |line| {
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("session") => {
                if let Some(id) = v.get("id").and_then(Value::as_str) {
                    facts.sid = id.to_string();
                }
                if let Some(n) = v.get("cwd").and_then(Value::as_str).and_then(project_name) {
                    facts.projects.insert(String::new(), n);
                }
            }
            Some("message") => {
                let Some(m) = v.get("message") else { return };
                let ts = v
                    .get("timestamp")
                    .or_else(|| m.get("timestamp"))
                    .and_then(|t| match t {
                        Value::String(s) => parse_ts_ms(s),
                        Value::Number(n) => n.as_f64().and_then(epoch_to_ms),
                        _ => None,
                    });
                let Some(ts) = ts else { return };
                match m.get("role").and_then(Value::as_str) {
                    Some("assistant") => assistant(&mut facts, &mut calls, &v, m, ts),
                    Some("toolResult") => {
                        let id = field(m, &["toolCallId", "tool_call_id"]).and_then(Value::as_str);
                        if let Some(&i) = id.and_then(|id| calls.get(id)) {
                            let t = &mut facts.tools[i];
                            t.e = field(m, &["isError", "is_error"])
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            if ts >= t.t {
                                t.d = Some(ts - t.t);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    })?;
    Ok(facts)
}

fn assistant(
    facts: &mut FileFacts,
    calls: &mut HashMap<String, usize>,
    entry: &Value,
    m: &Value,
    ts: i64,
) {
    if let Some(Value::Array(blocks)) = m.get("content") {
        for b in blocks {
            let ty = b.get("type").and_then(Value::as_str);
            if matches!(ty, Some("toolCall" | "tool_use")) {
                let name = b.get("name").and_then(Value::as_str).unwrap_or("tool");
                let id = b.get("id").and_then(Value::as_str).unwrap_or("");
                if !id.is_empty() {
                    calls.insert(id.to_string(), facts.tools.len());
                }
                facts.tools.push(Tool {
                    k: hash64(&["pi-tool", &facts.sid, id, &ts.to_string()]),
                    t: ts,
                    n: name.to_string(),
                    e: false,
                    d: None,
                    s: None,
                });
            }
        }
    }
    let Some(u) = m.get("usage") else { return };
    let get = |keys: &[&str]| nonneg(field(u, keys));
    let input = get(&["input", "inputTokens", "input_tokens"]);
    let read = get(&["cacheRead", "cache_read", "cacheReadTokens"]);
    let write = get(&["cacheWrite", "cache_write", "cacheWriteTokens"]);
    let out = get(&["output", "outputTokens", "output_tokens"]);
    if input + read + write + out == 0 {
        return;
    }
    let model = field(m, &["model", "modelId"])
        .or_else(|| field(entry, &["model", "modelId"]))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if model.is_empty() {
        return;
    }
    let provider = field(m, &["provider"])
        .or_else(|| field(entry, &["provider"]))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let lower = model.to_lowercase();
    let (cost, est) = if provider.contains("anthropic") || lower.contains("claude") {
        match claude_rates(&model, ts) {
            Some((r, est)) => (Some(claude_cost(&r, input, read, write, 0, out)), est),
            None => (None, false),
        }
    } else if provider.contains("openai") || lower.starts_with("gpt") {
        match codex_rates(&model, ts) {
            Some(r) => (
                Some(codex_cost(&r, input + read + write, read, write, out)),
                false,
            ),
            None => (None, false),
        }
    } else {
        (None, false)
    };
    let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
    facts.reqs.push(Req {
        k: if id.is_empty() {
            0
        } else {
            hash64(&["pi", &facts.sid, id])
        },
        t: ts,
        m: model,
        i: input,
        o: out,
        cr: read,
        cw: write,
        r: 0,
        c: cost,
        est,
        s: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::testutil::*;

    #[test]
    fn parses_session_usage_and_tool_calls() {
        let d = tmpdir("pi");
        let f = d.join("x.jsonl");
        write_lines(
            &f,
            &[
                r#"{"type":"session","id":"pi-1","cwd":"/w/demo","timestamp":"2026-11-01T10:00:00Z"}"#.into(),
                r#"{"type":"message","id":"e1","timestamp":"2026-11-01T10:00:01Z","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-4-8","content":[{"type":"text","text":"SECRET"},{"type":"toolCall","id":"c1","name":"bash","arguments":{"cmd":"SECRET"}}],"usage":{"input":10,"output":5,"cacheRead":100,"cacheWrite":20}}}"#.into(),
                r#"{"type":"message","id":"e2","timestamp":"2026-11-01T10:00:05Z","message":{"role":"toolResult","toolCallId":"c1","isError":true,"content":[{"type":"text","text":"SECRET"}]}}"#.into(),
                r#"{"type":"message","id":"e3","timestamp":"2026-11-01T10:00:06Z","message":{"role":"assistant","provider":"local","model":"mystery","usage":{"input":1,"output":1}}}"#.into(),
            ],
        );
        let facts = parse_file(&f).unwrap();
        assert_eq!(facts.sid, "pi-1");
        assert_eq!(facts.projects.get(""), Some(&"demo".to_string()));
        assert_eq!(facts.reqs.len(), 2);
        assert_eq!(
            (
                facts.reqs[0].i,
                facts.reqs[0].cr,
                facts.reqs[0].cw,
                facts.reqs[0].o
            ),
            (10, 100, 20, 5)
        );
        assert!(facts.reqs[0].c.is_some());
        assert_eq!(facts.reqs[1].c, None);
        assert_eq!(facts.tools.len(), 1);
        assert_eq!((facts.tools[0].e, facts.tools[0].d), (true, Some(4000)));
        assert!(!serde_json::to_string(&facts).unwrap().contains("SECRET"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
