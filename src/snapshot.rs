use crate::aggregator::Dashboard;
use crate::model;
use crate::providers::Registry;
use crate::rate_window::RateWindow;

/// Machine-readable snapshot of everything limtop knows right now.
/// Emitted by `--dump --json` / `--once --json` for scripting and gating.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: &'static str,
    pub span: &'static str,
    pub generated_at: i64,
    pub totals: model::UsageTotals,
    pub window: Option<RateWindow>,
    pub providers: Vec<crate::model::ProviderStatus>,
    pub by_provider: Vec<(String, model::UsageTotals)>,
    pub by_model: Vec<(String, model::UsageTotals)>,
    pub by_project: Vec<(String, model::UsageTotals)>,
}

impl Snapshot {
    pub fn build(reg: &Registry, dash: &Dashboard, window: Option<RateWindow>, now: i64) -> Self {
        Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            span: dash.span_label(),
            generated_at: now,
            totals: dash.totals.clone(),
            window,
            providers: reg.statuses.clone(),
            by_provider: dash.by_provider.clone(),
            by_model: dash.by_model.clone(),
            by_project: dash.by_project.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Span;

    #[test]
    fn snapshot_serializes() {
        let reg = Registry {
            statuses: vec![],
            events: vec![],
        };
        let dash = Dashboard::build(vec![], Span::Day, 1_800_000_000);
        let snap = Snapshot::build(&reg, &dash, None, 1_800_000_000);

        let v = serde_json::to_value(&snap).unwrap();
        assert!(v.get("version").is_some());
        assert!(v.get("span").is_some());
        assert!(v.get("providers").is_some());
        // no rate window → JSON null, key still present for consumers
        assert!(v.get("window").map(|w| w.is_null()).unwrap_or(false));
        assert_eq!(v.get("span").unwrap(), "24h");
    }

    #[test]
    fn snapshot_with_window_serializes_camelcase() {
        let reg = Registry {
            statuses: vec![crate::model::ProviderStatus {
                name: "claude-code".into(),
                detected: true,
                detail: "~/.claude".into(),
            }],
            events: vec![],
        };
        let ev = crate::model::UsageEvent {
            provider: "claude-code".into(),
            project: Some("limtop".into()),
            session: None,
            model: "claude-sonnet-4-5".into(),
            ts: 1_799_999_000,
            input_tokens: 1_000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: Some(0.01),
        };
        let dash = Dashboard::build(vec![ev], Span::Day, 1_800_000_000);
        let window = RateWindow {
            window_start: 1_799_982_000,
            window_secs: 18_000,
            used: 150_000,
            limit: 1_000_000,
            burn_rate: 30_000.0,
            resets_at: 1_800_000_500,
        };
        let snap = Snapshot::build(&reg, &dash, Some(window), 1_800_000_000);

        let v = serde_json::to_value(&snap).unwrap();

        // root keys: Snapshot itself is camelCase
        assert!(v.get("generatedAt").is_some(), "root key generatedAt");
        assert!(
            v.get("generated_at").is_none(),
            "root must not be snake_case"
        );
        assert!(v.get("byProvider").is_some(), "root key byProvider");
        assert!(v.get("by_provider").is_none());
        assert!(
            v.get("byProvider").map(|a| a.is_array()).unwrap_or(false),
            "byProvider is an array"
        );
        assert_eq!(
            v.get("byProvider").unwrap().as_array().unwrap().len(),
            1,
            "one by-provider entry"
        );

        // nested UsageTotals: camelCase
        let totals = v.get("totals").unwrap();
        assert!(totals.get("inputTokens").is_some(), "totals.inputTokens");
        assert!(totals.get("input_tokens").is_none());
        assert_eq!(totals.get("inputTokens").unwrap().as_u64().unwrap(), 1_000,);

        // window: RateWindow camelCase (already was; keep locked in)
        let w = v.get("window").unwrap();
        assert!(w.get("burnRate").is_some(), "window.burnRate");
        assert!(w.get("burn_rate").is_none());
        assert!(w.get("windowStart").is_some(), "window.windowStart");

        // providers: model::ProviderStatus serialized directly
        let provs = v.get("providers").unwrap().as_array().unwrap();
        assert_eq!(provs.len(), 1);
        assert_eq!(provs[0].get("name").unwrap(), "claude-code");
        assert_eq!(provs[0].get("detected").unwrap(), &serde_json::json!(true));
    }
}
