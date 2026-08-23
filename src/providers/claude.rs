use crate::model::{estimate_cost, UsageEvent};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Claude Code stores per-project session transcripts as JSONL under
/// ~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl
/// Assistant entries carry message.usage{input_tokens, output_tokens,
/// cache_read_input_tokens, cache_creation_input_tokens} + model + timestamp.
pub struct ClaudeProvider;

impl ClaudeProvider {
    /// Load every usage event across all projects: ~/.claude/projects plus
    /// any extra roots from config (each scanned for its projects/ subdir).
    pub fn load(home: &Path, extra_roots: &[String]) -> Vec<UsageEvent> {
        let mut out = Vec::new();
        let root = home.join(".claude").join("projects");
        walk_projects(&root, &mut out);
        for extra in extra_roots {
            let p = std::path::PathBuf::from(extra).join("projects");
            walk_projects(&p, &mut out);
        }
        out
    }
}

/// Walk a projects/ root dir: one subdir per project, one .jsonl per session.
fn walk_projects(root: &Path, out: &mut Vec<UsageEvent>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for proj_dir in entries.flatten() {
        let path = proj_dir.path();
        if !path.is_dir() {
            continue;
        }
        // Decode project dir name ("-home-rdc-foo" → "~/foo")
        let project = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(decode_project_dir)
            .unwrap_or_default();
        let Ok(files) = fs::read_dir(&path) else {
            continue;
        };
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            out.extend(parse_jsonl(&fp, &project));
        }
    }
}

/// "-home-rdc-claude-lantrn" → "~/claude/lantrn"
fn decode_project_dir(name: &str) -> String {
    let decoded = name.replace('-', "/");
    let trimmed = decoded.trim_start_matches('/');
    // Claude encodes absolute path; collapse $HOME to ~
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy().trim_end_matches('/').to_string();
        let prefix = format!("{}/", home_str);
        if let Some(rest) = trimmed.strip_prefix(&prefix.trim_start_matches('/')) {
            return format!("~/{}", rest);
        }
    }
    format!("/{}", trimmed)
}

fn parse_jsonl(path: &Path, project: &str) -> Vec<UsageEvent> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let session = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());
    let mut out = Vec::new();
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // Only assistant messages carry usage
        let Some(msg) = v.get("message").and_then(|m| m.as_object()) else {
            continue;
        };
        let Some(usage) = msg.get("usage").and_then(|u| u.as_object()) else {
            continue;
        };
        let model = msg
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();
        let ts = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(parse_iso_ts)
            .unwrap_or(0);
        let get_num = |k: &str| {
            usage
                .get(k)
                .and_then(|x| x.as_u64())
                .or_else(|| {
                    usage
                        .get(k)
                        .and_then(|x| x.as_i64().map(|i| i.max(0) as u64))
                })
                .unwrap_or(0)
        };
        let event = UsageEvent {
            provider: "claude-code".into(),
            model,
            project: Some(project.to_string()),
            session: session.clone(),
            ts,
            input_tokens: get_num("input_tokens"),
            output_tokens: get_num("output_tokens"),
            cache_read_tokens: get_num("cache_read_input_tokens"),
            cache_write_tokens: get_num("cache_creation_input_tokens"),
            cost: None,
        };
        let cost = estimate_cost(&event.model, &event);
        out.push(UsageEvent { cost, ..event });
    }
    out
}

/// "2025-10-15T17:14:43.868Z" → epoch seconds (UTC, no chrono needed for
/// this exact shape).
fn parse_iso_ts(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let min: i64 = s.get(14..16)?.parse().ok()?;
    let sec: i64 = s.get(17..19)?.parse().ok()?;
    // days from civil (Howard Hinnant's algorithm)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + min * 60 + sec)
}

/// True if ~/.claude/projects exists.
pub fn detected(home: &Path) -> bool {
    home.join(".claude").join("projects").is_dir()
}

#[allow(dead_code)]
pub fn session_dir(home: &Path) -> Option<PathBuf> {
    let p = home.join(".claude").join("projects");
    p.is_dir().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test: a total-outage bug (2026-08-23) put the
    /// `out.extend(parse_jsonl(...))` call after `continue`, making it
    /// unreachable — the provider silently emitted zero events while all
    /// 31 tests stayed green. This end-to-end fixture pins the walk:
    /// fake home → projects/<encoded-project>/session.jsonl → events.
    #[test]
    fn walk_projects_emits_events() {
        let tmp = std::env::temp_dir().join(format!("limtop-claude-test-{}", std::process::id()));
        let proj = tmp.join(".claude").join("projects").join("-home-x-foo");
        std::fs::create_dir_all(&proj).unwrap();
        let session = proj.join("s1.jsonl");
        std::fs::write(
            &session,
            concat!(
                r#"{"type":"assistant","timestamp":"2026-08-23T00:00:00Z","message":{"model":"claude-sonnet-4-5","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":1,"cache_creation_input_tokens":2}}}"#,
                "\n"
            ),
        )
        .unwrap();

        let events = ClaudeProvider::load(&tmp, &[]);
        assert_eq!(events.len(), 1, "must emit the fixture event");
        assert_eq!(events[0].input_tokens, 10);
        assert_eq!(events[0].output_tokens, 20);
        assert_eq!(events[0].model, "claude-sonnet-4-5");
        assert!(events[0].project.as_deref().unwrap_or("").ends_with("foo"));

        // extra root: same layout under a configured dir
        let extra =
            std::env::temp_dir().join(format!("limtop-claude-extra-{}", std::process::id()));
        let eproj = extra.join("projects").join("-home-y-bar");
        std::fs::create_dir_all(&eproj).unwrap();
        std::fs::write(
            eproj.join("s2.jsonl"),
            r#"{"type":"assistant","timestamp":"2026-08-23T01:00:00Z","message":{"model":"claude-opus-4-6","usage":{"input_tokens":5,"output_tokens":6}}}"#,
        )
        .unwrap();
        let roots = vec![extra.to_string_lossy().to_string()];
        let all = ClaudeProvider::load(&tmp, &roots);
        assert_eq!(all.len(), 2, "home + extra root both counted");
        assert!(all.iter().any(|e| e.model == "claude-opus-4-6"));

        std::fs::remove_dir_all(&tmp).ok();
        std::fs::remove_dir_all(&extra).ok();
    }
}
