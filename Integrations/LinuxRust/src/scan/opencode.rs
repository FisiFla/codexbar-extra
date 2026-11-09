//! OpenCode local history (`~/.local/share/opencode/opencode.db`, SQLite), read-only.
//!
//! `message.data` JSON for assistant rows holds `modelID`, `tokens {input, output, reasoning,
//! cache{read,write}}` (disjoint), the app-computed `cost` and `time.created`. Tool calls are
//! `part` rows with `type = 'tool'`. Titles, prompts and tool I/O are never selected.

use super::*;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;

pub fn parse_db(path: &Path) -> Result<FileFacts, rusqlite::Error> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(500))?;
    let mut facts = FileFacts::new(Provider::OpenCode, "opencode".into());

    let mut dirs: HashMap<String, String> = HashMap::new();
    if let Ok(mut st) = conn.prepare("SELECT id, directory FROM session") {
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for (id, dir) in rows.flatten() {
            if let Some(name) = project_name(&dir) {
                dirs.insert(id, name);
            }
        }
    }

    let mut st = conn.prepare(
        "SELECT id, session_id,
                CAST(COALESCE(json_extract(data,'$.time.created'), time_created) AS INTEGER),
                COALESCE(json_extract(data,'$.modelID'), ''),
                COALESCE(json_extract(data,'$.tokens.input'), 0),
                COALESCE(json_extract(data,'$.tokens.output'), 0),
                COALESCE(json_extract(data,'$.tokens.reasoning'), 0),
                COALESCE(json_extract(data,'$.tokens.cache.read'), 0),
                COALESCE(json_extract(data,'$.tokens.cache.write'), 0),
                json_extract(data,'$.cost')
         FROM message
         WHERE json_extract(data,'$.role') = 'assistant'
           AND json_type(data,'$.tokens') = 'object'",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, f64>(4)? as i64,
            r.get::<_, f64>(5)? as i64,
            r.get::<_, f64>(6)? as i64,
            r.get::<_, f64>(7)? as i64,
            r.get::<_, f64>(8)? as i64,
            r.get::<_, Option<f64>>(9)?,
        ))
    })?;
    for (id, sid, t, model, input, output, reasoning, read, write, cost) in rows.flatten() {
        if model.is_empty() {
            continue;
        }
        let output = output.max(0);
        let reasoning = reasoning.max(0);
        facts.reqs.push(Req {
            k: hash64(&["opencode", &id]),
            t,
            m: model,
            i: input.max(0),
            o: output + reasoning,
            cr: read.max(0),
            cw: write.max(0),
            r: reasoning,
            c: cost.filter(|c| c.is_finite() && *c >= 0.0),
            est: false,
            s: Some(sid),
        });
    }

    let mut st = conn.prepare(
        "SELECT id, session_id,
                CAST(COALESCE(json_extract(data,'$.state.time.start'), time_created) AS INTEGER),
                json_extract(data,'$.tool'),
                json_extract(data,'$.state.status'),
                json_extract(data,'$.state.time.start'),
                json_extract(data,'$.state.time.end')
         FROM part WHERE json_extract(data,'$.type') = 'tool'",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<f64>>(5)?,
            r.get::<_, Option<f64>>(6)?,
        ))
    })?;
    for (id, sid, t, name, status, start, end) in rows.flatten() {
        let d = match (start, end) {
            (Some(s), Some(e)) if e >= s => Some((e - s) as i64),
            _ => None,
        };
        facts.tools.push(Tool {
            k: hash64(&["opencode-tool", &id]),
            t,
            n: name.unwrap_or_else(|| "tool".into()),
            e: status.as_deref() == Some("error"),
            d,
            s: Some(sid),
        });
    }

    for (sid, name) in dirs {
        facts.projects.insert(sid, name);
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::testutil::*;

    fn fixture(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            "CREATE TABLE session(id text primary key, directory text not null);
             CREATE TABLE message(id text primary key, session_id text, time_created integer, data text);
             CREATE TABLE part(id text primary key, session_id text, time_created integer, data text);
             INSERT INTO session VALUES('s1','/w/demo');
             INSERT INTO message VALUES('m1','s1',1000,'{\"role\":\"assistant\",\"modelID\":\"fake-model\",\"cost\":0.5,\"time\":{\"created\":1786116646487},\"tokens\":{\"input\":10,\"output\":5,\"reasoning\":3,\"cache\":{\"read\":100,\"write\":7}}}');
             INSERT INTO message VALUES('m2','s1',2000,'{\"role\":\"user\"}');
             INSERT INTO part VALUES('p1','s1',1500,'{\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"error\",\"time\":{\"start\":1000,\"end\":13000},\"input\":{\"command\":\"SECRET\"}}}');
             INSERT INTO part VALUES('p2','s1',1600,'{\"type\":\"text\",\"text\":\"SECRET\"}');",
        )
        .unwrap();
    }

    #[test]
    fn reads_messages_tools_and_projects_read_only() {
        let d = tmpdir("oc");
        let db = d.join("opencode.db");
        fixture(&db);
        let facts = parse_db(&db).unwrap();
        assert_eq!(facts.reqs.len(), 1);
        let r = &facts.reqs[0];
        assert_eq!((r.i, r.o, r.cr, r.cw, r.r), (10, 8, 100, 7, 3));
        assert_eq!(r.c, Some(0.5));
        assert_eq!(r.s.as_deref(), Some("s1"));
        assert_eq!(facts.tools.len(), 1);
        assert_eq!(
            (
                facts.tools[0].n.as_str(),
                facts.tools[0].e,
                facts.tools[0].d
            ),
            ("bash", true, Some(12_000))
        );
        assert_eq!(facts.projects.get("s1"), Some(&"demo".to_string()));
        assert!(!serde_json::to_string(&facts).unwrap().contains("SECRET"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_db_is_an_error_not_a_panic() {
        assert!(parse_db(Path::new("/nonexistent/opencode.db")).is_err());
    }
}
