use crate::aggregator::Dashboard;
use crate::model;
use crate::providers::Registry;
use crate::rate_window::RateWindow;

/// Machine-readable snapshot of everything limtop knows right now.
/// Emitted by `--dump --json` / `--once --json` for scripting and gating.
#[derive(serde::Serialize)]
pub struct Snapshot {
    pub version: &'static str,
    pub span: String,
    pub generated_at: i64,
    pub totals: model::UsageTotals,
    pub window: Option<RateWindow>,
    pub providers: Vec<ProviderStatusJson>,
    pub by_provider: Vec<(String, model::UsageTotals)>,
    pub by_model: Vec<(String, model::UsageTotals)>,
    pub by_project: Vec<(String, model::UsageTotals)>,
}

#[derive(serde::Serialize)]
pub struct ProviderStatusJson {
    pub name: String,
    pub detected: bool,
    pub detail: String,
}

impl Snapshot {
    pub fn build(reg: &Registry, dash: &Dashboard, window: Option<RateWindow>, now: i64) -> Self {
        let providers = reg
            .statuses
            .iter()
            .map(|s| ProviderStatusJson {
                name: s.name.clone(),
                detected: s.detected,
                detail: s.detail.clone(),
            })
            .collect();
        Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            span: dash.span_label().to_string(),
            generated_at: now,
            totals: dash.totals.clone(),
            window,
            providers,
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
}
