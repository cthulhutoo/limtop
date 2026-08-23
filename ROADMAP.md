# limtop roadmap

Where limtop is headed. Milestones are loose — star the repo if you want
something moved up. [Issues](https://github.com/cthulhutoo/limtop/issues)
are the right place to +1 or argue priorities.

limtop's lane: the **flight recorder** for AI usage — what you burned, what
it cost, and when your window resets. Not a live process monitor; the
history and money layer other tools skip.

## Now — v0.3.x (foundations)

**Parity basics every CLI tool deserves**

- [ ] `limtop --json` / `--once` — one-shot machine-readable snapshot for
      scripts, cron, waybar/status-bar modules
- [ ] `limtop --dump --watch` — stream the report like `watch limtop --dump`
- [ ] Config file `~/.config/limtop.toml` — theme, plan limits, pricing
      overrides, extra profile roots (`~/.claude-work`, `~/.claude-personal`)
- [ ] Release binaries on tag push (macOS + Linux, preferably an installer
      script) — brew installs compile from source today
- [ ] Stay: zero-config discovery · no setup hooks · no quota-spending calls ·
      single static binary

## Next — v0.4 (the money view)

**Cost analytics as the headline — the thing nobody else does**

- [ ] Per-project cost over time + monthly invoice projection
      ("at this burn, you end the month at $N")
- [ ] `limtop --report weekly` — markdown digest: burn by project/model,
      week-over-week delta, window close calls
- [ ] Per-model buckets inside the Claude 5h window — *which* model is
      eating the limit
- [ ] Threshold alerts — terminal bell / desktop notify when projected
      exhaustion crosses a horizon you set
- [ ] Themes (press `t`): tokyo-night default, plus gruvbox, nord, and
      colorblind-friendly variants

## Later — v0.5+ (breadth + depth)

- [ ] Cursor sessions (`~/.cursor` deep storage)
- [ ] LM Studio server logs
- [ ] Live/idle badges on projects (● active in the last N min / ○ idle) —
      history view stays the point, but "is it running" answers itself
- [ ] Session timeline view — scroll through time, find the spike,
      answer "what was I burning at 2pm?"
- [ ] Windows support (Claude Code on Windows stores paths differently)
- [ ] `--dump --metrics` — Prometheus text export so you can scrape yourself
- [ ] Session start/end tracking — reconstruct "which project was I in?"

## Someday / maybe

- [ ] `limtop serve` headless mode? Leaning no — SSH + tmux is the story.
      Convince me in an issue.
- [ ] Pricing table as an external updatable file instead of baked into the binary
- [ ] Named sessions — tag a session, find it later
- [ ] Library crate — expose the collector layer so local tools can reuse
      limtop's scan in-process

---

Release history: [github.com/cthulhutoo/limtop/releases](https://github.com/cthulhutoo/limtop/releases)
