use crate::model::UsageEvent;
use std::process::Command;

/// Ollama local inference. Two signals, honestly reported:
///
/// 1. `/api/tags` on localhost:11434 (LIMTOP_OLLAMA_URL to override) —
///    used for detection + installed-model list.
/// 2. journald `ollama` unit logs — GIN access lines carry timestamps of
///    POST /api/chat + /api/generate calls. Token counts are NOT logged at
///    default level, so events carry zero tokens; the dashboard counts
///    calls and shows $0 (local inference is free after the hardware).
///
/// If journald is unavailable (macOS, containers), the provider still
/// detects via API and reports what it can.
pub struct OllamaProvider;

const DEFAULT_URL: &str = "http://localhost:11434";

impl OllamaProvider {
    pub fn load(configured_url: Option<&str>) -> Vec<UsageEvent> {
        // Try API detection first: quick, cross-platform.
        let api_up = ureq::get(&format!("{}/api/tags", url(configured_url)))
            .timeout(std::time::Duration::from_secs(2))
            .call()
            .map(|r| r.status() < 300)
            .unwrap_or(false);
        if !api_up {
            return Vec::new();
        }
        journal_events()
    }
}

/// Effective ollama URL: env `LIMTOP_OLLAMA_URL` beats config file beats
/// default localhost:11434.
fn url(configured: Option<&str>) -> String {
    resolved(configured)
}

/// Public wrapper for status strings elsewhere (e.g. registry fallback).
pub fn base_url(configured: Option<&str>) -> String {
    resolved(configured)
}

fn resolved(configured: Option<&str>) -> String {
    std::env::var("LIMTOP_OLLAMA_URL")
        .ok()
        .or_else(|| configured.map(|s| s.to_string()))
        .unwrap_or_else(|| DEFAULT_URL.into())
}

/// Parse GIN access lines from `journalctl -u ollama`.
fn journal_events() -> Vec<UsageEvent> {
    let out = Command::new("journalctl")
        .args(["-u", "ollama", "--no-pager", "-o", "json", "-n", "50000"])
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut events = Vec::new();
    for line in stdout.lines() {
        // each line is a JSON object; we only need MESSAGE + timestamp
        let Some((msg_start, ts)) = extract(line) else {
            continue;
        };
        if !is_inference_call(msg_start) {
            continue;
        }
        events.push(UsageEvent {
            provider: "ollama".into(),
            model: "local".into(),
            project: None,
            session: None,
            ts,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: Some(0.0),
        });
    }
    events
}

/// Pull "MESSAGE":"..." value (raw, escapes intact) and __REALTIME_TIMESTAMP.
/// Hand-rolled to avoid a full JSON parse per line (journalctl -o json lines
/// can be megabytes with large embedded contexts).
fn extract(line: &str) -> Option<(&str, i64)> {
    let msg_key = line.find("\"MESSAGE\":")?;
    let start = msg_key + "\"MESSAGE\":".len();
    let rest = &line[start..];
    let rest = rest.trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let rest = &rest[1..];
    // find unescaped closing quote
    let mut end = None;
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped char
            continue;
        }
        if bytes[i] == b'"' {
            end = Some(i);
            break;
        }
        i += 1;
    }
    let end = end?;
    let msg = &rest[..end];
    let ts_key = line.find("__REALTIME_TIMESTAMP")?;
    let ts_str = line[ts_key..].split('"').nth(2)?;
    let ts = ts_str.parse::<i64>().ok()? / 1_000_000;
    Some((msg, ts))
}

fn is_inference_call(msg: &str) -> bool {
    (msg.contains("POST") && (msg.contains("/api/chat") || msg.contains("/api/generate")))
        && !msg.contains(" 4") // crude status filter beyond 200/3xx
}

pub fn status(configured_url: Option<&str>) -> Option<(bool, String)> {
    // live API check (single url resolution for both use and display)
    let base = url(configured_url);
    match ureq::get(&format!("{}/api/tags", base))
        .timeout(std::time::Duration::from_secs(2))
        .call()
    {
        Ok(resp) if resp.status() < 300 => {
            let n = resp
                .into_string()
                .ok()
                .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
                .and_then(|v| v.get("models").and_then(|m| m.as_array()).map(|a| a.len()))
                .unwrap_or(0);
            Some((
                true,
                format!("ollama @ {} ({} models)", url(configured_url), n),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_message_and_ts() {
        let line = r#"{"MESSAGE":"[GIN] 2026/06/10 - 11:25:58 | 200 | 7.3s | POST \"/api/generate\"","__REALTIME_TIMESTAMP":"1781115958998510"}"#;
        let (msg, ts) = extract(line).unwrap();
        assert!(msg.contains("/api/generate"));
        assert_eq!(ts, 1_781_115_958);
    }

    #[test]
    fn skips_non_inference() {
        assert!(!is_inference_call("[GIN] | 200 | GET \"/api/tags\""));
        assert!(!is_inference_call("[GIN] | 200 | POST \"/api/show\""));
        assert!(is_inference_call("[GIN] | 200 | POST \"/api/chat\""));
        assert!(is_inference_call("[GIN] | 200 | POST \"/api/generate\""));
    }

    #[test]
    fn handles_escaped_quotes() {
        let line = r#"{"MESSAGE":"POST \"/api/chat\" ok \"still\"","__REALTIME_TIMESTAMP":"1000000000000"}"#;
        let (msg, ts) = extract(line).unwrap();
        assert!(msg.contains("/api/chat"));
        assert_eq!(ts, 1_000_000);
    }
}
