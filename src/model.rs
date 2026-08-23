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
    let (pi, po) = price_per_mtok(model)?;
    let cached = e.cache_read_tokens as f64 / 1_000_000.0 * pi * 0.1;
    let cache_w = e.cache_write_tokens as f64 / 1_000_000.0 * pi * 1.25;
    let input = e.input_tokens as f64 / 1_000_000.0 * pi;
    let output = e.output_tokens as f64 / 1_000_000.0 * po;
    Some(input + output + cached + cache_w)
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
