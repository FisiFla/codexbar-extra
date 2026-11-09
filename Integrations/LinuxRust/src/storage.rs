//! Local provider data footprint (port of `ProviderStorageScanner` / `ProviderStoragePathCatalog`).
//!
//! Sizes the Linux data dirs of each provider (`~/.codex`, `~/.claude`, ...) by summing regular
//! file sizes. Symlinks are never followed. Components are the top-level entries of each root,
//! sorted largest first. The scan is blocking; callers run it on a blocking thread and cache it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Automatic rescan interval (mac `automaticStorageRefreshInterval` is similarly coarse).
pub const REFRESH_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub path: String,
    pub bytes: u64,
}

impl Component {
    pub fn name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| self.path.clone())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Footprint {
    pub total_bytes: u64,
    pub paths: Vec<String>,
    pub missing: Vec<String>,
    pub unreadable: usize,
    pub components: Vec<Component>,
}

impl Footprint {
    pub fn has_local_data(&self) -> bool {
        self.total_bytes > 0
    }
}

/// Linux candidate dirs per provider (mac catalog minus the macOS-only Library paths).
pub fn candidate_paths(provider: &str, home: &Path, codex_home: Option<&str>) -> Vec<PathBuf> {
    let h = |p: &str| home.join(p);
    let mut v = match provider {
        "codex" => vec![codex_home
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| h(".codex"))],
        "claude" => vec![h(".claude"), h(".config/claude")],
        "gemini" => vec![h(".gemini"), h(".config/gemini")],
        "opencode" | "opencodego" => vec![h(".config/opencode"), h(".local/share/opencode")],
        "copilot" => vec![h(".config/github-copilot")],
        "cursor" => vec![h(".cursor"), h(".config/Cursor"), h(".cache/Cursor")],
        _ => vec![],
    };
    let mut seen = std::collections::BTreeSet::new();
    v.retain(|p| seen.insert(p.clone()));
    v
}

pub const PROVIDERS: [&str; 7] = [
    "codex",
    "claude",
    "gemini",
    "opencode",
    "opencodego",
    "copilot",
    "cursor",
];

/// Sum regular-file bytes under each existing candidate path.
pub fn scan(candidates: &[PathBuf]) -> Footprint {
    let mut fp = Footprint::default();
    let mut components: Vec<Component> = Vec::new();
    for root in candidates {
        let Ok(meta) = std::fs::symlink_metadata(root) else {
            fp.missing.push(root.display().to_string());
            continue;
        };
        fp.paths.push(root.display().to_string());
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            fp.total_bytes += meta.len();
            if meta.len() > 0 {
                components.push(Component {
                    path: root.display().to_string(),
                    bytes: meta.len(),
                });
            }
            continue;
        }
        let mut by_top: BTreeMap<String, u64> = BTreeMap::new();
        let mut stack = vec![(root.clone(), None::<String>)];
        while let Some((dir, top)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                fp.unreadable += 1;
                continue;
            };
            for item in rd {
                let Ok(item) = item else {
                    fp.unreadable += 1;
                    continue;
                };
                let Ok(ft) = item.file_type() else {
                    fp.unreadable += 1;
                    continue;
                };
                if ft.is_symlink() {
                    continue;
                }
                let name = top
                    .clone()
                    .unwrap_or_else(|| item.file_name().to_string_lossy().into_owned());
                if ft.is_dir() {
                    stack.push((item.path(), Some(name)));
                } else if ft.is_file() {
                    match item.metadata() {
                        Ok(m) => {
                            fp.total_bytes += m.len();
                            if m.len() > 0 {
                                *by_top.entry(name).or_default() += m.len();
                            }
                        }
                        Err(_) => fp.unreadable += 1,
                    }
                }
            }
        }
        components.extend(by_top.into_iter().map(|(name, bytes)| Component {
            path: root.join(name).display().to_string(),
            bytes,
        }));
    }
    components.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    fp.components = components;
    fp
}

/// Scan every known provider; providers with no local data are omitted.
pub fn scan_all() -> BTreeMap<String, Footprint> {
    let Some(home) = dirs::home_dir() else {
        return BTreeMap::new();
    };
    let codex_home = std::env::var("CODEX_HOME").ok();
    PROVIDERS
        .iter()
        .filter_map(|p| {
            let fp = scan(&candidate_paths(p, &home, codex_home.as_deref()));
            fp.has_local_data().then(|| (p.to_string(), fp))
        })
        .collect()
}

/// Mac `byteCountString`-style: 1.5 MB, 320 KB, 12 bytes (decimal units, like `ByteCountFormatter`).
pub fn bytes_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Mac `StorageBreakdownMenuView` folds the tail into one "Other (N items)" row.
pub const MAX_ROWS: usize = 6;

pub fn rows(fp: &Footprint) -> Vec<(String, u64)> {
    let mut rows: Vec<(String, u64)> = fp
        .components
        .iter()
        .take(MAX_ROWS)
        .map(|c| (c.name(), c.bytes))
        .collect();
    if fp.components.len() > MAX_ROWS {
        let rest = &fp.components[MAX_ROWS..];
        rows.push((
            format!("Other ({} items)", rest.len()),
            rest.iter().map(|c| c.bytes).sum(),
        ));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cbx-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scan_sums_files_groups_by_top_level_and_skips_symlinks() {
        let t = tree("scan");
        let root = t.join(".claude");
        fs::create_dir_all(root.join("projects/a/deep")).unwrap();
        fs::create_dir_all(root.join("plans")).unwrap();
        fs::write(root.join("projects/a/deep/x.jsonl"), vec![0u8; 3000]).unwrap();
        fs::write(root.join("projects/y.jsonl"), vec![0u8; 1000]).unwrap();
        fs::write(root.join("plans/p.md"), vec![0u8; 500]).unwrap();
        fs::write(root.join("settings.json"), vec![0u8; 50]).unwrap();
        fs::write(root.join("empty"), b"").unwrap();
        // A symlink to a big outside dir must not be followed or counted.
        let outside = t.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("big"), vec![0u8; 99_999]).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let missing = t.join(".config/claude");
        let fp = scan(&[root.clone(), missing.clone()]);
        assert_eq!(fp.total_bytes, 4550);
        assert_eq!(fp.paths, vec![root.display().to_string()]);
        assert_eq!(fp.missing, vec![missing.display().to_string()]);
        let comps: Vec<_> = fp.components.iter().map(|c| (c.name(), c.bytes)).collect();
        assert_eq!(
            comps,
            vec![
                ("projects".to_string(), 4000),
                ("plans".to_string(), 500),
                ("settings.json".to_string(), 50)
            ]
        );
        assert_eq!(fp.unreadable, 0);
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn scan_counts_unreadable_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let t = tree("unread");
        let root = t.join(".codex");
        fs::create_dir_all(root.join("locked")).unwrap();
        fs::write(root.join("a"), vec![0u8; 10]).unwrap();
        fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let fp = scan(std::slice::from_ref(&root));
        // Running as root can read anyway; otherwise the dir is reported.
        assert!(fp.unreadable <= 1);
        assert_eq!(fp.total_bytes, 10);
        fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn candidates_honor_codex_home_and_dedupe() {
        let home = Path::new("/home/u");
        assert_eq!(
            candidate_paths("codex", home, None),
            vec![PathBuf::from("/home/u/.codex")]
        );
        assert_eq!(
            candidate_paths("codex", home, Some("/data/cx")),
            vec![PathBuf::from("/data/cx")]
        );
        assert!(candidate_paths("deepseek", home, None).is_empty());
    }

    #[test]
    fn rows_fold_the_tail_into_other() {
        let fp = Footprint {
            total_bytes: 0,
            components: (0..9)
                .map(|i| Component {
                    path: format!("/r/c{i}"),
                    bytes: 100 - i,
                })
                .collect(),
            ..Default::default()
        };
        let r = rows(&fp);
        assert_eq!(r.len(), MAX_ROWS + 1);
        assert_eq!(r[0], ("c0".to_string(), 100));
        assert_eq!(r[MAX_ROWS], ("Other (3 items)".to_string(), 94 + 93 + 92));
    }

    #[test]
    fn byte_labels() {
        assert_eq!(bytes_label(12), "12 bytes");
        assert_eq!(bytes_label(1500), "1.5 KB");
        assert_eq!(bytes_label(320_000), "320 KB");
        assert_eq!(bytes_label(2_400_000_000), "2.4 GB");
    }
}
