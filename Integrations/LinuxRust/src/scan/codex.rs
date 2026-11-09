//! Codex rollouts (`~/.codex/sessions/**/rollout-*.jsonl`).
//!
//! Lines are `{timestamp, type, payload}`. `token_count` events carry per-request usage
//! (`last_token_usage`, whose input *includes* cached reads), `turn_context` names the model and
//! reasoning effort, `task_started`/`task_complete` bracket a turn with timing, and
//! `function_call`/`custom_tool_call` + their `*_output` records are tool invocations.

use super::pricing::{codex_cost, codex_rates};
use super::*;
use serde_json::Value;
use std::collections::HashMap;

fn num(v: Option<&Value>) -> i64 {
    nonneg(v)
}

pub fn parse_file(path: &Path) -> std::io::Result<FileFacts> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut facts = FileFacts::new(Provider::Codex, stem);
    let mut model = String::new();
    let mut effort: Option<String> = None;
    let mut last_total: Option<i64> = None;
    // per-turn accumulators (reset at task_started)
    let (mut t_out, mut t_in, mut t_cached) = (0i64, 0i64, 0i64);
    let mut calls: HashMap<String, usize> = HashMap::new();
    for_each_line(path, |line| {
        let ty = quick_str(line, "type");
        // Fast skips: bulky message/reasoning records never matter.
        if ty == Some("response_item")
            && !(contains(line, "\"function_call")
                || contains(line, "\"custom_tool_call")
                || contains(line, "\"web_search_call\""))
        {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let Some(ts) = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts_ms)
        else {
            return;
        };
        let p = v.get("payload").unwrap_or(&Value::Null);
        let pty = p.get("type").and_then(Value::as_str).unwrap_or("");
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "session_meta" => {
                if let Some(id) = p.get("id").and_then(Value::as_str) {
                    facts.sid = id.to_string();
                }
                if let Some(name) = p.get("cwd").and_then(Value::as_str).and_then(project_name) {
                    facts.projects.insert(String::new(), name);
                }
            }
            "turn_context" => {
                if let Some(m) = p.get("model").and_then(Value::as_str) {
                    model = m.to_string();
                }
                effort = p
                    .get("effort")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(effort.take());
            }
            "event_msg" => match pty {
                "task_started" => {
                    (t_out, t_in, t_cached) = (0, 0, 0);
                }
                "token_count" => {
                    let Some(info) = p.get("info").filter(|i| i.is_object()) else {
                        return;
                    };
                    let total = info.get("total_token_usage");
                    let total_tokens = num(total.and_then(|t| t.get("total_tokens")));
                    // Repeated events re-report an unchanged cumulative total.
                    if last_total == Some(total_tokens) {
                        return;
                    }
                    last_total = Some(total_tokens);
                    let Some(last) = info.get("last_token_usage") else {
                        return;
                    };
                    let input_all = num(last.get("input_tokens"));
                    let cached = num(last.get("cached_input_tokens")).min(input_all);
                    let write = num(last.get("cache_write_input_tokens")).min(input_all - cached);
                    let out = num(last.get("output_tokens"));
                    let reasoning = num(last.get("reasoning_output_tokens")).min(out);
                    let (cost, est) = match codex_rates(&model, ts) {
                        Some(r) => (Some(codex_cost(&r, input_all, cached, write, out)), false),
                        None => (None, false),
                    };
                    t_out += out;
                    t_in += input_all;
                    t_cached += cached;
                    facts.reqs.push(Req {
                        k: hash64(&[
                            "codex",
                            &ts.to_string(),
                            &total_tokens.to_string(),
                            &input_all.to_string(),
                            &out.to_string(),
                        ]),
                        t: ts,
                        m: if model.is_empty() {
                            "unknown".into()
                        } else {
                            model.clone()
                        },
                        i: input_all - cached - write,
                        o: out,
                        cr: cached,
                        cw: write,
                        r: reasoning,
                        c: cost,
                        est,
                        s: None,
                    });
                }
                "task_complete" => {
                    let dur = p.get("duration_ms").and_then(Value::as_i64).unwrap_or(0);
                    let ok = p.get("error").is_none_or(Value::is_null);
                    if ok && dur > 0 {
                        facts.turns.push(Turn {
                            t: ts,
                            m: (!model.is_empty()).then(|| model.clone()),
                            e: effort.clone(),
                            d: dur,
                            f: p.get("time_to_first_token_ms").and_then(Value::as_i64),
                            o: t_out,
                            i: Some(t_in),
                            c: Some(t_cached),
                            s: None,
                        });
                    }
                }
                _ => {}
            },
            "response_item" => match pty {
                "function_call" | "custom_tool_call" => {
                    let name = p.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let call = p.get("call_id").and_then(Value::as_str).unwrap_or("");
                    if !call.is_empty() {
                        calls.insert(call.to_string(), facts.tools.len());
                    }
                    facts.tools.push(Tool {
                        k: hash64(&["codex-tool", &facts.sid, call, &ts.to_string()]),
                        t: ts,
                        n: name.to_string(),
                        e: false,
                        d: None,
                        s: None,
                    });
                }
                "web_search_call" => facts.tools.push(Tool {
                    k: hash64(&["codex-search", &facts.sid, &ts.to_string()]),
                    t: ts,
                    n: "web_search".into(),
                    e: false,
                    d: None,
                    s: None,
                }),
                "function_call_output" | "custom_tool_call_output" => {
                    let call = p.get("call_id").and_then(Value::as_str).unwrap_or("");
                    if let Some(&i) = calls.get(call) {
                        let t = &mut facts.tools[i];
                        if ts >= t.t {
                            t.d = Some(ts - t.t);
                        }
                        if let Some(out) = p.get("output").and_then(Value::as_str) {
                            t.e = exited_nonzero(out);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    })?;
    Ok(facts)
}

/// Codex shell outputs end with `Process exited with code N`; only the number is inspected.
fn exited_nonzero(out: &str) -> bool {
    out.find("exited with code ")
        .map(|i| {
            let rest = &out[i + "exited with code ".len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            !digits.is_empty() && digits != "0"
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::testutil::*;

    fn tc(ts: &str, total: i64, inp: i64, cached: i64, out: i64, reason: i64) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{inp},"cached_input_tokens":{cached},"output_tokens":{out},"reasoning_output_tokens":{reason},"total_tokens":{total}}},"last_token_usage":{{"input_tokens":{inp},"cached_input_tokens":{cached},"cache_write_input_tokens":0,"output_tokens":{out},"reasoning_output_tokens":{reason},"total_tokens":{total}}}}}}}}}"#
        )
    }

    fn fixture() -> Vec<String> {
        vec![
            r#"{"timestamp":"2026-11-01T10:00:00.000Z","type":"session_meta","payload":{"id":"sess-c1","cwd":"/w/demo","base_instructions":{"text":"SECRET"}}}"#.into(),
            r#"{"timestamp":"2026-11-01T10:00:01.000Z","type":"turn_context","payload":{"model":"gpt-5.4","effort":"high"}}"#.into(),
            r#"{"timestamp":"2026-11-01T10:00:01.500Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1","started_at":1}}"#.into(),
            r#"{"timestamp":"2026-11-01T10:00:02.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"SECRET","call_id":"c1"}}"#.into(),
            r#"{"timestamp":"2026-11-01T10:00:05.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"SECRET\nProcess exited with code 2"}}"#.into(),
            r#"{"timestamp":"2026-11-01T10:00:06.000Z","type":"response_item","payload":{"type":"message","content":"SECRET TEXT"}}"#.into(),
            tc("2026-11-01T10:00:07.000Z", 1100, 1000, 600, 100, 40),
            tc("2026-11-01T10:00:07.500Z", 1100, 1000, 600, 100, 40),
            r#"{"timestamp":"2026-11-01T10:00:08.000Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t1","last_agent_message":"SECRET","duration_ms":6500,"time_to_first_token_ms":1200}}"#.into(),
        ]
    }

    #[test]
    fn parses_requests_turns_tools_and_drops_text() {
        let d = tmpdir("cx1");
        let f = d.join("rollout-x.jsonl");
        write_lines(&f, &fixture());
        let facts = parse_file(&f).unwrap();
        assert_eq!(facts.sid, "sess-c1");
        assert_eq!(facts.projects.get(""), Some(&"demo".to_string()));
        // duplicate token_count (same cumulative total) collapses
        assert_eq!(facts.reqs.len(), 1);
        let r = &facts.reqs[0];
        assert_eq!((r.i, r.cr, r.cw, r.o, r.r), (400, 600, 0, 100, 40));
        assert_eq!(r.total(), 1100);
        // gpt-5.4: 2.5/15/0.25 per M
        let want = 400.0 * 2.5e-6 + 600.0 * 2.5e-7 + 100.0 * 1.5e-5;
        assert!((r.c.unwrap() - want).abs() < 1e-12);
        assert_eq!(facts.turns.len(), 1);
        let t = &facts.turns[0];
        assert_eq!(
            (t.d, t.f, t.o, t.e.as_deref()),
            (6500, Some(1200), 100, Some("high"))
        );
        assert_eq!((t.i, t.c), (Some(1000), Some(600)));
        assert_eq!(facts.tools.len(), 1);
        let tool = &facts.tools[0];
        assert_eq!(
            (tool.n.as_str(), tool.d, tool.e),
            ("exec_command", Some(3000), true)
        );
        assert!(!serde_json::to_string(&facts).unwrap().contains("SECRET"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unknown_model_is_unpriced_and_failed_turn_is_not_a_sample() {
        let d = tmpdir("cx2");
        let f = d.join("rollout-y.jsonl");
        write_lines(
            &f,
            &[
                r#"{"timestamp":"2026-11-01T10:00:01.000Z","type":"turn_context","payload":{"model":"zzz-made-up"}}"#.to_string(),
                tc("2026-11-01T10:00:07.000Z", 10, 8, 0, 2, 0),
                r#"{"timestamp":"2026-11-01T10:00:08.000Z","type":"event_msg","payload":{"type":"task_complete","duration_ms":900,"error":{"message":"x"}}}"#.to_string(),
            ],
        );
        let facts = parse_file(&f).unwrap();
        assert_eq!(facts.reqs[0].c, None);
        assert!(facts.turns.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn exit_code_detection() {
        assert!(exited_nonzero("x\nProcess exited with code 127"));
        assert!(!exited_nonzero("Process exited with code 0"));
        assert!(!exited_nonzero("no marker"));
    }
}
