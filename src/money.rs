//! MoneyView — month-to-date spend, end-of-month projection, and
//! per-project budget tracking.
//!
//! Everything is derived from the event list plus an explicit `now`
//! (epoch seconds) parameter — never wall-clock time — so the struct is
//! pure and testable with fixed epochs.
//!
//! Definitions:
//! - `month_spend`: sum of event costs whose civil month equals `now`'s.
//! - `daily_avg`: trailing-7-day cost / 7; if that sum is 0, fall back to
//!   trailing-30-day cost / 30; else 0.0.
//! - `projected_month`: month_spend + daily_avg × days_remaining.
//! - `days_remaining`: days_in_month − day_of_month + 1 (TODAY COUNTS:
//!   spend can still accrue before midnight, so the last day yields 1).
//! - Budget lines: exact project-string match (events with `project: None`
//!   are never attributed to a budget), same 7d/30d daily-average fallback
//!   computed over that project's events only.

use crate::aggregator::{days_in_month_and_day, month_key};
use crate::model::UsageEvent;
use std::collections::BTreeMap;

/// Tolerance for the over-budget comparison: `over = projected > budget + EPSILON`.
pub const OVER_EPSILON: f64 = 1e-4;

/// Month-to-date money picture, ready for the snapshot / report renderer.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoneyView {
    /// USD spent so far in the current calendar month (all events).
    pub month_spend: f64,
    /// month_spend + daily_avg × days_remaining.
    pub projected_month: f64,
    /// Trailing-7d cost / 7 (fallback trailing-30d / 30, else 0.0).
    pub daily_avg: f64,
    /// Days left in the current month including today (min 1).
    pub days_remaining: u32,
    /// One line per configured budget, in budget-name order (BTreeMap).
    pub budgets: Vec<BudgetLine>,
}

/// A configured per-project monthly budget vs. actual/projected spend.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetLine {
    pub project: String,
    pub budget: f64,
    /// Month-to-date spend for this project.
    pub spent: f64,
    /// spent + this project's daily_avg × days_remaining.
    pub projected: f64,
    /// projected > budget + OVER_EPSILON.
    pub over: bool,
}

/// Trailing-`days`-day cost sum / days, over the given events.
/// None when the window sum is 0 (nothing to project from).
fn trailing_daily_avg<'a, I>(events: I, now: i64, days: i64) -> Option<f64>
where
    I: IntoIterator<Item = &'a UsageEvent>,
{
    let start = now - days * 86_400;
    let sum: f64 = events
        .into_iter()
        .filter(|e| e.ts >= start && e.ts <= now)
        .map(|e| e.cost.unwrap_or(0.0))
        .sum();
    if sum > 0.0 {
        Some(sum / days as f64)
    } else {
        None
    }
}

impl MoneyView {
    /// Derive the money picture from `events` as of `now` (epoch secs).
    /// Budget keys must EXACTLY match `event.project` strings; events with
    /// `project: None` are never attributed to a budget.
    pub fn build(events: &[UsageEvent], now: i64, budgets: &BTreeMap<String, f64>) -> Self {
        let this_month = month_key(now);

        // ── Month-to-date spend (all events) ────────────────────────
        let month_spend: f64 = events
            .iter()
            .filter(|e| month_key(e.ts) == this_month)
            .map(|e| e.cost.unwrap_or(0.0))
            .sum();

        // ── Global daily average: trailing 7d, fallback trailing 30d ─
        let daily_avg = trailing_daily_avg(events, now, 7)
            .or_else(|| trailing_daily_avg(events, now, 30))
            .unwrap_or(0.0);

        // ── Projection to month end (today still counts) ────────────
        let (days_in_month, day_of_month) = days_in_month_and_day(now);
        let days_remaining = days_in_month - day_of_month + 1;
        let projected_month = month_spend + daily_avg * days_remaining as f64;

        // ── Per-project budget lines ─────────────────────────────────
        let budgets = budgets
            .iter()
            .map(|(project, &budget)| {
                let own: Vec<&UsageEvent> = events
                    .iter()
                    .filter(|e| e.project.as_deref() == Some(project.as_str()))
                    .collect();
                let spent: f64 = own
                    .iter()
                    .filter(|e| month_key(e.ts) == this_month)
                    .map(|e| e.cost.unwrap_or(0.0))
                    .sum();
                let avg = trailing_daily_avg(own.iter().copied(), now, 7)
                    .or_else(|| trailing_daily_avg(own.iter().copied(), now, 30))
                    .unwrap_or(0.0);
                let projected = spent + avg * days_remaining as f64;
                BudgetLine {
                    project: project.clone(),
                    budget,
                    spent,
                    projected,
                    over: projected > budget + OVER_EPSILON,
                }
            })
            .collect();

        MoneyView {
            month_spend,
            projected_month,
            daily_avg,
            days_remaining,
            budgets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// now = 1_800_000_000 → 2027-01-15T08:00Z (verified: `date -u -d @1800000000`).
    /// January has 31 days → days_remaining = 31 − 15 + 1 = 17.
    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;
    /// 2027-01-01T00:00Z = NOW − 14 days − 8h.
    const JAN1: i64 = 1_798_761_600;

    fn ev(ts: i64, project: Option<&str>, cost: f64) -> UsageEvent {
        UsageEvent {
            provider: "test".into(),
            model: "m".into(),
            project: project.map(|s| s.to_string()),
            session: None,
            ts,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: Some(cost),
        }
    }

    #[test]
    fn month_spend_daily_avg_and_projection() {
        // 7 × $1 inside the trailing-7d window [NOW−7d, NOW]…
        #[allow(unused_mut)] // push below mutates; fmt collapses the let
        let mut events: Vec<UsageEvent> = (0..7).map(|k| ev(NOW - k * DAY, None, 1.0)).collect();
        // …plus $5 on Jan 1: same month, OUTSIDE the 7d window.
        events.push(ev(JAN1 + 3_600, None, 5.0));

        let mv = MoneyView::build(&events, NOW, &BTreeMap::new());
        assert_eq!(mv.month_spend, 12.0, "7×$1 + $5, all in Jan 2027");
        assert_eq!(mv.daily_avg, 1.0, "trailing-7d sum 7.0 / 7");
        assert_eq!(mv.days_remaining, 17, "Jan 15 in a 31-day month");
        assert_eq!(mv.projected_month, 12.0 + 1.0 * 17.0);
        assert!(mv.budgets.is_empty(), "no budgets configured");
    }

    #[test]
    fn budget_line_projected_and_over() {
        // now_b = NOW + 10 days → 2027-01-25T08:00Z; days_remaining = 31−25+1 = 7.
        let now_b: i64 = 1_800_864_000;
        let mut budgets = BTreeMap::new();
        budgets.insert("~/claude/lantrn".to_string(), 10.0);

        let mut events: Vec<UsageEvent> = (0..4)
            .map(|k| ev(now_b - k * DAY, Some("~/claude/lantrn"), 1.0))
            .collect();
        // $4 earlier in the month but outside the 7d window.
        events.push(ev(JAN1 + DAY, Some("~/claude/lantrn"), 4.0));
        // A different project: counts in month_spend, NOT in the budget line.
        events.push(ev(now_b - DAY, Some("other-proj"), 2.0));

        let mv = MoneyView::build(&events, now_b, &budgets);
        assert_eq!(mv.month_spend, 10.0, "8.0 lantrn + 2.0 other");
        let line = &mv.budgets[0];
        assert_eq!(line.project, "~/claude/lantrn");
        assert_eq!(line.budget, 10.0);
        assert_eq!(line.spent, 8.0, "exact project match only");
        // projected = 8 + (4/7)×7 ≈ 12.0 (f64 rounding < 1e-9).
        assert!(
            (line.projected - 12.0).abs() < 1e-9,
            "got {}",
            line.projected
        );
        assert!(line.over, "12.0 > 10.0 + epsilon");
    }

    #[test]
    fn empty_events_zero_money_but_budgets_listed() {
        let mut budgets = BTreeMap::new();
        budgets.insert("proj-a".to_string(), 10.0);
        let mv = MoneyView::build(&[], NOW, &budgets);
        assert_eq!(mv.month_spend, 0.0);
        assert_eq!(mv.daily_avg, 0.0);
        assert_eq!(mv.projected_month, 0.0);
        assert_eq!(mv.days_remaining, 17);
        assert_eq!(mv.budgets.len(), 1);
        let line = &mv.budgets[0];
        assert_eq!(line.project, "proj-a");
        assert_eq!(line.spent, 0.0);
        assert_eq!(line.projected, 0.0);
        assert!(!line.over);
    }

    #[test]
    fn days_remaining_is_inclusive_of_today() {
        // 1_800_691_200 → 2027-01-23T08:00Z → 31 − 23 + 1 = 9.
        let now_23: i64 = 1_800_691_200;
        let mv = MoneyView::build(&[], now_23, &BTreeMap::new());
        assert_eq!(mv.days_remaining, 9);

        // 1_798_675_200 → 2026-12-31T00:00Z → last day of month → 1.
        let mv31 = MoneyView::build(&[], 1_798_675_200, &BTreeMap::new());
        assert_eq!(mv31.days_remaining, 1, "today counts, even on the last day");
    }

    #[test]
    fn daily_avg_falls_back_to_trailing_30d_then_zero() {
        // Nothing in the last 7 days; $14 exactly 20 days back (still ≤ 30d).
        let mv = MoneyView::build(&[ev(NOW - 20 * DAY, None, 14.0)], NOW, &BTreeMap::new());
        assert!(
            (mv.daily_avg - 14.0 / 30.0).abs() < 1e-12,
            "30d fallback: got {}",
            mv.daily_avg
        );

        // Nothing within 30 days either → 0.0.
        let mv0 = MoneyView::build(&[ev(NOW - 40 * DAY, None, 99.0)], NOW, &BTreeMap::new());
        assert_eq!(mv0.daily_avg, 0.0);

        // A project with no recent events projects 0 spend on top of month spend.
        let mut budgets = BTreeMap::new();
        budgets.insert("p".to_string(), 100.0);
        let events = vec![
            ev(NOW - 20 * DAY, Some("p"), 14.0), // 30d fallback, outside month? Dec 26 → different month
        ];
        let mv2 = MoneyView::build(&events, NOW, &budgets);
        let line = &mv2.budgets[0];
        assert_eq!(line.spent, 0.0, "Dec 26 spend is not January spend");
        assert!((line.projected - (14.0 / 30.0) * 17.0).abs() < 1e-9);
    }
}
