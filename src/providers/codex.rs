use crate::model::{estimate_cost, UsageEvent};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Codex CLI. Modern versions keep thread metadata in `~/.codex/state_5.sqlite`
/// (`threads` table with `rollout_path`) but the token counts live in the
/// rollout JSONL files themselves (possibly empty on some installs).
/// Older versions wrote rollouts under `~/.codex/sessions/` directly.
pub struct CodexProvider;

impl CodexProvider {
    pub fn load(home: &Path) -> Vec<UsageEvent> {
        let mut out = Vec::new();
        // modern: rollout paths from sqlite
        let db = home.join(".codex").join("state_5.sqlite");
        if db.exists() {
            if let Ok(conn) = Connection::open_with_flags_read_only(&db) {
                let mut stmt = match conn
                    .prepare("SELECT rollout_path FROM threads WHERE rollout_path != ''")
                {
                    Ok(s) => s,
                    Err(_) => {
                        legacy_scan(home, &mut out);
                        return out;
                    }
                };
                let paths: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                for p in paths {
                    parse_rollout(Path::new(&p), &mut out);
                }
                if !out.is_empty() {
                    return out;
                }
            }
        }
        legacy_scan(home, &mut out);
        out
    }
}
fn legacy_scan(home: &Path, out: &mut Vec<UsageEvent>) {
    let sessions = home.join(".codex").join("sessions");
    let Ok(rd) = fs::read_dir(&sessions) else {
        return;
    };
    for entry in rd.flatten() {
        walk_jsonl(&entry.path(), out);
    }
}

fn walk_jsonl(dir: &Path, out: &mut Vec<UsageEvent>) {
    if dir.is_file() {
        parse_rollout(dir, out);
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        walk_jsonl(&e.path(), out);
    }
}

/// Rollout files are JSONL; token events look like:
/// {"timestamp":"...","type":"event_msg","payload":{"type":"token_count",
///  "info":{"total_token_usage":{...},"last_token_usage":{...}}}}
/// plus per-turn entries with model + cwd context lines.
fn parse_rollout(path: &Path, out: &mut Vec<UsageEvent>) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    let mut last_usage: Option<Value> = None;
    let mut last_ts: i64 = 0;
    let mut project = String::new();
    let mut model = String::new();
    let mut session = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // session/project context lines
        if project.is_empty() {
            if let Some(cwd) = v
                .get("payload")
                .and_then(|p| p.get("cwd"))
                .and_then(|c| c.as_str())
            {
                project = cwd.to_string();
            }
        }
        if let Some(s) = v
            .get("payload")
            .and_then(|p| p.get("id"))
            .and_then(|i| i.as_str())
        {
            if !s.is_empty() {
                session = s.to_string();
            }
        }
        if model.is_empty() {
            if let Some(m) = v
                .get("payload")
                .and_then(|p| p.get("turn_context"))
                .and_then(|t| t.get("model"))
                .and_then(|m| m.as_str())
            {
                model = m.to_string();
            }
        }
        // token events
        if let Some(info) = v
            .get("payload")
            .and_then(|p| p.get("info"))
            .and_then(|i| i.get("total_token_usage"))
            .cloned()
        {
            last_usage = Some(info);
            last_ts = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(parse_iso)
                .unwrap_or(last_ts);
        }
    }
    // emit one event per rollout file using final cumulative usage
    if let Some(usage) = last_usage {
        let get_u64 = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let mut e = UsageEvent {
            provider: "codex".into(),
            project: if project.is_empty() {
                None
            } else {
                Some(project)
            },
            session: Some(session),
            model: if model.is_empty() {
                "codex-unknown".into()
            } else {
                model
            },
            ts: last_ts,
            input_tokens: get_u64("input_tokens"),
            output_tokens: get_u64("output_tokens"),
            cache_read_tokens: get_u64("cached_input_tokens"),
            cache_write_tokens: 0,
            cost: None,
        };
        e.cost = estimate_cost(&e.model, &e);
        out.push(e);
    }
}

/// "2026-06-11T17:05:52.123Z" → epoch
fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        std::str::from_utf8(&b[r]).ok()?.parse().ok()
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    timestamps_ok(days, h, mi, sec)
}

fn timestamps_ok(days: i64, h: i64, mi: i64, sec: i64) -> Option<i64> {
    Some(days * 86_400 + h * 3_600 + mi * 60 + sec)
}

pub fn status(home: &Path) -> Option<(bool, String)> {
    let dir = home.join(".codex");
    if !dir.is_dir() {
        return None;
    }
    Some((true, "~/.codex (sqlite + rollouts)".into()))
}

// tiny read-only sqlite shim via rusqlite (kept here to avoid cluttering imports at top)
use rusqlite::Connection;

trait ConnectionExt {
    fn open_with_flags_read_only(db: &Path) -> rusqlite::Result<Connection>;
}

impl ConnectionExt for Connection {
    fn open_with_flags_read_only(db: &Path) -> rusqlite::Result<Connection> {
        Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rollout_token_event() {
        let dir = std::env::temp_dir().join("aitop_codex_test");
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rollout-2026-06-11.jsonl");
        fs::write(
            &f,
            concat!(
                r#"{"timestamp":"2026-06-11T17:05:52.123Z","type":"session_meta","payload":{"id":"s1","cwd":"/home/rdc/proj"}}"#, "\n",
                r#"{"timestamp":"2026-06-11T17:06:10.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":500,"output_tokens":200}}}}"#,
                "\n"
            ),
        )
        .unwrap();
        let mut out = Vec::new();
        parse_rollout(&f, &mut out);
        assert_eq!(out.len(), 1);
        let e = &out[0];
        assert_eq!(e.provider, "codex");
        assert_eq!(e.project.as_deref(), Some("/home/rdc/proj"));
        assert_eq!(e.session.as_deref(), Some("s1"));
        assert_eq!(e.input_tokens, 1000);
        assert_eq!(e.cache_read_tokens, 500);
        assert_eq!(e.output_tokens, 200);
        assert!(e.ts > 1_700_000_000);
        fs::remove_dir_all(&dir).unwrap();
    }
}
