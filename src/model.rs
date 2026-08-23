use std::fmt;

/// A single billable LLM API call observed by a provider.
#[derive(Debug, Clone)]
pub struct UsageEvent {
    /// Provider that produced this event ("claude-code", "hermes", "opencode").
    pub provider: String,
    /// Model called (e.g. "claude-haiku-4-5-20251001").
    pub model: String,
    /// Project / working directory context, if known.
    pub project: Option<String>,
    /// Session identifier, if known.
    pub session: Option<String>,
    /// Unix epoch seconds.
    pub ts: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cached (re-read) input tokens — billed at a discount or free.
    pub cache_read_tokens: u64,
    /// Tokens written to the cache on this call.
    pub cache_write_tokens: u64,
    /// Cost in USD if the provider reported it directly; otherwise computed.
    pub cost: Option<f64>,
}

impl UsageEvent {
    /// Total tokens including cache traffic.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }
}

/// Aggregate over a window.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost: f64,
    pub events: u64,
}

impl UsageTotals {
    pub fn add(&mut self, e: &UsageEvent) {
        self.input_tokens += e.input_tokens;
        self.output_tokens += e.output_tokens;
        self.cache_read_tokens += e.cache_read_tokens;
        self.cache_write_tokens += e.cache_write_tokens;
        self.cost += e.cost.unwrap_or(0.0);
        self.events += 1;
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }
}

/// A discovered provider installation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderStatus {
    pub name: String,
    pub detected: bool,
    pub detail: String,
}

/// Every provider implements this: scan disk, emit usage events.
pub trait Provider {
    fn name(&self) -> &'static str;
    fn detect(&self, home: &std::path::Path) -> Option<std::path::PathBuf>;
}

// ── Pricing (USD per million tokens) ────────────────────────────────────────
// Sourced from public pricing pages, early 2026. Cache writes cost ~1.25x
// input; cache reads ~0.1x input. Where unknown, cost = None (excluded from
// totals rather than guessing).

pub fn price_per_mtok(model: &str) -> Option<(f64, f64)> {
    let m = model.to_ascii_lowercase();
    let (i, o) = if m.contains("opus-4") {
        (15.0, 75.0)
    } else if m.contains("sonnet-4-6") || m.contains("sonnet-4.6") {
        (3.0, 15.0)
    } else if m.contains("sonnet") {
        (3.0, 15.0)
    } else if m.contains("haiku-4") || m.contains("haiku-4-5") {
        (1.0, 5.0)
    } else if m.contains("haiku") {
        (0.80, 4.0)
    } else if m.contains("gpt-5.2") || m.contains("gpt-5-2") {
        (1.25, 10.0)
    } else if m.contains("gpt-5.1") || m.contains("gpt-5-1") {
        (1.25, 10.0)
    } else if m.contains("gpt-5") {
        (1.25, 10.0)
    } else if m.contains("gpt-4.5") || m.contains("gpt-4-5") {
        (27.0, 55.0) // preview pricing
    } else if m.contains("gpt-4o-mini") {
        (0.15, 0.60)
    } else if m.contains("gpt-4o") {
        (2.50, 10.0)
    } else if m.contains("o3") {
        (2.0, 8.0)
    } else if m.contains("o4-mini") {
        (1.10, 4.40)
    } else if m.contains("gemini-3") || m.contains("gemini-3.5") {
        (2.0, 12.0)
    } else if m.contains("gemini-2.5-pro") || m.contains("gemini-2-5-pro") {
        (1.25, 10.0)
    } else if m.contains("gemini-2.5-flash") || m.contains("gemini-2-5-flash") {
        (0.30, 2.50)
    } else if m.contains("gemini-2.5-flash-lite") {
        (0.10, 0.40)
    } else if m.contains("deepseek-v4") || m.contains("deepseek-chat") {
        (0.27, 1.10)
    } else if m.contains("deepseek-r") {
        (0.55, 2.20)
    } else if m.contains("glm-5") || m.contains("glm-4.7") {
        (0.50, 2.0)
    } else if m.contains("kimi-k2") || m.contains("moonshot") {
        (0.60, 2.50)
    } else if m.contains("llama") || m.contains("qwen") || m.contains("mistral") {
        (0.0, 0.0) // local/open weights — free
    } else {
        return None;
    };
    Some((i, o))
}

pub fn estimate_cost(model: &str, e: &UsageEvent) -> Option<f64> {
    // consult config overrides (installed once at startup); empty = builtin
    static EMPTY: std::sync::OnceLock<
        std::collections::HashMap<String, crate::config::PricingOverride>,
    > = std::sync::OnceLock::new();
    let ov = PRICING_OVERRIDES
        .get()
        .unwrap_or_else(|| EMPTY.get_or_init(Default::default));
    estimate_cost_with(model, e, ov)
}

/// Config price overrides (model → per-Mtok prices), set once at startup
/// from ~/.config/limtop.toml. A global is used deliberately: estimate_cost
/// is called from 5+ provider parsers deep in the scan path, and threading
/// a &HashMap through every provider would break the Provider API for a
/// rarely-changed setting. Documented trade-off; see set_pricing_overrides.
static PRICING_OVERRIDES: std::sync::OnceLock<
    std::collections::HashMap<String, crate::config::PricingOverride>,
> = std::sync::OnceLock::new();

/// Install config pricing overrides (called once from main). First call
/// wins; later calls are ignored (tests must not fight each other over it).
pub fn set_pricing_overrides(m: std::collections::HashMap<String, crate::config::PricingOverride>) {
    let _ = PRICING_OVERRIDES.set(m);
}

/// Resolve (input, output) per-Mtok prices for `model`: builtin, patched by
/// any config override for that exact model name.
pub fn price_per_mtok_with(
    model: &str,
    ov: &std::collections::HashMap<String, crate::config::PricingOverride>,
) -> Option<(f64, f64)> {
    let builtin = price_per_mtok(model);
    let Some(p) = ov.get(model) else {
        return builtin;
    };
    // exact-match override patches only the fields it sets; an override on
    // a model with NO builtin price only works if both fields are set
    // (we never guess the missing half)
    let i = p.input.or(builtin.map(|b| b.0));
    let o = p.output.or(builtin.map(|b| b.1));
    match (i, o) {
        (Some(i), Some(o)) => Some((i, o)),
        _ => None,
    }
}

/// estimate_cost with an explicit overrides table (pure; testable).
pub fn estimate_cost_with(
    model: &str,
    e: &UsageEvent,
    ov: &std::collections::HashMap<String, crate::config::PricingOverride>,
) -> Option<f64> {
    let (pi, po) = price_per_mtok_with(model, ov)?;
    let cached = e.cache_read_tokens as f64 / 1_000_000.0 * pi * 0.1;
    let cache_w = e.cache_write_tokens as f64 / 1_000_000.0 * pi * 1.25;
    let input = e.input_tokens as f64 / 1_000_000.0 * pi;
    let output = e.output_tokens as f64 / 1_000_000.0 * po;
    Some(input + output + cached + cache_w)
}

#[cfg(test)]
mod pricing_tests {
    use super::*;
    use crate::config::PricingOverride;

    fn ev(model: &str, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            provider: "claude-code".into(),
            model: model.into(),
            project: None,
            session: None,
            ts: 0,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: None,
        }
    }

    fn overrides(
        pairs: &[(&str, Option<f64>, Option<f64>)],
    ) -> std::collections::HashMap<String, PricingOverride> {
        pairs
            .iter()
            .map(|(m, i, o)| {
                (
                    m.to_string(),
                    PricingOverride {
                        input: *i,
                        output: *o,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn override_patches_price_for_exact_model() {
        let e = ev("claude-sonnet-4-5", 1_000_000, 1_000_000);
        let base = estimate_cost("claude-sonnet-4-5", &e).unwrap(); // 3+15 = 18
        let ov = overrides(&[("claude-sonnet-4-5", Some(99.0), Some(99.0))]);
        let got = estimate_cost_with("claude-sonnet-4-5", &e, &ov).unwrap();
        assert!(
            (got - 198.0).abs() < 1e-9,
            "full override: 99+99, was {base}"
        );
    }

    #[test]
    fn partial_override_keeps_other_builtin_field() {
        let e = ev("claude-sonnet-4-5", 1_000_000, 0);
        // only input overridden; builtin output 15.0 irrelevant (no output tokens)
        let ov = overrides(&[("claude-sonnet-4-5", Some(10.0), None)]);
        let got = estimate_cost_with("claude-sonnet-4-5", &e, &ov).unwrap();
        assert!((got - 10.0).abs() < 1e-9, "input override only");
        // now with output tokens: builtin output 15.0 must survive
        let e2 = ev("claude-sonnet-4-5", 0, 1_000_000);
        let got2 = estimate_cost_with("claude-sonnet-4-5", &e2, &ov).unwrap();
        assert!((got2 - 15.0).abs() < 1e-9, "None output keeps builtin 15.0");
    }

    #[test]
    fn override_for_other_model_is_noop() {
        let e = ev("claude-sonnet-4-5", 1_000_000, 0);
        let ov = overrides(&[("gpt-9", Some(99.0), Some(99.0))]);
        let got = estimate_cost_with("claude-sonnet-4-5", &e, &ov).unwrap();
        assert!((got - 3.0).abs() < 1e-9, "unrelated override ignored");
    }

    #[test]
    fn override_prices_unknown_builtin_model() {
        let e = ev("my-private-finetune", 1_000_000, 1_000_000);
        assert!(estimate_cost("my-private-finetune", &e).is_none());
        let ov = overrides(&[("my-private-finetune", Some(1.0), Some(2.0))]);
        let got = estimate_cost_with("my-private-finetune", &e, &ov).unwrap();
        assert!(
            (got - 3.0).abs() < 1e-9,
            "override gives price to unknown model"
        );
    }

    #[test]
    fn empty_overrides_equal_builtin() {
        let e = ev("claude-opus-4", 1_000_000, 1_000_000);
        let empty = std::collections::HashMap::new();
        let a = estimate_cost("claude-opus-4", &e).unwrap();
        let b = estimate_cost_with("claude-opus-4", &e, &empty).unwrap();
        assert!((a - b).abs() < 1e-12);
    }

    #[test]
    fn estimate_cost_consults_installed_global() {
        // unique model name so the installed global can't disturb other
        // tests (they look up different keys and miss)
        let m = "global-delegation-test-model-only";
        let e = ev(m, 1_000_000, 0);
        assert!(
            estimate_cost(m, &e).is_none(),
            "no builtin, no override yet"
        );
        let mut map = std::collections::HashMap::new();
        map.insert(
            m.to_string(),
            PricingOverride {
                input: Some(7.0),
                output: Some(7.0),
            },
        );
        set_pricing_overrides(map); // OnceLock: first set in the binary wins
        let got = estimate_cost(m, &e).expect("global override applies");
        assert!((got - 7.0).abs() < 1e-9);
    }
}

/// Compact human-readable byte/token counts: 1.2M, 847k, 512
pub fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn fmt_cost(c: f64) -> String {
    if c >= 1_000.0 {
        format!("${:.1}k", c / 1_000.0)
    } else if c >= 10.0 {
        format!("${:.2}", c)
    } else {
        format!("${:.4}", c)
    }
}

/// Simple span for time windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Span {
    #[default]
    Day,
    Hour,
    Week,
    Month,
    All,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Span::Hour => write!(f, "1h"),
            Span::Day => write!(f, "24h"),
            Span::Week => write!(f, "7d"),
            Span::Month => write!(f, "30d"),
            Span::All => write!(f, "all"),
        }
    }
}

impl Span {
    pub fn seconds(&self) -> Option<i64> {
        match self {
            Span::Hour => Some(3600),
            Span::Day => Some(86_400),
            Span::Week => Some(604_800),
            Span::Month => Some(2_592_000),
            Span::All => None,
        }
    }
}
