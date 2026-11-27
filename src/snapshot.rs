use crate::aggregator::{cache_stats, CacheStats, Dashboard};
use crate::model;
use crate::money::MoneyView;
use crate::providers::Registry;
use crate::rate_window::RateWindow;

/// JSON row shape for `byProjectCache`: project name flattened in front
/// of the stats, i.e. `{project, hitRate, reads, writes, rewarmDollars}`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheRow {
    pub project: String,
    #[serde(flatten)]
    pub stats: CacheStats,
}

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
    /// Month-to-date money picture; always present (built even with no events).
    pub money: MoneyView,
    /// Per-project cache analytics over the snapshot's span; always
    /// present (empty with no events). Sorted rewarm-dollars desc.
    pub by_project_cache: Vec<CacheRow>,
}

impl Snapshot {
    pub fn build(
        reg: &Registry,
        dash: &Dashboard,
        window: Option<RateWindow>,
        money: &MoneyView,
        now: i64,
    ) -> Self {
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
            money: money.clone(),
            // Same event source as byModel/byProject (dash.events, i.e. the
            // span-filtered set) so the panels always agree on the span.
            by_project_cache: cache_stats(&dash.events)
                .into_iter()
                .map(|(project, stats)| CacheRow { project, stats })
                .collect(),
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
        let money = crate::money::MoneyView::build(&[], 1_800_000_000, &Default::default());
        let snap = Snapshot::build(&reg, &dash, None, &money, 1_800_000_000);

        let v = serde_json::to_value(&snap).unwrap();
        assert!(v.get("version").is_some());
        assert!(v.get("span").is_some());
        assert!(v.get("providers").is_some());
        // no rate window → JSON null, key still present for consumers
        assert!(v.get("window").map(|w| w.is_null()).unwrap_or(false));
        assert_eq!(v.get("span").unwrap(), "24h");
        // money always present, even with zero events
        let m = v.get("money").expect("money key present");
        assert_eq!(m.get("monthSpend").unwrap(), &serde_json::json!(0.0));
        assert_eq!(m.get("daysRemaining").unwrap(), &serde_json::json!(17));
        assert!(m.get("budgets").unwrap().is_array());
        // byProjectCache always present; empty with no events
        let c = v
            .get("byProjectCache")
            .expect("byProjectCache key present even with no events");
        assert!(c.is_array());
        assert_eq!(c.as_array().unwrap().len(), 0);
    }

    #[test]
    fn snapshot_by_project_cache_camelcase_and_span_scoped() {
        // Sonnet: (3.0, 15.0)/Mtok. cache_write 10M > 3×input 3M →
        // rewarm (10−3)M × 3.0/1M × 1.25 = 26.25. Event ts is INSIDE the
        // Day span ending at now=1_800_000_000, so dash.events carries it.
        let ev = crate::model::UsageEvent {
            provider: "claude-code".into(),
            project: Some("~/claude/lantrn".into()),
            session: None,
            model: "claude-sonnet-4-5-20250929".into(),
            ts: 1_799_999_000,
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 10_000_000,
            cost: Some(0.01),
        };
        // This one is OUTSIDE the Day span → must NOT appear in cache stats.
        let mut old = ev.clone();
        old.ts = 1_700_000_000;
        old.project = Some("~/ghost".into());
        old.input_tokens = 1;
        old.cache_write_tokens = 1_000_000;
        let reg = Registry {
            statuses: vec![],
            events: vec![ev.clone(), old.clone()],
        };
        let dash = Dashboard::build(vec![ev, old], Span::Day, 1_800_000_000);
        let money =
            crate::money::MoneyView::build(&dash.events, 1_800_000_000, &Default::default());
        let snap = Snapshot::build(&reg, &dash, None, &money, 1_800_000_000);

        let v = serde_json::to_value(&snap).unwrap();
        let rows = v.get("byProjectCache").expect("byProjectCache present");
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 1, "old event outside span is excluded");
        let row = &rows[0];
        assert_eq!(row.get("project").unwrap(), "~/claude/lantrn");
        assert!(row.get("hitRate").is_some(), "camelCase hitRate");
        assert!(
            row.get("rewarmDollars").is_some(),
            "camelCase rewarmDollars"
        );
        assert_eq!(row.get("reads").unwrap(), &serde_json::json!(0));
        assert_eq!(row.get("writes").unwrap(), &serde_json::json!(10_000_000));
        let rw = row.get("rewarmDollars").unwrap().as_f64().unwrap();
        assert!((rw - 26.25).abs() < 1e-6, "rewarm {rw}");
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
        let money =
            crate::money::MoneyView::build(&dash.events, 1_800_000_000, &Default::default());
        let window = RateWindow {
            window_start: 1_799_982_000,
            window_secs: 18_000,
            used: 150_000,
            limit: 1_000_000,
            plan_name: "pro",
            burn_rate: 30_000.0,
            resets_at: 1_800_000_500,
        };
        let snap = Snapshot::build(&reg, &dash, Some(window), &money, 1_800_000_000);

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
