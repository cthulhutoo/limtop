mod aggregator;
mod model;
mod providers;
mod rate_window;
mod snapshot;
mod tui;

use aggregator::Dashboard;
use model::Span;
use providers::Registry;
use std::io;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn main() {
    // ── CLI args ───────────────────────────────────────────────────
    let mut dump = false;
    let mut json = false;
    let mut span = Span::Day;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--dump" => dump = true,
            "--once" => dump = true, // alias: one-shot, no TUI
            "--json" => json = true,
            "--all" => span = Span::All,
            "--week" => span = Span::Week,
            "--month" => span = Span::Month,
            "--hour" => span = Span::Hour,
            other if other.starts_with("--") => {
                eprintln!("limtop: unknown flag {other} (ignored)")
            }
            _ => {}
        }
    }

    // --json implies one-shot: a bare `limtop --json` must print the
    // snapshot and exit, never fall through into the (headless-hostile) TUI.
    let dump = dump || json;

    let home = dirs::home_dir().expect("cannot resolve $HOME");
    let reg = Registry::scan(&home);

    if dump && json {
        dump_json(&reg, span);
        return;
    }
    if dump {
        dump_report(&reg, span);
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
            last_scan = Some((Registry::scan(&home), now_epoch()));
            need_rescan = false;
        }
        let (reg, _) = last_scan.as_ref().expect("scan exists");
        let dash = Dashboard::build(reg.events.clone(), span, now_epoch());
        let window = rate_window::RateWindow::build(&reg.events, now_epoch());

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

fn dump_json(reg: &Registry, span: Span) {
    let now = now_epoch();
    let dash = Dashboard::build(reg.events.clone(), span, now);
    let window = rate_window::RateWindow::build(&reg.events, now);
    let snap = snapshot::Snapshot::build(reg, &dash, window, now);
    println!(
        "{}",
        serde_json::to_string_pretty(&snap).expect("snapshot serializes")
    );
}

fn dump_report(reg: &Registry, span: Span) {
    let dash = Dashboard::build(reg.events.clone(), span, now_epoch());
    let window = rate_window::RateWindow::build(&reg.events, now_epoch());
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
