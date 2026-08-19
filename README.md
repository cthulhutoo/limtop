# aitop

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ aitop   37.2M tok  $77.18   in 27.2M out 2.0M cache 7.3Mr/734.2kw   822 calls │
└──────────────────────────────────────────────────────────────────────────────┘
┌ cost burn · peak $60.04 ─────────────────────────────────────────────────────┐
│         ██                                                                   │
│         ██                                                                   │
│         ██ ▄▄                                                                │
│▄▄ ▅▅    ██ ▆▆ ▁▁                                                             │
└──────────────────────────────────────────────────────────────────────────────┘
┌ projects ────────────────────────────────────────┐┌ models ─────────────────┐
│ ~/projects/lantrn    7.7M  ████████████  $4.93   ││ glm-5.2      9.5M  $6.00 │
│ ~/projects/aitop   100.3k  ············  $0.09   ││ opus-4-8     2.7M  $58.35│
└──────────────────────────────────────────────────┘└──────────────────────────┘
┌ claude-code ─────┐┌ hermes ──────────┐┌ opencode ───────┐
│ ● detected  $5.12││ ● detected  $68  ││ ● detected  $4.06│
└──────────────────┘└──────────────────┘└──────────────────┘
```

**btop for AI usage.** A single Rust binary that reads the CLI coding agents
you already use — Claude Code, opencode, Hermes — and shows tokens, cost,
cache behavior, and burn-rate in one terminal dashboard. No server, no
account, no telemetry: everything is read from local files on disk.

## Why

Every AI usage tracker in 2026 is a menu-bar app or a web dashboard. If you
live in a terminal over SSH — on a server, in tmux, in a container — there's
nothing. `aitop` is the missing one: `cargo install aitop`, run it anywhere,
see everything.

## Install

    cargo install --path .
    # or from source
    git clone https://github.com/rdclark/aitop && cd aitop && cargo install --path .

## Providers

| Provider | Source | Status |
|----------|--------|--------|
| Claude Code | `~/.claude/projects/**/*.jsonl` | working |
| Codex | `~/.codex/` (sqlite + rollout JSONL) | working |
| Gemini CLI | `~/.gemini/tmp/*/chats/*.jsonl` | working |
| Hermes Agent | `~/.hermes/state.db` | working |
| opencode | `~/.local/share/opencode/storage/` | working |
| Ollama | local API | planned |

aitop auto-discovers providers at startup and shows which ones it found.

## Rate-limit window

The dashboard shows a derived Claude 5-hour usage window: weighted tokens
consumed in the trailing 5h, burn rate, and when the oldest usage ages out.
Cache reads count at 0.1× weight, matching how limits bite.

Claude does not publish plan limits, so presets are community-observed
approximations. Pick yours with:

    AITOP_CLAUDE_LIMIT=pro|max5|max20|custom:<tokens>   aitop

Defaults to `pro`. The panel is labeled "derived" — it's a local estimate,
not an official reading.

## Usage

    aitop              # TUI dashboard
    aitop --dump       # plain-text report (pipes, CI, cron)
    aitop --dump --all # all-time report

In the TUI: `1`–`5` switch span (1h / 24h / 7d / 30d / all), `r` rescan,
`q` quit.

## Cost estimates

Costs are computed from a static pricing table (USD per million tokens,
cache reads at 0.1× input, cache writes at 1.25× input). When a provider
reports cost directly, that number is used instead. Local models (Ollama,
LM Studio) show $0.

## Privacy

aitop never makes a network request. All data comes from local session
files and provider state on your machine. There is no analytics, no
phone-home, no config file.

## License

MIT
