//! Map review anchors to GitLab diff positions, including context and renamed files.
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
#[derive(Clone, Copy)]
struct Line {
    old: Option<u64>,
    new: Option<u64>,
}
fn at(patch: &str, side: &str, wanted: u64) -> Option<Line> {
    let (mut old, mut new) = (0, 0);
    let mut hunk = false;
    for line in patch.lines() {
        if line.starts_with("@@ ") {
            let mut parts = line.split_whitespace().skip(1);
            old = parts
                .next()?
                .trim_start_matches('-')
                .split(',')
                .next()?
                .parse()
                .ok()?;
            new = parts
                .next()?
                .trim_start_matches('+')
                .split(',')
                .next()?
                .parse()
                .ok()?;
            hunk = true;
            continue;
        }
        if !hunk {
            continue;
        }
        let pos = match line.as_bytes().first() {
            Some(b' ') => {
                let p = Line {
                    old: Some(old),
                    new: Some(new),
                };
                old += 1;
                new += 1;
                p
            }
            Some(b'-') => {
                let p = Line {
                    old: Some(old),
                    new: None,
                };
                old += 1;
                p
            }
            Some(b'+') => {
                let p = Line {
                    old: None,
                    new: Some(new),
                };
                new += 1;
                p
            }
            _ => continue,
        };
        if (if side == "old" { pos.old } else { pos.new }) == Some(wanted) {
            return Some(pos);
        }
    }
    None
}
fn range_line(line: Line, path: &str) -> Value {
    let hash = Sha1::digest(path)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut value = json!({"line_code":format!("{hash}_{}_{}",line.old.unwrap_or(0),line.new.unwrap_or(0)),"type":if line.old.is_none(){"new"}else{"old"}});
    if let Some(n) = line.old {
        value["old_line"] = n.into();
    }
    if let Some(n) = line.new {
        value["new_line"] = n.into();
    }
    value
}
pub fn position(anchor: &Value, changed: &Value, refs: &Value, patch: &str) -> Option<Value> {
    let path = anchor["path"].as_str()?;
    let file = changed.as_array()?.iter().find(|f| f["path"] == path)?;
    let mut pos = refs.clone();
    pos["old_path"] = file["oldPath"].as_str().unwrap_or(path).into();
    pos["new_path"] = path.into();
    if anchor["kind"] == "file" {
        pos["position_type"] = "file".into();
        return Some(pos);
    }
    let side = anchor["side"].as_str()?;
    let start = at(patch, side, anchor["startLine"].as_u64()?)?;
    let end = at(patch, side, anchor["endLine"].as_u64()?)?;
    pos["position_type"] = "text".into();
    if let Some(n) = end.old {
        pos["old_line"] = n.into();
    }
    if let Some(n) = end.new {
        pos["new_line"] = n.into();
    }
    if anchor["startLine"] != anchor["endLine"] {
        pos["line_range"] = json!({"start":range_line(start,path),"end":range_line(end,path)});
    }
    Some(pos)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renamed_added_deleted_context_and_range() {
        let changed = json!([{"path":"new.rs","oldPath":"old.rs"}]);
        let refs = json!({"base_sha":"base","head_sha":"head","start_sha":"start"});
        let patch =
            "--- a/old.rs\n+++ b/new.rs\n@@ -1,3 +1,4 @@\n same\n-old\n+new\n+more\n last\n";
        let mut a = json!({"kind":"line","path":"new.rs","side":"new","startLine":2,"endLine":3});
        let pos = position(&a, &changed, &refs, patch).unwrap();
        assert_eq!(pos["old_path"], "old.rs");
        assert_eq!(pos["new_line"], 3);
        assert!(pos.get("old_line").is_none());
        assert_eq!(pos["line_range"]["start"]["type"], "new");
        assert!(pos["line_range"]["end"]["line_code"]
            .as_str()
            .unwrap()
            .ends_with("_0_3"));
        a["startLine"] = 4.into();
        a["endLine"] = 4.into();
        let pos = position(&a, &changed, &refs, patch).unwrap();
        assert_eq!(pos["old_line"], 3);
        assert_eq!(pos["new_line"], 4);
        a["side"] = "old".into();
        a["startLine"] = 2.into();
        a["endLine"] = 2.into();
        let pos = position(&a, &changed, &refs, patch).unwrap();
        assert_eq!(pos["old_line"], 2);
        assert!(pos.get("new_line").is_none());
        a["endLine"] = 500.into();
        assert!(position(&a, &changed, &refs, patch).is_none());
    }
}
