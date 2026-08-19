use crate::model::{Span, UsageEvent, UsageTotals};
use std::collections::BTreeMap;

/// Everything the TUI needs to render one frame.
#[derive(Debug, Default)]
pub struct Dashboard {
    pub events: Vec<UsageEvent>,
    /// Per-provider totals over the selected span.
    pub by_provider: Vec<(String, UsageTotals)>,
    /// Per-model totals over the selected span.
    pub by_model: Vec<(String, UsageTotals)>,
    /// Per-project totals over the selected span.
    pub by_project: Vec<(String, UsageTotals)>,
    /// Buckets for the burn graph (label, cost) across the span.
    pub burn: Vec<(String, f64)>,
    /// Totals over the selected span.
    pub totals: UsageTotals,
    pub span: Span,
}

impl Dashboard {
    pub fn build(all: Vec<UsageEvent>, span: Span, now: i64) -> Self {
        let cutoff = span.seconds().map(|s| now - s).unwrap_or(i64::MIN);
        let events: Vec<UsageEvent> = all.into_iter().filter(|e| e.ts >= cutoff).collect();

        let mut d = Dashboard {
            totals: Default::default(),
            by_provider: Vec::new(),
            by_model: Vec::new(),
            by_project: Vec::new(),
            burn: Vec::new(),
            span,
            events: events.clone(),
        };

        // ── Groupings ─────────────────────────────────────────────
        let mut prov: BTreeMap<String, UsageTotals> = BTreeMap::new();
        let mut model: BTreeMap<String, UsageTotals> = BTreeMap::new();
        let mut proj: BTreeMap<String, UsageTotals> = BTreeMap::new();
        for e in &events {
            d.totals.add(e);
            prov.entry(e.provider.clone()).or_default().add(e);
            model.entry(e.model.clone()).or_default().add(e);
            let p = e.project.clone().unwrap_or_else(|| "—".into());
            proj.entry(p).or_default().add(e);
        }
        let mut sort_tot = |m: BTreeMap<String, UsageTotals>| -> Vec<(String, UsageTotals)> {
            let mut v: Vec<_> = m.into_iter().collect();
            v.sort_by(|a, b| b.1.total_tokens().cmp(&a.1.total_tokens()));
            v
        };
        d.by_provider = sort_tot(prov);
        d.by_model = sort_tot(model);
        d.by_project = sort_tot(proj);

        // ── Burn graph buckets ────────────────────────────────────
        // Span→bucket count so the graph always shows the whole span.
        let (buckets, width): (usize, i64) = match span {
            Span::Hour => (30, 120),     // 2-min buckets
            Span::Day => (48, 1800),     // 30-min buckets
            Span::Week => (56, 10_800),  // 3-hour buckets
            Span::Month => (30, 86_400), // daily buckets
            Span::All => (40, 0),        // computed dynamically below
        };
        if width > 0 {
            let mut costs = vec![0.0f64; buckets];
            for e in &events {
                let age = now - e.ts;
                if age < 0 {
                    continue;
                }
                let idx = ((age / width) as usize).min(buckets - 1);
                costs[idx] += e.cost.unwrap_or(0.0);
            }
            // oldest → newest (left → right)
            costs.reverse();
            d.burn = costs
                .into_iter()
                .enumerate()
                .map(|(i, c)| (format!("{}", i), c))
                .collect();
        } else {
            // All-time: bucket by month
            let mut months: BTreeMap<String, f64> = BTreeMap::new();
            for e in &events {
                let key = month_key(e.ts);
                *months.entry(key).or_insert(0.0) += e.cost.unwrap_or(0.0);
            }
            d.burn = months.into_iter().collect();
        }
        d
    }
}

/// epoch → "2026-08" for all-time bucketing.
fn month_key(ts: i64) -> String {
    // civil-from-days (Hinnant), then format
    let z = ts.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}", y, m)
}
