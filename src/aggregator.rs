use crate::model::{Span, UsageEvent, UsageTotals};

/// Wall-clock helper for drilldown hourly bucketing.
pub fn now_hint() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
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
    pub fn span_label(&self) -> &'static str {
        match self.span {
            crate::model::Span::Hour => "1h",
            crate::model::Span::Day => "24h",
            crate::model::Span::Week => "7d",
            crate::model::Span::Month => "30d",
            crate::model::Span::All => "all",
        }
    }
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
            model.entry(canonical_model(&e.model)).or_default().add(e);
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

/// Canonical model identity for by-model grouping.
///
/// Lowercases, strips ONE vendor prefix (zai-org-/anthropic-/google-/openai-)
/// and ONE dated suffix (-20250929/-20260301), and dots dash-style version
/// segments. Unknown model strings pass through unchanged, so raw names like
/// "e2ee-deepseek-v4-flash" or already-dotted "gpt-5.2-turbo" are safe.
pub fn canonical_model(raw: &str) -> String {
    let mut s = raw.to_lowercase();
    for p in ["zai-org-", "anthropic-", "google-", "openai-"] {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.to_string();
            break; // first match wins, strip only once
        }
    }
    for suf in ["-20250929", "-20260301"] {
        if let Some(rest) = s.strip_suffix(suf) {
            s = rest.to_string();
            break;
        }
    }
    for (from, to) in [
        ("-5-2", "-5.2"),
        ("-5-1", "-5.1"),
        ("-4-7", "-4.7"),
        ("-4-5", "-4.5"),
        ("-4-6", "-4.6"),
    ] {
        s = s.replace(from, to);
    }
    s
}

/// epoch → (year, month, day-of-month) via civil-from-days (Hinnant).
/// Single source of truth for calendar math: month_key and
/// days_in_month_and_day both derive from this.
pub(crate) fn civil_ymd(ts: i64) -> (i64, u32, u32) {
    let z = ts.div_euclid(86_400) + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let d = doy - (153 * mp + 2) / 5 + 1;
    (y, m as u32, d as u32)
}

/// epoch → "2026-08" for all-time bucketing.
pub(crate) fn month_key(ts: i64) -> String {
    let (y, m, _) = civil_ymd(ts);
    format!("{:04}-{:02}", y, m)
}

/// epoch → (days in that month, day of month). Feeds month-projection
/// math: days_remaining = days_in_month - day_of_month + 1 (today counts,
/// so spend can still accrue before midnight).
pub(crate) fn days_in_month_and_day(ts: i64) -> (u32, u32) {
    let (y, m, d) = civil_ymd(ts);
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => unreachable!("civil_ymd month is always 1..=12"),
    };
    (dim, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ts: i64, model: &str, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            provider: "test".into(),
            model: model.into(),
            project: None,
            session: None,
            ts,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: None,
        }
    }

    #[test]
    fn canonical_claude_dated() {
        assert_eq!(
            canonical_model("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4.5"
        );
    }

    #[test]
    fn canonical_zai_prefix() {
        assert_eq!(canonical_model("zai-org-glm-5-2"), "glm-5.2");
    }

    #[test]
    fn canonical_unchanged_dotted() {
        assert_eq!(canonical_model("glm-4.7"), "glm-4.7");
    }

    #[test]
    fn canonical_unchanged_turbo_suffix() {
        assert_eq!(canonical_model("gpt-5.2-turbo"), "gpt-5.2-turbo");
    }

    #[test]
    fn canonical_claude_opus() {
        assert_eq!(canonical_model("claude-opus-4-6"), "claude-opus-4.6");
    }

    #[test]
    fn canonical_v4_flash_untouched() {
        assert_eq!(
            canonical_model("e2ee-deepseek-v4-flash"),
            "e2ee-deepseek-v4-flash"
        );
    }

    #[test]
    fn build_groups_by_canonical_model() {
        let events = vec![
            ev(1_000_000, "zai-org-glm-5-2", 100, 50),
            ev(1_000_001, "glm-5.2", 200, 80),
        ];
        let d = Dashboard::build(events, Span::All, 2_000_000);
        assert_eq!(d.by_model.len(), 1, "expected one merged entry");
        assert_eq!(d.by_model[0].0, "glm-5.2");
        assert_eq!(d.by_model[0].1.input_tokens, 300);
        assert_eq!(d.by_model[0].1.output_tokens, 130);
    }
}
