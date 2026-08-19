use crate::model::{estimate_cost, UsageEvent};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Gemini CLI stores chat transcripts as JSONL under
/// `~/.gemini/tmp/<project-slug>/chats/session-*.jsonl`.
/// Assistant turns are `type: "gemini"` lines with a `tokens` object
/// (`input`, `output`, `cached`, ...) and the model id.
pub struct GeminiProvider;

impl GeminiProvider {
    pub fn load(home: &Path) -> Vec<UsageEvent> {
        let mut out = Vec::new();
        let tmp = home.join(".gemini").join("tmp");
        let Ok(dirs) = fs::read_dir(&tmp) else {
            return out;
        };
        for proj in dirs.flatten() {
            let chats = proj.path().join("chats");
            if !chats.is_dir() {
                continue;
            }
            let project = proj.file_name().to_string_lossy().to_string();
            let Ok(files) = fs::read_dir(&chats) else {
                continue;
            };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                parse_chat(&p, &project, &mut out);
            }
        }
        out
    }
}

fn parse_chat(path: &Path, project: &str, out: &mut Vec<UsageEvent>) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // assistant turns only
        if v.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }
        let Some(tokens) = v.get("tokens").and_then(|t| t.as_object()) else {
            continue;
        };
        let get = |k: &str| tokens.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let model = v
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("gemini-unknown")
            .to_string();
        // ISO-8601 timestamp "2026-06-10T18:08:01.521Z"
        let ts = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(parse_iso)
            .unwrap_or(0);
        let cached = get("cached");
        let input = get("input");
        let output = get("output");
        let mut e = UsageEvent {
            provider: "gemini".into(),
            project: Some(project.to_string()),
            session: v
                .get("sessionId")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string()),
            model,
            ts,
            input_tokens: input.saturating_sub(cached),
            output_tokens: output,
            cache_read_tokens: cached,
            cache_write_tokens: 0,
            cost: None,
        };
        e.cost = estimate_cost(&e.model, &e);
        out.push(e);
    }
}

/// "2026-06-10T18:08:01.521Z" → epoch seconds (no chrono, fixed-slice parse)
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
    // days from civil (Howard Hinnant's algorithm)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + mi * 60 + sec)
}

fn detected(home: &Path) -> bool {
    home.join(".gemini").join("tmp").is_dir()
}

pub fn status(home: &Path) -> Option<(bool, String)> {
    let tmp = home.join(".gemini").join("tmp");
    if !tmp.is_dir() {
        return None;
    }
    // count chat files for the detail line
    let mut n = 0usize;
    if let Ok(rd) = fs::read_dir(&tmp) {
        for proj in rd.flatten() {
            if let Ok(chats) = fs::read_dir(proj.path().join("chats")) {
                n += chats.flatten().count();
            }
        }
    }
    Some((true, format!("~/.gemini/tmp ({} chats)", n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse_roundtrip() {
        // 2026-06-10T18:08:01Z = 1780908481
        assert_eq!(parse_iso("2026-06-10T18:08:01.521Z"), Some(1_781_114_881));
        assert_eq!(parse_iso("2026-06-10T18:08:01Z"), Some(1_781_114_881));
        assert_eq!(parse_iso("bogus"), None);
    }

    #[test]
    fn parses_gemini_line() {
        let line = r#"{"id":"x","timestamp":"2026-06-10T18:08:01.521Z","type":"gemini","content":"hi","tokens":{"input":6190,"output":3,"cached":100,"thoughts":27,"tool":0,"total":6220},"model":"gemini-2.5-flash"}"#;
        let mut out = Vec::new();
        let dir = std::env::temp_dir().join("limtop_gemini_test");
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("session-test.jsonl");
        fs::write(&f, line).unwrap();
        parse_chat(&f, "lantrn", &mut out);
        assert_eq!(out.len(), 1);
        let e = &out[0];
        assert_eq!(e.input_tokens, 6090); // 6190 - 100 cached
        assert_eq!(e.cache_read_tokens, 100);
        assert_eq!(e.output_tokens, 3);
        assert_eq!(e.model, "gemini-2.5-flash");
        assert_eq!(e.project.as_deref(), Some("lantrn"));
        assert_eq!(e.ts, 1_781_114_881);
        fs::remove_dir_all(&dir).unwrap();
    }
}
