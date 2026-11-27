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
        let sort_tot = |m: BTreeMap<String, UsageTotals>| -> Vec<(String, UsageTotals)> {
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

/// Per-project cache analytics over a span's events.
///
/// Exact: `hit_rate`, `reads`, `writes` are token-count facts.
/// Heuristic: `rewarm_dollars` ESTIMATES dollars spent re-creating cached
/// context that a cache read should have served — per event, when
/// `cache_write_tokens > 3 × input_tokens`, the excess
/// (`cache_write − 3×input`) is priced at the event model's input price ×
/// 1.25 (the cache-write multiplier). Events whose model has no known
/// price contribute 0. All projects appear (even zero-rewarm) so the TUI
/// can show hit rates; sorted by `rewarm_dollars` descending.
/// Project key matches `Dashboard::build`: `project.unwrap_or("—")`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    /// cache_read / (cache_read + input); 0.0 when the denominator is 0.
    pub hit_rate: f64,
    /// Cache-read tokens.
    pub reads: u64,
    /// Cache-write (creation) tokens.
    pub writes: u64,
    /// Estimated USD from avoidable context re-warming (heuristic).
    pub rewarm_dollars: f64,
}

/// Compute per-project cache stats from raw events (see [`CacheStats`]).
pub fn cache_stats(events: &[UsageEvent]) -> Vec<(String, CacheStats)> {
    use crate::model::price_per_mtok;

    // Accumulators per project: (reads, writes, input, rewarm_dollars)
    let mut acc: BTreeMap<String, (u64, u64, u64, f64)> = BTreeMap::new();
    for e in events {
        let key = e.project.clone().unwrap_or_else(|| "—".into());
        let entry = acc.entry(key).or_default();
        entry.0 += e.cache_read_tokens;
        entry.1 += e.cache_write_tokens;
        entry.2 += e.input_tokens;
        if e.cache_write_tokens > 3 * e.input_tokens {
            let wasteful = (e.cache_write_tokens - 3 * e.input_tokens) as f64;
            if let Some((in_price, _)) = price_per_mtok(&e.model) {
                entry.3 += wasteful / 1_000_000.0 * in_price * 1.25;
            }
        }
    }

    let mut out: Vec<(String, CacheStats)> = acc
        .into_iter()
        .map(|(project, (reads, writes, input, rewarm))| {
            let denom = (reads + input) as f64;
            let hit_rate = if denom > 0.0 {
                reads as f64 / denom
            } else {
                0.0
            };
            (
                project,
                CacheStats {
                    hit_rate,
                    reads,
                    writes,
                    rewarm_dollars: rewarm,
                },
            )
        })
        .collect();
    // rewarm desc; ties fall back to project name for stable output
    out.sort_by(|a, b| {
        b.1.rewarm_dollars
            .partial_cmp(&a.1.rewarm_dollars)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    out
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

    fn cache_ev(
        ts: i64,
        model: &str,
        project: Option<&str>,
        input: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> UsageEvent {
        UsageEvent {
            provider: "test".into(),
            model: model.into(),
            project: project.map(|s| s.to_string()),
            session: None,
            ts,
            input_tokens: input,
            output_tokens: 0,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            cost: None,
        }
    }

    // ── cache_stats ───────────────────────────────────────────────

    #[test]
    fn cache_hit_rate_simple() {
        // 90 cached reads vs 10 fresh input tokens → 0.9
        let events = vec![cache_ev(0, "claude-sonnet-4-5", Some("~/a"), 10, 90, 0)];
        let stats = cache_stats(&events);
        assert_eq!(stats.len(), 1);
        assert!((stats[0].1.hit_rate - 0.9).abs() < 1e-6);
        assert_eq!(stats[0].1.reads, 90);
        assert_eq!(stats[0].1.writes, 0);
    }

    #[test]
    fn cache_rewarm_dollars_sonnet() {
        // price_per_mtok("claude-sonnet-4-5-20250929") → sonnet → (3.0, 15.0).
        // cache_write 10M > 3×input 3M → wasteful 7M × 3.0/1M × 1.25 = 26.25
        let events = vec![cache_ev(
            0,
            "claude-sonnet-4-5-20250929",
            Some("~/a"),
            1_000_000,
            0,
            10_000_000,
        )];
        let stats = cache_stats(&events);
        assert_eq!(stats[0].1.writes, 10_000_000);
        assert!(
            (stats[0].1.rewarm_dollars - 26.25).abs() < 1e-6,
            "got {}",
            stats[0].1.rewarm_dollars
        );
    }

    #[test]
    fn cache_rewarm_zero_at_or_below_threshold() {
        // cache_write == 3×input exactly → threshold not exceeded → 0.0
        let events = vec![cache_ev(
            0,
            "claude-sonnet-4-5-20250929",
            Some("~/a"),
            1_000_000,
            0,
            3_000_000,
        )];
        let stats = cache_stats(&events);
        assert_eq!(stats[0].1.rewarm_dollars, 0.0);
        assert_eq!(stats[0].1.writes, 3_000_000, "writes still counted");
    }

    #[test]
    fn cache_hit_rate_zero_denominator_no_nan() {
        // no reads, no input → denominator 0 → hit_rate 0.0, never NaN
        let events = vec![cache_ev(0, "claude-sonnet-4-5", Some("~/a"), 0, 0, 0)];
        let stats = cache_stats(&events);
        assert_eq!(stats.len(), 1, "zero-rewarm project still listed");
        assert_eq!(stats[0].1.hit_rate, 0.0);
        assert!(!stats[0].1.hit_rate.is_nan());
        assert_eq!(stats[0].1.rewarm_dollars, 0.0);
    }

    #[test]
    fn cache_stats_per_project_sorted_by_rewarm_desc() {
        // ~/b re-warms 7M (26.25); ~/a re-warms 2M (7.5). Both listed,
        // sorted rewarm desc despite ~/a coming first alphabetically.
        let events = vec![
            cache_ev(
                0,
                "claude-sonnet-4-5-20250929",
                Some("~/a"),
                1_000_000,
                0,
                5_000_000,
            ),
            cache_ev(
                1,
                "claude-sonnet-4-5-20250929",
                Some("~/b"),
                1_000_000,
                0,
                10_000_000,
            ),
        ];
        let stats = cache_stats(&events);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].0, "~/b");
        assert!((stats[0].1.rewarm_dollars - 26.25).abs() < 1e-6);
        assert_eq!(stats[1].0, "~/a");
        assert!((stats[1].1.rewarm_dollars - 7.5).abs() < 1e-6);
    }

    #[test]
    fn cache_unknown_model_counts_tokens_but_no_rewarm() {
        // price lookup fails → rewarm contribution 0.0, tokens still exact
        let events = vec![cache_ev(
            0,
            "totally-unknown-xyz",
            Some("~/a"),
            1_000,
            50_000,
            9_999_999,
        )];
        let stats = cache_stats(&events);
        let s = &stats[0].1;
        assert_eq!(s.reads, 50_000);
        assert_eq!(s.writes, 9_999_999);
        assert_eq!(s.rewarm_dollars, 0.0);
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
