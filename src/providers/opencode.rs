use crate::model::{estimate_cost, UsageEvent};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// opencode stores one JSON file per message under
/// ~/.local/share/opencode/storage/message/<session>/<msg>.json
/// Assistant messages carry tokens{input,output,reasoning,cache} + cost + model info.
pub struct OpencodeProvider;

impl OpencodeProvider {
    pub fn load(home: &Path) -> Vec<UsageEvent> {
        // opencode uses XDG data home; default to ~/.local/share
        let data = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".local/share"));
        let root = data.join("opencode").join("storage").join("message");
        let mut out = Vec::new();
        let Ok(sessions) = fs::read_dir(&root) else {
            return out;
        };
        for sdir in sessions.flatten() {
            let spath = sdir.path();
            if !spath.is_dir() {
                continue;
            }
            let session = spath
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string());
            let Ok(files) = fs::read_dir(&spath) else {
                continue;
            };
            for f in files.flatten() {
                let fp = f.path();
                if fp.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(raw) = fs::read_to_string(&fp) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(&raw) else {
                    continue;
                };
                let Some(tokens) = v.get("tokens").and_then(|t| t.as_object()) else {
                    continue;
                };
                let model = v
                    .get("modelID")
                    .and_then(|m| m.as_str())
                    .or_else(|| v.get("model").and_then(|m| m.as_str()))
                    .unwrap_or("unknown")
                    .to_string();
                let ts = v
                    .get("time")
                    .and_then(|t| t.as_f64())
                    .map(|f| f as i64)
                    .unwrap_or(0);
                let get_num = |k: &str| tokens.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                let mut e = UsageEvent {
                    provider: "opencode".into(),
                    model: model.clone(),
                    project: None,
                    session: session.clone(),
                    ts,
                    input_tokens: get_num("input"),
                    output_tokens: get_num("output"),
                    cache_read_tokens: get_num("cache"),
                    cache_write_tokens: 0,
                    cost: None,
                };
                // opencode reports cost directly in some versions
                let reported = v.get("cost").and_then(|c| c.as_f64()).filter(|c| *c > 0.0);
                e.cost = reported.or_else(|| estimate_cost(&e.model, &e));
                out.push(e);
            }
        }
        out
    }
}

use std::path::PathBuf;

pub fn detected(home: &Path) -> bool {
    let data = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".local/share"));
    data.join("opencode")
        .join("storage")
        .join("message")
        .is_dir()
}
