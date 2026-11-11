//! Live status-page components (port of Swift `ProviderStatusFetcher.fetchStatusSummary`):
//! incident.io's grouped proxy feed first, then the classic statuspage.io `components.json`.

use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;

const MAX_BODY: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub id: String,
    pub name: String,
    /// Raw statuspage status (`operational`, `degraded_performance`, ...).
    pub status: String,
    pub children: Vec<Component>,
}

/// Mac `ProviderStatusIndicator` severity: 0 none, 1 maintenance/unknown, 2 minor, 3 major,
/// 4 critical.
pub fn rank(status: &str) -> u8 {
    match status {
        "operational" => 0,
        "degraded_performance" => 2,
        "partial_outage" => 3,
        "major_outage" | "full_outage" => 4,
        _ => 1,
    }
}

/// Mac `label(forStatuspageStatus:)` (en strings).
pub fn label(status: &str) -> &'static str {
    match status {
        "operational" => "Operational",
        "degraded_performance" => "Degraded performance",
        "partial_outage" => "Partial outage",
        "major_outage" | "full_outage" => "Major outage",
        "under_maintenance" => "Maintenance",
        _ => "Status unknown",
    }
}

/// Mac `StatusComponentsMenuView` dot colors.
pub fn dot_rgb(status: &str) -> Option<(f32, f32, f32)> {
    match status {
        "operational" => Some((0.20, 0.78, 0.35)),
        "degraded_performance" | "under_maintenance" => Some((0.96, 0.77, 0.13)),
        "partial_outage" | "major_outage" | "full_outage" => Some((0.91, 0.30, 0.24)),
        _ => None,
    }
}

fn name_of(v: &Value) -> Option<String> {
    let n = v.get("name")?.as_str()?.trim();
    (!n.is_empty()).then(|| n.to_string())
}

fn hidden(v: &Value) -> bool {
    v.get("hidden").and_then(Value::as_bool) == Some(true)
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// incident.io `/proxy/<host>` summary: groups from `structure.items`, statuses from
/// `affected_components` (unlisted = operational); a group takes its worst child's status.
pub fn parse_incident_io(data: &[u8]) -> Option<Vec<Component>> {
    let root: Value = serde_json::from_slice(data).ok()?;
    let summary = root.get("summary")?;
    let items = summary.get("structure")?.get("items")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    let mut status_by_id: HashMap<String, String> = HashMap::new();
    for a in summary
        .get("affected_components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(id), Some(st)) = (str_of(a, "component_id"), str_of(a, "status")) {
            status_by_id.insert(id, st);
        }
    }
    let leaf = |id: String, name: String| Component {
        status: status_by_id
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "operational".into()),
        id,
        name,
        children: Vec::new(),
    };
    let mut out = Vec::new();
    for item in items {
        if let Some(group) = item.get("group").filter(|g| !g.is_null() && !hidden(g)) {
            let children: Vec<Component> = group
                .get("components")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|c| !hidden(c))
                .filter_map(|c| Some(leaf(str_of(c, "component_id")?, name_of(c)?)))
                .collect();
            let (Some(id), Some(name)) = (str_of(group, "id"), name_of(group)) else {
                continue;
            };
            let status = children
                .iter()
                .max_by_key(|c| rank(&c.status))
                .map(|c| c.status.clone())
                .unwrap_or_else(|| "operational".into());
            out.push(Component {
                id,
                name,
                status,
                children,
            });
        } else if let Some(c) = item.get("component").filter(|c| !c.is_null() && !hidden(c)) {
            if let (Some(id), Some(name)) = (str_of(c, "component_id"), name_of(c)) {
                out.push(leaf(id, name));
            }
        }
    }
    Some(out)
}

/// statuspage.io `api/v2/components.json`: groups carry their children (by `group_id`),
/// ungrouped leaves stay top-level, all in `position` order.
pub fn parse_statuspage(data: &[u8]) -> Option<Vec<Component>> {
    let root: Value = serde_json::from_slice(data).ok()?;
    let mut raw: Vec<&Value> = root
        .get("components")?
        .as_array()?
        .iter()
        .filter(|c| name_of(c).is_some())
        .collect();
    raw.sort_by_key(|c| c.get("position").and_then(Value::as_i64).unwrap_or(0));
    let row = |c: &Value, children: Vec<Component>| Component {
        id: str_of(c, "id").unwrap_or_default(),
        name: name_of(c).unwrap_or_default(),
        status: str_of(c, "status").unwrap_or_default(),
        children,
    };
    let is_group = |c: &Value| c.get("group").and_then(Value::as_bool) == Some(true);
    let group_id = |c: &Value| str_of(c, "group_id");
    let mut by_group: HashMap<String, Vec<Component>> = HashMap::new();
    for c in raw.iter().filter(|c| !is_group(c)) {
        if let Some(g) = group_id(c) {
            by_group.entry(g).or_default().push(row(c, Vec::new()));
        }
    }
    Some(
        raw.iter()
            .filter_map(|c| {
                if is_group(c) {
                    let kids = by_group
                        .remove(&str_of(c, "id").unwrap_or_default())
                        .unwrap_or_default();
                    Some(row(c, kids))
                } else if group_id(c).is_some() {
                    None
                } else {
                    Some(row(c, Vec::new()))
                }
            })
            .collect(),
    )
}

fn get(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    let resp = agent.get(url).call().ok()?;
    let mut body = Vec::new();
    resp.into_reader()
        .take(MAX_BODY)
        .read_to_end(&mut body)
        .ok()?;
    Some(body)
}

/// Blocking fetch (call from `spawn_blocking`). `base` is the status page URL.
pub fn fetch(base: &str) -> Result<Vec<Component>, String> {
    let base = base.trim_end_matches('/');
    let host = base
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "Invalid status page URL".to_string())?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    if let Some(list) = get(&agent, &format!("https://{host}/proxy/{host}"))
        .and_then(|b| parse_incident_io(&b))
    {
        return Ok(list);
    }
    get(&agent, &format!("{base}/api/v2/components.json"))
        .and_then(|b| parse_statuspage(&b))
        .ok_or_else(|| "Status unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incident_io_groups_take_worst_child() {
        let json = br#"{"summary":{
            "affected_components":[{"component_id":"c2","status":"partial_outage"}],
            "structure":{"items":[
                {"group":{"id":"g1","name":"Codex","components":[
                    {"component_id":"c1","name":"CLI"},
                    {"component_id":"c2","name":" Cloud tasks "},
                    {"component_id":"c3","name":"Secret","hidden":true}]}},
                {"component":{"component_id":"c4","name":"Login"}},
                {"group":{"id":"g2","name":"Hidden","hidden":true,"components":[]}}
            ]}}}"#;
        let list = parse_incident_io(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Codex");
        assert_eq!(list[0].status, "partial_outage");
        assert_eq!(list[0].children.len(), 2);
        assert_eq!(list[0].children[1].name, "Cloud tasks");
        assert_eq!(list[1].name, "Login");
        assert_eq!(list[1].status, "operational");
        assert!(parse_incident_io(br#"{"summary":{"structure":{"items":[]}}}"#).is_none());
        assert!(parse_incident_io(b"<html>").is_none());
    }

    #[test]
    fn statuspage_nests_children_in_position_order() {
        let json = br#"{"components":[
            {"id":"b","name":"API","status":"degraded_performance","position":2,"group_id":"g"},
            {"id":"g","name":"Claude","status":"degraded_performance","position":1,"group":true},
            {"id":"a","name":"claude.ai","status":"operational","position":1,"group_id":"g"},
            {"id":"x","name":"Console","status":"major_outage","position":3},
            {"id":"y","name":"  ","status":"operational","position":4}
        ]}"#;
        let list = parse_statuspage(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Claude");
        let kids: Vec<&str> = list[0].children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(kids, ["claude.ai", "API"]);
        assert_eq!(list[1].name, "Console");
        assert_eq!(label(&list[1].status), "Major outage");
    }

    /// Network: `cargo test live_status_feeds -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_status_feeds() {
        for url in ["https://status.openai.com/", "https://status.claude.com/"] {
            let list = fetch(url).unwrap();
            assert!(!list.is_empty(), "{url}");
            for c in &list {
                println!("{url} {} [{}] {}", c.name, c.status, c.children.len());
            }
        }
    }

    #[test]
    fn labels_and_ranks_match_mac() {
        assert_eq!(label("operational"), "Operational");
        assert_eq!(label("weird"), "Status unknown");
        assert!(rank("major_outage") > rank("partial_outage"));
        assert!(rank("partial_outage") > rank("degraded_performance"));
        assert!(dot_rgb("weird").is_none());
    }
}
