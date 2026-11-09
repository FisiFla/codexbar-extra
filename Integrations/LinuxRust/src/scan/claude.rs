//! Claude Code transcripts (`~/.claude/projects/**/*.jsonl`).
//!
//! Assistant lines carry `message.usage` (disjoint input/cache counts) and `tool_use` content
//! blocks; user lines carry the matching `tool_result` blocks. Only ids, names, counters and
//! timestamps are read; text blocks are skipped by the deserializer and never stored.

use super::pricing::{claude_cost, claude_rates};
use super::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Default)]
struct Line {
    #[serde(rename = "type", default)]
    ty: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "sessionId", default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(rename = "requestId", default)]
    request_id: Option<String>,
    #[serde(default)]
    message: Option<Msg>,
}

#[derive(Deserialize, Default)]
struct Msg {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    content: Option<Content>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Blocks(Vec<Block>),
    Text(#[allow(dead_code)] String),
}

#[derive(Deserialize, Default)]
struct Block {
    #[serde(rename = "type", default)]
    ty: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    is_error: Option<bool>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<i64>,
    #[serde(default)]
    cache_read_input_tokens: Option<i64>,
    #[serde(default)]
    cache_creation: Option<CacheCreation>,
}

#[derive(Deserialize, Default)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_1h_input_tokens: Option<i64>,
}

pub fn parse_file(path: &Path) -> std::io::Result<FileFacts> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut facts = FileFacts::new(Provider::Claude, stem.clone());
    parse_lines(path, &stem, &mut facts)?;
    Ok(facts)
}

fn parse_lines(path: &Path, stem: &str, facts: &mut FileFacts) -> std::io::Result<()> {
    let mut req_index: HashMap<u64, usize> = HashMap::new();
    // tool_use id -> index into facts.tools (to attach result status and duration)
    let mut tool_index: HashMap<String, usize> = HashMap::new();
    for_each_line(path, |line| {
        // Cheap prefilter: only assistant usage rows and tool blocks matter.
        let has_usage = contains(line, "\"usage\"");
        let has_tool = contains(line, "\"tool_use\"") || contains(line, "\"tool_result\"");
        if !has_usage && !has_tool {
            return;
        }
        let Ok(l) = serde_json::from_slice::<Line>(line) else {
            return;
        };
        let Some(ts) = l.timestamp.as_deref().and_then(parse_ts_ms) else {
            return;
        };
        let sid = l
            .session_id
            .as_deref()
            .filter(|s| *s != stem)
            .map(str::to_string);
        if let Some(cwd) = l.cwd.as_deref() {
            if let Some(name) = project_name(cwd) {
                let key = sid.clone().unwrap_or_default();
                facts.projects.entry(key).or_insert(name);
            }
        }
        let Some(msg) = l.message else { return };
        let real_sid = sid.clone().unwrap_or_else(|| stem.to_string());

        if l.ty.as_deref() == Some("assistant") {
            if let Some(u) = &msg.usage {
                let model = msg.model.clone().unwrap_or_default();
                if !model.is_empty() && model != "<synthetic>" {
                    let key = match (&msg.id, &l.request_id) {
                        (Some(m), Some(r)) => hash64(&["claude", m, r]),
                        (Some(m), None) => hash64(&["claude", &real_sid, m]),
                        _ => 0,
                    };
                    let input = u.input_tokens.unwrap_or(0).max(0);
                    let out = u.output_tokens.unwrap_or(0).max(0);
                    let cw = u.cache_creation_input_tokens.unwrap_or(0).max(0);
                    let cr = u.cache_read_input_tokens.unwrap_or(0).max(0);
                    let cw1h = u
                        .cache_creation
                        .as_ref()
                        .and_then(|c| c.ephemeral_1h_input_tokens)
                        .unwrap_or(0)
                        .max(0);
                    let (cost, est) = match claude_rates(&model, ts) {
                        Some((r, est)) => (Some(claude_cost(&r, input, cr, cw, cw1h, out)), est),
                        None => (None, false),
                    };
                    upsert_req(
                        facts,
                        &mut req_index,
                        Req {
                            k: key,
                            t: ts,
                            m: model,
                            i: input,
                            o: out,
                            cr,
                            cw,
                            r: 0,
                            c: cost,
                            est,
                            s: sid.clone(),
                        },
                    );
                }
            }
        }

        if let Some(Content::Blocks(blocks)) = msg.content {
            for b in blocks {
                match b.ty.as_deref() {
                    Some("tool_use") => {
                        let (Some(id), Some(name)) = (b.id, b.name) else {
                            continue;
                        };
                        if tool_index.contains_key(&id) {
                            continue;
                        }
                        tool_index.insert(id.clone(), facts.tools.len());
                        facts.tools.push(Tool {
                            k: hash64(&["claude-tool", &id]),
                            t: ts,
                            n: name,
                            e: false,
                            d: None,
                            s: sid.clone(),
                        });
                    }
                    Some("tool_result") => {
                        if let Some(&i) = b.tool_use_id.as_ref().and_then(|id| tool_index.get(id)) {
                            let t = &mut facts.tools[i];
                            t.e = b.is_error.unwrap_or(false);
                            let d = ts - t.t;
                            if d >= 0 {
                                t.d = Some(d);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::testutil::*;

    fn asst(ts: &str, msg: &str, req: &str, model: &str, extra: &str, usage: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","cwd":"/w/proj","requestId":"{req}","message":{{"id":"{msg}","model":"{model}","role":"assistant","content":[{{"type":"text","text":"hi"}}{extra}],"usage":{usage}}}}}"#
        )
    }

    #[test]
    fn parses_usage_cost_project_and_dedupes_streamed_rows() {
        let d = tmpdir("cl1");
        let f = d.join("s1.jsonl");
        let u1 = r#"{"input_tokens":100,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#;
        let u2 = r#"{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":1000,"cache_read_input_tokens":2000,"cache_creation":{"ephemeral_1h_input_tokens":400}}"#;
        write_lines(
            &f,
            &[
                asst("2026-11-01T10:00:00Z", "m1", "r1", "claude-opus-4-8", "", u1),
                asst("2026-11-01T10:00:02Z", "m1", "r1", "claude-opus-4-8", "", u2),
                asst("2026-11-01T10:01:00Z", "m2", "r2", "<synthetic>", "", u1),
                asst("2026-11-01T10:02:00Z", "m3", "r3", "claude-mystery-9", "", u1),
                r#"{"type":"user","timestamp":"2026-11-01T10:03:00Z","message":{"role":"user","content":"plain text prompt"}}"#.to_string(),
            ],
        );
        let facts = parse_file(&f).unwrap();
        assert_eq!(facts.sid, "s1");
        assert_eq!(facts.projects.get(""), Some(&"proj".to_string()));
        // streamed duplicate collapses to the last row; synthetic model is dropped
        assert_eq!(facts.reqs.len(), 2);
        let r = &facts.reqs[0];
        assert_eq!((r.i, r.o, r.cr, r.cw), (100, 50, 2000, 1000));
        assert_eq!(r.total(), 3150);
        // opus 4.8: 5/25/6.25/0.5 per M; 600 5m + 400 1h(2x input = 10/M)
        let want = 100.0 * 5e-6 + 50.0 * 2.5e-5 + 2000.0 * 5e-7 + 600.0 * 6.25e-6 + 400.0 * 1e-5;
        assert!((r.c.unwrap() - want).abs() < 1e-12, "{:?} vs {want}", r.c);
        assert!(!r.est);
        // unknown model stays unpriced
        assert_eq!(facts.reqs[1].c, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn family_fallback_is_flagged_as_estimate() {
        let d = tmpdir("cl2");
        let f = d.join("s1.jsonl");
        let u = r#"{"input_tokens":10,"output_tokens":10}"#;
        write_lines(
            &f,
            &[asst(
                "2026-11-01T10:00:00Z",
                "m",
                "r",
                "claude-sonnet-9-9",
                "",
                u,
            )],
        );
        let facts = parse_file(&f).unwrap();
        assert!(facts.reqs[0].est);
        assert!(facts.reqs[0].c.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tool_results_attach_status_and_duration_without_text() {
        let d = tmpdir("cl3");
        let f = d.join("s1.jsonl");
        let u = r#"{"input_tokens":1,"output_tokens":1}"#;
        let tu = r#",{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"SECRET"}},{"type":"tool_use","id":"tu2","name":"Read","input":{}}"#;
        write_lines(
            &f,
            &[
                asst("2026-11-01T10:00:00Z", "m1", "r1", "claude-opus-4-8", tu, u),
                r#"{"type":"user","timestamp":"2026-11-01T10:00:12Z","sessionId":"s1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":"SECRET OUTPUT","is_error":true},{"type":"tool_result","tool_use_id":"tu2","content":"x"}]}}"#.to_string(),
            ],
        );
        let facts = parse_file(&f).unwrap();
        assert_eq!(facts.tools.len(), 2);
        let bash = &facts.tools[0];
        assert_eq!(
            (bash.n.as_str(), bash.e, bash.d),
            ("Bash", true, Some(12_000))
        );
        assert!(!facts.tools[1].e);
        let dump = serde_json::to_string(&facts).unwrap();
        assert!(!dump.contains("SECRET"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn other_session_ids_become_row_overrides() {
        let d = tmpdir("cl4");
        let f = d.join("agent-x.jsonl");
        let u = r#"{"input_tokens":1,"output_tokens":1}"#;
        write_lines(
            &f,
            &[asst(
                "2026-11-01T10:00:00Z",
                "m1",
                "r1",
                "claude-opus-4-8",
                "",
                u,
            )],
        );
        let facts = parse_file(&f).unwrap();
        assert_eq!(facts.sid, "agent-x");
        assert_eq!(facts.reqs[0].s.as_deref(), Some("s1"));
        assert_eq!(facts.projects.get("s1"), Some(&"proj".to_string()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
