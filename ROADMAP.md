# limtop roadmap

Where limtop is headed. Milestones are loose — star the repo if you want
something moved up. [Issues](https://github.com/cthulhutoo/limtop/issues)
are the right place to +1 or argue priorities.

## Now — v0.3.x (CLI-agent core polish)

- [ ] `limtop --dump --watch` — stream the report like `watch limtop --dump`
- [ ] `limtop --json` — machine-readable dump for scripts / status bars
- [ ] Session start/end tracking — answer "which project was I burning in at 2pm?"
- [ ] macOS + Linux release binaries on tag push (brew installs from source today)
- [ ] Stay: zero-config discovery · no network calls · single static binary

## Next — v0.4 (Claude window, deeper)

- [ ] Per-model buckets inside the 5h window — see which model is eating the limit
- [ ] Config file `~/.config/limtop.toml` — custom plan limits, pricing overrides
- [ ] `--dump --metrics` — Prometheus text export so you can scrape yourself
- [ ] Threshold alert: terminal bell / desktop notify when projected exhaustion
      crosses a horizon you set

## Later — v0.5+ (breadth)

- [ ] Cursor sessions (`~/.cursor` deep storage)
- [ ] LM Studio server logs
- [ ] Windows support (Claude Code on Windows stores paths differently)
- [ ] Session timeline view — scroll through time, find the spike

## Someday / maybe

- [ ] `limtop serve` headless mode? Leaning no — SSH + tmux is the story.
      Convince me in an issue.
- [ ] Pricing table as an external updatable file instead of baked into the binary
- [ ] Named sessions — tag a session, find it later

---

Release history: [github.com/cthulhutoo/limtop/releases](https://github.com/cthulhutoo/limtop/releases)
