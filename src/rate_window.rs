use crate::model::{estimate_cost, UsageEvent};
use std::time::{SystemTime, UNIX_EPOCH};

/// Derived Claude rate-limit window state.
///
/// Claude Code does not persist its server-side 5h usage windows to disk,
/// so limtop reconstructs one from local transcripts: the trailing 5-hour
/// token sum of Claude events. Limits come from plan presets (env-tunable):
///
///   LIMTOP_CLAUDE_LIMIT=pro|max5|max20|custom:<n>   (default: pro)
///
/// Presets are community-observed approximations, not official numbers.
#[derive(Debug, Clone, Copy)]
pub struct RateWindow {
    /// epoch seconds when the current window started
    pub window_start: i64,
    /// window length (5h for Claude)
    pub window_secs: i64,
    /// tokens (weighted) consumed in the current window
    pub used: i64,
    /// plan limit in weighted tokens
    pub limit: i64,
    /// weighted tokens/hour over the window so far
    pub burn_rate: f64,
    /// when the window resets (epoch)
    pub resets_at: i64,
}

impl RateWindow {
    /// Weighted total: cache reads count 0.1x (matches how limits bite).
    fn weighted(e: &UsageEvent) -> i64 {
        e.input_tokens as i64
            + e.output_tokens as i64
            + e.cache_write_tokens as i64
            + (e.cache_read_tokens as i64 / 10)
    }

    pub fn build(claude_events: &[UsageEvent], now: i64) -> Option<Self> {
        let claude_events: Vec<&UsageEvent> = claude_events
            .iter()
            .filter(|e| e.provider == "claude-code")
            .collect();
        if claude_events.is_empty() {
            return None;
        }

        let window_secs = 18_000; // 5h
        let window_start = now - window_secs;
        let in_window: Vec<&UsageEvent> = claude_events
            .into_iter()
            .filter(|e| e.ts >= window_start && e.ts <= now)
            .collect();
        if in_window.is_empty() {
            // still show a window if claude was used at all (all-time)
            return None;
        }

        let used: i64 = in_window.iter().map(|e| Self::weighted(e)).sum();
        let elapsed = (now - window_start).max(1);
        let burn_rate = used as f64 / (elapsed as f64 / 3600.0);
        // the oldest in-window usage ages out of the trailing window here
        let oldest = in_window.iter().map(|e| e.ts).min().unwrap_or(now);
        let resets_at = oldest + window_secs;

        Some(RateWindow {
            window_start,
            used,
            limit: plan_limit(),
            burn_rate,
            resets_at,
            window_secs,
        })
    }

    /// The oldest event in-window defines the true start (usage ages out
    /// gradually in a trailing window).
    pub fn pct_used(&self) -> f64 {
        if self.limit <= 0 {
            return 0.0;
        }
        (self.used as f64 / self.limit as f64).clamp(0.0, 1.0)
    }

    pub fn limit_name(&self) -> &'static str {
        plan_limit_name()
    }
}

// ── plan presets ─────────────────────────────────────────────────────
// Community-observed approximations for Claude subscription plans,
// weighted tokens per 5h window. NOT official numbers.
fn plan_limit() -> i64 {
    match std::env::var("LIMTOP_CLAUDE_LIMIT").as_deref() {
        Ok("pro") | Err(_) => 1_000_000,
        Ok("max5") => 2_200_000,
        Ok("max20") => 22_000_000,
        Ok("custom") => 1_000_000,
        Ok(v) if v.starts_with("custom:") => v
            .strip_prefix("custom:")
            .and_then(|n| n.parse().ok())
            .unwrap_or(1_000_000),
        Ok(other) => {
            // numeric directly
            other.parse().unwrap_or(1_000_000)
        }
    }
}

fn plan_limit_name() -> &'static str {
    match std::env::var("LIMTOP_CLAUDE_LIMIT").as_deref() {
        Ok("pro") | Err(_) => "pro",
        Ok("max5") => "max5",
        Ok("max20") => "max20",
        _ => "custom",
    }
}

/// convenience for dump mode
pub fn fmt_window(w: &RateWindow) -> String {
    let pct = w.pct_used() * 100.0;
    let reset_h = (w.resets_at - now_epoch()).max(0) as f64 / 3600.0;
    format!(
        "claude 5h window: {} / {} ({}%) · burn {} tok/h · resets in {:.1}h [{}]",
        crate::model::fmt_tokens(w.used as u64),
        crate::model::fmt_tokens(w.limit as u64),
        pct as u64,
        crate::model::fmt_tokens(w.burn_rate as u64),
        reset_h,
        w.limit_name()
    )
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ts: i64, input: u64, output: u64, cache_read: u64) -> UsageEvent {
        let mut e = UsageEvent {
            provider: "claude-code".into(),
            project: Some("p".into()),
            session: Some("s".into()),
            model: "claude-sonnet-4-5".into(),
            ts,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
            cost: None,
        };
        e.cost = estimate_cost(&e.model, &e);
        e
    }

    #[test]
    fn weighted_sums() {
        let now = 1_800_000_000;
        // 1 event: 100k input, 10k output, 500k cache-read (→50k weighted)
        let evs = vec![ev(now - 1000, 100_000, 10_000, 500_000)];
        let w = RateWindow::build(&evs, now).unwrap();
        assert_eq!(w.used, 100_000 + 10_000 + 50_000); // 160k
        assert_eq!(w.limit, 1_000_000); // default pro
        assert!((w.pct_used() - 0.16).abs() < 0.001);
    }

    #[test]
    fn old_events_excluded() {
        let now = 1_800_000_000;
        // inside 5h (window = 18000s) vs long gone
        let evs = vec![
            ev(now - 17_900, 999_999, 0, 0),
            ev(now - 30_000, 1_000_000, 0, 0),
        ];
        let w = RateWindow::build(&evs, now).unwrap();
        assert_eq!(w.used, 999_999);
    }

    #[test]
    fn custom_limit_env() {
        std::env::set_var("LIMTOP_CLAUDE_LIMIT", "custom:500000");
        let now = 1_800_000_000;
        let evs = vec![ev(now - 1000, 250_000, 0, 0)];
        let w = RateWindow::build(&evs, now).unwrap();
        assert_eq!(w.limit, 500_000);
        std::env::remove_var("LIMTOP_CLAUDE_LIMIT");
    }

    #[test]
    fn zero_usage_no_window() {
        let evs: Vec<UsageEvent> = vec![];
        assert!(RateWindow::build(&evs, 1_800_000_000).is_none());
        let old = vec![ev(1_700_000_000, 100, 0, 0)];
        assert!(RateWindow::build(&old, 1_800_000_000).is_none());
    }

    #[test]
    fn burn_rate_sane() {
        let now = 1_800_000_000;
        // one event 1h ago of 100k weighted → 100k/h over 5h-window elapsed
        let evs = vec![ev(now - 3_600, 100_000, 0, 0)];
        let w = RateWindow::build(&evs, now).unwrap();
        // elapsed = 3600+... window_start = now-18000, event at now-3600
        // used=100k, elapsed=18000 → rate = 100000/(18000/3600) ≈ 20k/h
        assert!(w.burn_rate > 0.0 && w.burn_rate < 100_000.0);
    }
}
