# limtop

[![Crates.io](https://img.shields.io/crates/v/limtop.svg)](https://crates.io/crates/limtop)
[![CI](https://github.com/cthulhutoo/limtop/actions/workflows/ci.yml/badge.svg)](https://github.com/cthulhutoo/limtop/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

![limtop dashboard](assets/rate-hero.png?v=4)

**btop for AI usage.** A single Rust binary that reads the CLI coding agents
you already use — Claude Code, opencode, Hermes — and shows tokens, cost,
cache behavior, and burn-rate in one terminal dashboard. No server, no
account, no telemetry: everything is read from local files on disk.

> Formerly **aitop** — renamed after a crates.io name collision.

## Why

Every AI usage tracker in 2026 is a menu-bar app or a web dashboard. If you
live in a terminal over SSH — on a server, in tmux, in a container — there's
nothing. `limtop` is the missing one: `cargo install limtop`, run it anywhere,
see everything.

## Screenshots

| | |
|---|---|
| ![dashboard](assets/rate-hero.png?v=4) | ![projects](assets/projects-list.png?v=4) |
| ![project detail](assets/project-detail.png?v=4) | ![rate window](assets/rate-window.png?v=4) |

## Install

    # Homebrew (macOS / Linuxbrew)
    brew install cthulhutoo/limtop/limtop

    # Cargo
    cargo install limtop

    # From source
    git clone https://github.com/cthulhutoo/limtop && cd limtop && cargo install --path .

## Providers

| Provider | Source | Status |
|----------|--------|--------|
| Claude Code | `~/.claude/projects/**/*.jsonl` | working |
| Codex | `~/.codex/` (sqlite + rollout JSONL) | working |
| Gemini CLI | `~/.gemini/tmp/*/chats/*.jsonl` | working |
| Hermes Agent | `~/.hermes/state.db` | working |
| opencode | `~/.local/share/opencode/storage/` | working |
| Ollama | local API + journald | working (call counts) |

limtop auto-discovers providers at startup and shows which ones it found.

## Rate-limit window

The dashboard shows a derived Claude 5-hour usage window: weighted tokens
consumed in the trailing 5h, burn rate, and when the oldest usage ages out.
Cache reads count at 0.1× weight, matching how limits bite.

Claude does not publish plan limits, so presets are community-observed
approximations. Pick yours with:

    LIMTOP_CLAUDE_LIMIT=pro|max5|max20|custom:<tokens>   limtop

Defaults to `pro`. The panel is labeled "derived" — it's a local estimate,
not an official reading.

## Usage

    limtop              # TUI dashboard
    limtop --dump       # plain-text report (pipes, CI, cron)
    limtop --dump --all # all-time report

In the TUI:

    1–5      switch span (1h / 24h / 7d / 30d / all)
    p        project drill-down (↑/↓ select, enter detail, backspace back)
    r        rescan providers
    q        quit

## Headless & scripting

    limtop --json            # one-shot JSON snapshot — never launches the TUI
    limtop --once            # same, plain text (alias for --dump)
    limtop --watch           # refresh every 5s — Ctrl-C to quit
    limtop --watch=10        # ...every 10s (`--watch 10` also works; 1–3600s)
    limtop --json --watch=5  # streaming JSON

`--json` implies one-shot, so bare `limtop --json` drops straight into
scripts. Keys are camelCase (`totals.inputTokens`, `window.burnRate`, ...);
`window` is `null` when no Claude rate window is active. Piped `--json`
output is byte-clean — no ANSI escapes — so line-oriented tools just work:

    limtop --json | jq '.totals.cost'
    limtop --json | jq '.window.usedTokens // 0'
    limtop --json --watch=10 | jq --unbuffered -c '{cost: .totals.cost}'

The last one is a ready-made status-bar module (waybar, i3blocks, a spare
tmux pane). `limtop --dump --watch` streams the text report the same way,
like `watch limtop --dump` without the rerun. Unknown flags warn on stderr
and are otherwise ignored.

## Configuration

Zero config is the default; every key is optional. `~/.config/limtop.toml`:

    # Claude 5h window limit: pro | max5 | max20 | custom:N (or plain tokens)
    plan = "max20"
    ollama_url = "http://nas:11434"             # ollama API endpoint
    extra_claude_dirs = ["~/work/claude-alt"]   # each scanned for projects/

    [pricing."glm-4.7"]   # USD per Mtok — patches the builtin pricing table
    input = 3.0
    output = 12.0

Precedence is env > file > defaults: `LIMTOP_CLAUDE_LIMIT` and
`LIMTOP_OLLAMA_URL` always win over their toml counterparts.

## Cost estimates

Costs are computed from a static pricing table (USD per million tokens,
cache reads at 0.1× input, cache writes at 1.25× input). When a provider
reports cost directly, that number is used instead. Local models (Ollama,
LM Studio) show $0.

## Privacy

limtop never makes a network request on your behalf. All data comes from
local session files and provider state on your machine. There is no
analytics and no phone-home; the optional config file is read locally and
never leaves your disk.

## Roadmap

See [ROADMAP.md](ROADMAP.md) — v0.3.x ships machine-readable output +
config + release binaries; v0.4 makes cost analytics the headline:
invoice projection, weekly reports, per-model window buckets.

## License

MIT
