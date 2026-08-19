use crate::model::{estimate_cost, UsageEvent};
use rusqlite::Connection;
use std::path::Path;

/// Hermes Agent stores sessions + per-model usage in ~/.hermes/state.db.
/// sessions: id, source, model, started_at/ended_at (epoch REAL),
/// input_tokens, output_tokens, ...
pub struct HermesProvider;

impl HermesProvider {
    pub fn load(home: &Path) -> Vec<UsageEvent> {
        let db = home.join(".hermes").join("state.db");
        let Ok(conn) = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        else {
            return Vec::new();
        };
        // Treat each session's tokens as one aggregate event at session end
        // (fall back to start time when session still open).
        let mut stmt = match conn.prepare(
            "SELECT id, model, started_at, COALESCE(ended_at, started_at), \
             COALESCE(input_tokens,0), COALESCE(output_tokens,0) \
             FROM sessions WHERE input_tokens > 0 OR output_tokens > 0",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let model: String = row
                .get::<_, Option<String>>(1)?
                .unwrap_or_else(|| "unknown".into());
            let started: f64 = row.get(2)?;
            let ended: f64 = row.get(3)?;
            let input: i64 = row.get(4)?;
            let output: i64 = row.get(5)?;
            Ok((
                id,
                model,
                started,
                ended,
                input.max(0) as u64,
                output.max(0) as u64,
            ))
        });
        let mut out = Vec::new();
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                let (id, model, _started, ended, input, output) = r;
                let mut e = UsageEvent {
                    provider: "hermes".into(),
                    model: model.clone(),
                    project: None,
                    session: Some(id),
                    ts: ended as i64,
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    cost: None,
                };
                e.cost = estimate_cost(&e.model, &e);
                out.push(e);
            }
        }
        out
    }
}

pub fn detected(home: &Path) -> bool {
    home.join(".hermes").join("state.db").exists()
}
