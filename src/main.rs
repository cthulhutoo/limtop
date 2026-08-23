mod aggregator;
mod config;
mod model;
mod providers;
mod rate_window;
mod snapshot;
mod tui;

use aggregator::Dashboard;
use model::Span;
use providers::Registry;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Default `--watch` refresh interval, in seconds.
const WATCH_DEFAULT_SECS: u64 = 5;
/// `--watch` interval is clamped to 1s ..= 1h.
const WATCH_MIN_SECS: u64 = 1;
const WATCH_MAX_SECS: u64 = 3600;

/// Extract the `--watch` flag and its interval from the CLI args.
///
/// Accepted forms:
///   `--watch`   → enabled, default interval
///   `--watch=N` → enabled, interval N seconds
///   `--watch N` → enabled, interval N seconds (N must parse as u64)
///
/// The interval is clamped to `WATCH_MIN_SECS..=WATCH_MAX_SECS`; values
/// that don't parse as a number fall back to the default. Returns
/// `(watch_enabled, seconds)`.
fn parse_watch_interval(args: &[String]) -> (bool, u64) {
    let mut watch = false;
    let mut secs: Option<u64> = None;
    for (i, arg) in args.iter().enumerate() {
        if arg == "--watch" {
            watch = true;
            // following-value form: `--watch 10`
            if let Some(next) = args.get(i + 1) {
                if let Ok(n) = next.parse::<u64>() {
                    secs = Some(n);
                }
            }
        } else if let Some(v) = arg.strip_prefix("--watch=") {
            watch = true;
            match v.parse::<u64>() {
                Ok(n) => secs = Some(n),
                Err(_) => eprintln!("limtop: bad --watch interval {v:?} (using 5s)"),
            }
        }
    }
    (
        watch,
        secs.unwrap_or(WATCH_DEFAULT_SECS)
            .clamp(WATCH_MIN_SECS, WATCH_MAX_SECS),
    )
}

fn main() {
    // ── CLI args ───────────────────────────────────────────────────
    // Args are collected into a Vec so `--watch N` can peek at the
    // following value, and so parse_watch_interval can scan the whole
    // command line.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dump = false;
    let mut json = false;
    let mut span = Span::Day;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dump" => dump = true,
            "--once" => dump = true, // alias: one-shot, no TUI
            "--json" => json = true,
            // The --watch flag and its interval are parsed wholesale by
            // parse_watch_interval below; here we only make sure the flag
            // (and a following numeric interval) doesn't trip the
            // unknown-flag warning below.
            "--watch" => {
                if args.get(i + 1).is_some_and(|v| v.parse::<u64>().is_ok()) {
                    i += 1; // consume the interval value of `--watch N`
                }
            }
            other if other.starts_with("--watch=") => {} // `--watch=N`
            "--all" => span = Span::All,
            "--week" => span = Span::Week,
            "--month" => span = Span::Month,
            "--hour" => span = Span::Hour,
            other if other.starts_with("--") => {
                eprintln!("limtop: unknown flag {other} (ignored)")
            }
            _ => {}
        }
        i += 1;
    }

    let (watch, interval_secs) = parse_watch_interval(&args);

    // --json implies one-shot: a bare `limtop --json` must print the
    // snapshot and exit, never fall through into the (headless-hostile) TUI.
    // --watch implies dump too: a bare `limtop --watch` streams the text
    // report.
    let dump = dump || json || watch;

    let home = dirs::home_dir().expect("cannot resolve $HOME");

    // ── config (~/.config/limtop.toml, all-optional) ────────────────
    let mut cfg = config::Config::load();
    let plan = cfg.plan_or_env();
    // pricing overrides go into a global the cost estimator consults; set
    // once here (OnceLock: first write wins, scans re-read it for free)
    model::set_pricing_overrides(cfg.pricing.take().unwrap_or_default());

    // ── streaming mode: --dump/--json + --watch ────────────────────
    if dump && watch {
        // Redraw a fresh snapshot every `interval_secs` seconds. Exit is
        // Ctrl-C: we never enter raw mode, so relying on the kernel's
        // default SIGINT termination is deliberate — the reporter is
        // read-only and every frame is fully flushed before sleeping, so
        // dying mid-loop leaves nothing to clean up.
        let sleep_for = Duration::from_secs(interval_secs);
        // Only emit the ANSI clear when stdout is a terminal: piped or
        // redirected output must stay byte-clean (zero escape bytes) so
        // `limtop --watch --json > log` remains parseable.
        let interactive = std::io::stdout().is_terminal();
        loop {
            // Scan FIRST: the previous frame stays visible while the
            // (possibly slow) filesystem scan runs — no blank window.
            let reg = Registry::scan(&home, &cfg); // fresh data every cycle
            if interactive {
                print!("\x1b[2J\x1b[H"); // clear screen, home cursor
                io::stdout().flush().ok();
            }
            if json {
                dump_json(&reg, span, plan.as_deref());
            } else {
                dump_report(&reg, span, plan.as_deref());
            }
            // Explicit flush: stdout is block-buffered when redirected to
            // a file, and the process may be SIGTERM/SIGINT-killed during
            // the sleep — each frame must land before then.
            io::stdout().flush().ok();
            std::thread::sleep(sleep_for);
        }
    }

    let reg = Registry::scan(&home, &cfg);

    if dump && json {
        dump_json(&reg, span, plan.as_deref());
        return;
    }
    if dump {
        dump_report(&reg, span, plan.as_deref());
        return;
    }

    // ── TUI loop ───────────────────────────────────────────────────
    let mut terminal = ratatui::init();
    let mut span = span;
    let mut last_scan: Option<(Registry, i64)> = None;
    let mut need_rescan = true;

    // drill-down state (project list / detail), 'p' toggles
    let mut drill: Option<tui::drilldown::Drilldown> = None;

    let tick_rate = Duration::from_millis(1000);
    let mut last_tick = std::time::Instant::now();

    let res = io::stdin();
    let _ = res;

    loop {
        // rescan every 30s or on demand
        let fresh = match &last_scan {
            Some((_, at)) => now_epoch() - at > 30,
            None => true,
        };
        if need_rescan || fresh {
            last_scan = Some((Registry::scan(&home, &cfg), now_epoch()));
            need_rescan = false;
        }
        let (reg, _) = last_scan.as_ref().expect("scan exists");
        let dash = Dashboard::build(reg.events.clone(), span, now_epoch());
        let window = rate_window::RateWindow::build(&reg.events, now_epoch(), plan.as_deref());

        match drill.as_mut() {
            None => {
                terminal
                    .draw(|f| tui::render(f, &dash, &reg.statuses, window.as_ref()))
                    .expect("draw failed");
            }
            Some(dd) => {
                dd.refresh_detail(&dash);
                terminal
                    .draw(|f| tui::drilldown::render(f, dd, &dash.span_label().to_string()))
                    .expect("draw failed");
            }
        }

        // input with timeout
        if crossterm::event::poll(tick_rate).expect("poll failed") {
            if let crossterm::event::Event::Key(k) = crossterm::event::read().expect("read failed")
            {
                if k.kind == crossterm::event::KeyEventKind::Press {
                    match k.code {
                        crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc => {
                            break
                        }
                        crossterm::event::KeyCode::Char('r') => {
                            need_rescan = true;
                        }
                        crossterm::event::KeyCode::Char('p') => {
                            if drill.is_none() {
                                drill = Some(tui::drilldown::Drilldown::new(&dash));
                            } else {
                                drill = None; // toggle back
                            }
                        }
                        crossterm::event::KeyCode::Up => {
                            if let Some(dd) = drill.as_mut() {
                                dd.up();
                            }
                        }
                        crossterm::event::KeyCode::Down => {
                            if let Some(dd) = drill.as_mut() {
                                dd.down();
                            }
                        }
                        crossterm::event::KeyCode::Enter => {
                            if let Some(dd) = drill.as_mut() {
                                dd.enter();
                            }
                        }
                        crossterm::event::KeyCode::Backspace => {
                            if let Some(dd) = drill.as_mut() {
                                dd.back();
                            }
                        }
                        crossterm::event::KeyCode::Char('1') => span = Span::Hour,
                        crossterm::event::KeyCode::Char('2') => span = Span::Day,
                        crossterm::event::KeyCode::Char('3') => span = Span::Week,
                        crossterm::event::KeyCode::Char('4') => span = Span::Month,
                        crossterm::event::KeyCode::Char('5') => span = Span::All,
                        _ => {}
                    }
                }
            }
        }
        let _ = last_tick;
    }

    ratatui::restore();
}

fn dump_json(reg: &Registry, span: Span, plan: Option<&str>) {
    let now = now_epoch();
    let dash = Dashboard::build(reg.events.clone(), span, now);
    let window = rate_window::RateWindow::build(&reg.events, now, plan);
    let snap = snapshot::Snapshot::build(reg, &dash, window, now);
    println!(
        "{}",
        serde_json::to_string_pretty(&snap).expect("snapshot serializes")
    );
}

fn dump_report(reg: &Registry, span: Span, plan: Option<&str>) {
    let dash = Dashboard::build(reg.events.clone(), span, now_epoch());
    let window = rate_window::RateWindow::build(&reg.events, now_epoch(), plan);
    println!("limtop — AI usage report (span: {})", dash.span);
    println!();
    if let Some(w) = &window {
        println!("{}", rate_window::fmt_window(&w));
        println!();
    }
    println!("providers detected:");
    for s in &reg.statuses {
        println!(
            "  {} {} — {}",
            if s.detected { "●" } else { "○" },
            s.name,
            s.detail
        );
    }
    println!();
    println!("totals: {}", summarize(&dash.totals));
    println!();
    println!("by provider:");
    for (name, t) in dash.by_provider.iter().take(8) {
        println!("  {:<14} {}", name, summarize(t));
    }
    println!();
    println!("by model:");
    for (name, t) in dash.by_model.iter().take(10) {
        println!("  {:<34} {}", name, summarize(t));
    }
    println!();
    println!("by project:");
    for (name, t) in dash.by_project.iter().take(10) {
        println!("  {:<34} {}", name, summarize(t));
    }
}

fn summarize(t: &model::UsageTotals) -> String {
    format!(
        "{:>8} tok  {:>8} in {:>8} out  {}  ({} calls)",
        model::fmt_tokens(t.total_tokens()),
        model::fmt_tokens(t.input_tokens),
        model::fmt_tokens(t.output_tokens),
        model::fmt_cost(t.cost),
        t.events
    )
}

#[cfg(test)]
mod tests {
    use super::parse_watch_interval;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn watch_interval_parsing() {
        // no flag → disabled, default interval
        assert_eq!(parse_watch_interval(&args(&[])), (false, 5));
        assert_eq!(
            parse_watch_interval(&args(&["--dump", "--all"])),
            (false, 5)
        );
        // bare --watch → enabled, default interval
        assert_eq!(parse_watch_interval(&args(&["--watch"])), (true, 5));
        // --watch=N
        assert_eq!(parse_watch_interval(&args(&["--watch=10"])), (true, 10));
        // --watch N (following-value form)
        assert_eq!(parse_watch_interval(&args(&["--watch", "10"])), (true, 10));
        // flags around it don't disturb the interval
        assert_eq!(
            parse_watch_interval(&args(&["--dump", "--watch=3", "--json"])),
            (true, 3)
        );
        // clamp: below minimum → 1
        assert_eq!(parse_watch_interval(&args(&["--watch=0"])), (true, 1));
        assert_eq!(parse_watch_interval(&args(&["--watch", "0"])), (true, 1));
        // clamp: above maximum → 3600
        assert_eq!(
            parse_watch_interval(&args(&["--watch=99999"])),
            (true, 3600)
        );
        // garbage interval falls back to default
        assert_eq!(parse_watch_interval(&args(&["--watch=abc"])), (true, 5));
        // bare --watch followed by a flag: the flag is NOT consumed as
        // an interval (only a numeric next arg counts)
        assert_eq!(
            parse_watch_interval(&args(&["--watch", "--json"])),
            (true, 5)
        );
        // repeated --watch: the last value wins
        assert_eq!(
            parse_watch_interval(&args(&["--watch=5", "--watch=7"])),
            (true, 7)
        );
        // negative value fails u64 parsing → falls back to default 5,
        // NOT clamped to the 1s minimum (fallback ≠ clamp)
        assert_eq!(parse_watch_interval(&args(&["--watch=-5"])), (true, 5));
    }
}
