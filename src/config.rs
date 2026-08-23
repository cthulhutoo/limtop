use std::collections::HashMap;
use std::path::Path;

/// User configuration from `~/.config/limtop.toml`.
///
/// Every field is optional; a missing file (or a bad one) yields defaults
/// and limtop behaves exactly as it did before config existed.
///
/// ```toml
/// plan = "max20"                      # pro | max5 | max20 | custom:<n> | <n>
/// ollama_url = "http://nas:11434"
/// extra_claude_dirs = ["~/work/claude-alt"]   # each scanned for projects/
/// [pricing."claude-sonnet-4-5-20250929"]      # USD per Mtok; unset = builtin
/// input = 3.0
/// output = 15.0
/// ```
#[derive(Debug, Default, serde::Deserialize)]
pub struct Config {
    pub plan: Option<String>,
    pub ollama_url: Option<String>,
    pub extra_claude_dirs: Option<Vec<String>>,
    pub pricing: Option<HashMap<String, PricingOverride>>,
}

/// Per-model price override, USD per million tokens.
/// `None` fields keep the builtin price.
#[derive(Debug, Default, PartialEq, serde::Deserialize)]
pub struct PricingOverride {
    pub input: Option<f64>,
    pub output: Option<f64>,
}

impl Config {
    /// Load `~/.config/limtop.toml` (via dirs::config_dir). Missing file or
    /// unreadable content → defaults; bad TOML → warning + defaults. Never
    /// panics, never blocks startup.
    pub fn load() -> Config {
        let Some(dir) = dirs::config_dir() else {
            return Config::default();
        };
        load_path(&dir.join("limtop.toml"))
    }

    /// Effective plan string: env `LIMTOP_CLAUDE_LIMIT` beats the file.
    pub fn plan_or_env(&self) -> Option<String> {
        std::env::var("LIMTOP_CLAUDE_LIMIT")
            .ok()
            .or_else(|| self.plan.clone())
    }

    /// Effective ollama URL setting from the file (env still wins at use
    /// site in providers::ollama). None = no config opinion.
    pub fn ollama_url(&self) -> Option<&str> {
        self.ollama_url.as_deref()
    }

    /// Extra claude roots, `~`-expanded. Empty when unconfigured.
    pub fn claude_roots(&self) -> Vec<String> {
        self.extra_claude_dirs
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|s| expand_tilde(s))
            .collect()
    }
}

/// `~/x` → `$HOME/x`; other paths untouched.
fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    p.to_string()
}

/// Serialises tests that mutate LIMTOP_CLAUDE_LIMIT (env is process-global;
/// cargo test runs in parallel threads).
#[cfg(test)]
pub static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Parse config TOML text. Bad TOML → default (caller decides whether to warn).
pub fn parse(s: &str) -> Config {
    match toml::from_str::<Config>(s) {
        Ok(c) => c,
        Err(_) => Config::default(),
    }
}

/// Load from a specific path (so tests never touch the user's real config).
/// Bad TOML → warning on stderr + defaults.
pub fn load_path(p: &Path) -> Config {
    let Ok(text) = std::fs::read_to_string(p) else {
        return Config::default();
    };
    match parse_result(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("limtop: bad config {} ({e}) — using defaults", p.display());
            Config::default()
        }
    }
}

fn parse_result(s: &str) -> Result<Config, toml::de::Error> {
    toml::from_str::<Config>(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
plan = "max20"
ollama_url = "http://nas.local:11434"
extra_claude_dirs = ["~/work/alt", "/srv/shared"]

[pricing."claude-sonnet-4-5-20250929"]
input = 3.0
output = 15.0

[pricing."gpt-5.1"]
input = 2.0
"#;

    #[test]
    fn parses_full_config() {
        let c = parse(FULL);
        assert_eq!(c.plan.as_deref(), Some("max20"));
        assert_eq!(c.ollama_url.as_deref(), Some("http://nas.local:11434"));
        assert_eq!(
            c.extra_claude_dirs.as_deref(),
            Some(["~/work/alt".to_string(), "/srv/shared".to_string()].as_slice())
        );
        let pricing = c.pricing.expect("pricing map present");
        let sonnet = pricing
            .get("claude-sonnet-4-5-20250929")
            .expect("sonnet override");
        assert_eq!(sonnet.input, Some(3.0));
        assert_eq!(sonnet.output, Some(15.0));
        let gpt = pricing.get("gpt-5.1").expect("gpt override");
        assert_eq!(gpt.input, Some(2.0));
        assert_eq!(gpt.output, None, "unset output stays None (keep builtin)");
    }

    #[test]
    fn empty_string_is_defaults() {
        let c = parse("");
        assert_eq!(c.plan, None);
        assert_eq!(c.ollama_url, None);
        assert_eq!(c.extra_claude_dirs, None);
        assert_eq!(c.pricing, None);
    }

    #[test]
    fn bad_toml_is_defaults() {
        let c = parse("this is [not valid toml");
        assert_eq!(c.plan, None);
        assert_eq!(c.ollama_url, None);
    }

    #[test]
    fn load_missing_file_is_default() {
        let c = load_path(Path::new("/nonexistent/limtop-test-should-not-exist.toml"));
        assert_eq!(c.plan, None);
    }

    #[test]
    fn load_bad_file_is_default() {
        // write garbage to a unique temp file — NOT the user's config
        let p = std::env::temp_dir().join(format!("limtop-test-bad-{}.toml", std::process::id()));
        std::fs::write(&p, "not [ toml").unwrap();
        let c = load_path(&p);
        assert_eq!(c.plan, None);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn plan_env_beats_file() {
        // only test that manipulates LIMTOP_CLAUDE_LIMIT
        std::env::remove_var("LIMTOP_CLAUDE_LIMIT");
        let c = parse("plan = \"max5\"\n");
        assert_eq!(
            c.plan_or_env().as_deref(),
            Some("max5"),
            "file value when env unset"
        );
        std::env::set_var("LIMTOP_CLAUDE_LIMIT", "max20");
        assert_eq!(c.plan_or_env().as_deref(), Some("max20"), "env beats file");
        std::env::remove_var("LIMTOP_CLAUDE_LIMIT");
        assert_eq!(
            Config::default().plan_or_env(),
            None,
            "nothing configured → None (caller uses builtin default)"
        );
    }

    #[test]
    fn ollama_url_and_roots_accessors() {
        let c = parse(FULL);
        assert_eq!(c.ollama_url(), Some("http://nas.local:11434"));
        assert_eq!(Config::default().ollama_url(), None);
        assert_eq!(c.claude_roots().len(), 2);
        assert!(Config::default().claude_roots().is_empty());
    }

    #[test]
    fn claude_roots_expand_tilde() {
        let c = parse("extra_claude_dirs = [\"~/alt\", \"/abs/path\"]\n");
        let home = dirs::home_dir().unwrap();
        let roots = c.claude_roots();
        assert_eq!(
            roots[0],
            home.join("alt").to_string_lossy().to_string(),
            "~/alt expands under $HOME"
        );
        assert_eq!(roots[1], "/abs/path", "absolute paths untouched");
    }
}
