pub mod claude;
pub mod codex;
pub mod gemini;
pub mod hermes;
pub mod opencode;

use crate::model::{ProviderStatus, UsageEvent};
use std::path::Path;

/// Registry of all known providers.
pub struct Registry {
    pub statuses: Vec<ProviderStatus>,
    pub events: Vec<UsageEvent>,
}

impl Registry {
    pub fn scan(home: &Path) -> Self {
        let mut statuses = Vec::new();
        let mut events = Vec::new();

        // Claude Code
        let claude_ok = claude::detected(home);
        statuses.push(ProviderStatus {
            name: "claude-code".into(),
            detected: claude_ok,
            detail: "~/.claude/projects".into(),
        });
        if claude_ok {
            events.extend(claude::ClaudeProvider::load(home));
        }

        // Codex
        let codex_ok = codex::status(home).is_some();
        statuses.push(ProviderStatus {
            name: "codex".into(),
            detected: codex_ok,
            detail: "~/.codex (sqlite + rollouts)".into(),
        });
        if codex_ok {
            events.extend(codex::CodexProvider::load(home));
        }

        // Gemini CLI
        let (gemini_ok, gemini_detail) =
            gemini::status(home).unwrap_or((false, "~/.gemini/tmp".into()));
        statuses.push(ProviderStatus {
            name: "gemini".into(),
            detected: gemini_ok,
            detail: gemini_detail,
        });
        if gemini_ok {
            events.extend(gemini::GeminiProvider::load(home));
        }

        // Hermes
        let hermes_ok = hermes::detected(home);
        statuses.push(ProviderStatus {
            name: "hermes".into(),
            detected: hermes_ok,
            detail: "~/.hermes/state.db".into(),
        });
        if hermes_ok {
            events.extend(hermes::HermesProvider::load(home));
        }

        // opencode
        let oc_ok = opencode::detected(home);
        statuses.push(ProviderStatus {
            name: "opencode".into(),
            detected: oc_ok,
            detail: "~/.local/share/opencode".into(),
        });
        if oc_ok {
            events.extend(opencode::OpencodeProvider::load(home));
        }

        events.sort_by(|a, b| b.ts.cmp(&a.ts));
        Registry { statuses, events }
    }
}
