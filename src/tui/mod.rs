use crate::aggregator::Dashboard;
use crate::model::{fmt_cost, fmt_tokens, ProviderStatus, UsageTotals};
use crate::rate_window::RateWindow;
pub mod drilldown;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as RSpan},
    widgets::block::Title,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Gauge, Paragraph},
    Frame,
};

// ---------------------------------------------------------------------------
// Palette — indexed 256 colors, soft pastels (Tokyo-Night inspired).
// Much easier on the eyes than pure Cyan/Green/Yellow/Red on black.
// ---------------------------------------------------------------------------
const ACCENT: Color = Color::Indexed(81); // #5fd7ff sky — keys, names, tokens
const COST: Color = Color::Indexed(114); // #87ff87 soft green — money
const WARN: Color = Color::Indexed(222); // #ffd787 warm yellow — time/plans
const RED: Color = Color::Indexed(210); // #ff8787 soft red — danger
const DIM: Color = Color::Indexed(245); // #8a8a8a readable gray
const FAINT: Color = Color::Indexed(240); // borders
const HEAD: Color = Color::Indexed(250); // near-white labels

fn block(title: Line<'_>) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(FAINT))
        .title(title)
}

/// Render the full dashboard.
pub fn render(
    f: &mut Frame,
    d: &Dashboard,
    statuses: &[ProviderStatus],
    window: Option<&RateWindow>,
) {
    let mut rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // header (2 content lines)
            Constraint::Length(6),  // claude rate window (when present)
            Constraint::Length(10), // burn graph
            Constraint::Min(8),     // middle: projects + models
            Constraint::Length(4),  // providers strip (2 content lines)
            Constraint::Length(1),  // footer
        ])
        .split(f.area());

    render_header(f, rows[0], d);
    if let Some(w) = window {
        render_window(f, rows[1], w);
        render_burn(f, rows[2], d);
    } else {
        // no window: stretch burn chart over its space + graph's
        let merged = Rect {
            x: rows[1].x,
            y: rows[1].y,
            width: rows[1].width,
            height: rows[1].height + rows[2].height,
        };
        render_burn(f, merged, d);
    }
    render_middle(f, rows[3], d);
    render_providers(f, rows[4], statuses, d);
    render_footer(f, rows[5]);
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Header — two lines: primary metrics, then secondary detail
// ---------------------------------------------------------------------------
fn render_header(f: &mut Frame, area: Rect, d: &Dashboard) {
    let t = &d.totals;
    let line1 = Line::from(vec![
        RSpan::styled(
            " limtop ",
            Style::default()
                .fg(Color::Indexed(17))
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        RSpan::raw("  "),
        RSpan::styled(
            fmt_tokens(t.total_tokens()),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled(" tok  ", Style::default().fg(DIM)),
        RSpan::styled(
            fmt_cost(t.cost),
            Style::default().fg(COST).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled("  ", Style::default()),
        RSpan::styled(format!("{} calls", t.events), Style::default().fg(DIM)),
        RSpan::styled("   ·   ", Style::default().fg(FAINT)),
        RSpan::styled("span ", Style::default().fg(DIM)),
        RSpan::styled(
            d.span.to_string(),
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
    ]);
    let line2 = Line::from(vec![
        RSpan::raw("        "),
        RSpan::styled("in ", Style::default().fg(DIM)),
        RSpan::styled(fmt_tokens(t.input_tokens), Style::default().fg(HEAD)),
        RSpan::styled("   out ", Style::default().fg(DIM)),
        RSpan::styled(fmt_tokens(t.output_tokens), Style::default().fg(HEAD)),
        RSpan::styled("   cache ", Style::default().fg(DIM)),
        RSpan::styled(
            format!(
                "{}r / {}w",
                fmt_tokens(t.cache_read_tokens),
                fmt_tokens(t.cache_write_tokens)
            ),
            Style::default().fg(HEAD),
        ),
    ]);
    let p = Paragraph::new(vec![line1, line2]).block(block(Line::from("")));
    f.render_widget(p, area);
}

// ---------------------------------------------------------------------------
// Claude 5h window — info line + unicode gauge, no mega-label
// ---------------------------------------------------------------------------
fn render_window(f: &mut Frame, area: Rect, w: &RateWindow) {
    let pct = (w.pct_used() * 100.0) as u16;
    let color = if pct >= 90 {
        RED
    } else if pct >= 70 {
        WARN
    } else {
        COST
    };
    let resets_in = (w.resets_at - now_epoch()).max(0);
    let (rh, rm) = (resets_in / 3600, (resets_in % 3600) / 60);

    let title = Line::from(vec![
        RSpan::styled(
            " claude 5h window ",
            Style::default().fg(HEAD).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled(" derived · not official ", Style::default().fg(FAINT)),
    ]);
    let right = Line::from(vec![
        RSpan::styled("resets in ", Style::default().fg(DIM)),
        RSpan::styled(
            format!("{}h {:02}m", rh, rm),
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled("  ·  plan ", Style::default().fg(DIM)),
        RSpan::styled(w.limit_name().to_string(), Style::default().fg(ACCENT)),
    ]);

    let b = block(title).title(Title::from(right).alignment(Alignment::Right));
    f.render_widget(&b, area); // draw border + titles
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(2)])
        .split(b.inner(area));

    let info = Line::from(vec![
        RSpan::styled("  ", Style::default()),
        RSpan::styled(
            fmt_tokens(w.used as u64),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled(" / ", Style::default().fg(DIM)),
        RSpan::styled(
            format!("{} weighted tok", fmt_tokens(w.limit as u64)),
            Style::default().fg(DIM),
        ),
        RSpan::styled(
            format!("  ({}%)", pct),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled("    burn ", Style::default().fg(DIM)),
        RSpan::styled(
            format!("{}/h", fmt_tokens(w.burn_rate as u64)),
            Style::default().fg(ACCENT),
        ),
    ]);
    f.render_widget(Paragraph::new(info), inner[0]);

    let gauge = Gauge::default()
        .ratio(w.pct_used().min(1.0))
        .label(format!(" {}% of window used ", pct))
        .use_unicode(true)
        .gauge_style(Style::default().fg(color).bg(Color::Indexed(234)));
    f.render_widget(gauge, inner[1]);
}

// ---------------------------------------------------------------------------
// Cost-over-time bar chart
// ---------------------------------------------------------------------------
fn render_burn(f: &mut Frame, area: Rect, d: &Dashboard) {
    let bars: Vec<Bar> = d
        .burn
        .iter()
        .map(|(_, c)| {
            Bar::default()
                .value((*c * 1000.0) as u64) // millidollars for resolution
                .style(Style::default().fg(ACCENT))
                .value_style(Style::default().fg(ACCENT))
        })
        .collect();
    let max_cost = d.burn.iter().map(|(_, c)| *c).fold(0.0f64, f64::max);
    let title = Line::from(vec![
        RSpan::styled(
            " cost burn ",
            Style::default().fg(HEAD).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled(
            format!("· peak {} · {} ", fmt_cost(max_cost), d.span),
            Style::default().fg(DIM),
        ),
    ]);
    let chart = BarChart::default()
        .block(block(title))
        .data(BarGroup::default().bars(&bars))
        .bar_width(2)
        .bar_gap(1)
        .max(
            d.burn
                .iter()
                .map(|(_, c)| (*c * 1000.0) as u64)
                .fold(0, u64::max),
        );
    f.render_widget(chart, area);
}

// ---------------------------------------------------------------------------
// Middle: projects + models tables
// ---------------------------------------------------------------------------
fn render_middle(f: &mut Frame, area: Rect, d: &Dashboard) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)])
        .split(area);
    render_table(f, cols[0], "projects", &d.by_project);
    render_table(f, cols[1], "models", &d.by_model);
}

fn elide(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else {
        let cut: String = s.chars().take(w.saturating_sub(1)).collect();
        format!("{}…", cut)
    }
}

fn render_table(f: &mut Frame, area: Rect, title: &str, rows: &[(String, UsageTotals)]) {
    let max_cost = rows.iter().map(|(_, t)| t.cost).fold(0.0f64, f64::max);
    let inner_w = area.width.saturating_sub(2) as usize;
    // tokens(8) gap(2) bar(10) gap(1) cost(9) + lead(1)
    let fixed = 31;
    let name_w = inner_w.saturating_sub(fixed).clamp(8, 26);
    let bar_w = 10;

    let mut lines = vec![Line::from(vec![
        RSpan::styled(
            format!(" {:<width$}", "name", width = name_w),
            Style::default().fg(DIM),
        ),
        RSpan::styled(format!("{:>8}  ", "tokens"), Style::default().fg(DIM)),
        RSpan::styled(" ".repeat(bar_w), Style::default()),
        RSpan::styled(format!("{:>9}", "cost"), Style::default().fg(DIM)),
    ])];

    let capacity = area.height.saturating_sub(3) as usize; // borders + header
    for (name, tot) in rows.iter().take(capacity) {
        let frac = if max_cost > 0.0 {
            tot.cost / max_cost
        } else {
            0.0
        };
        let filled = (frac * bar_w as f64).round() as usize;
        let bar =
            "█".repeat(filled.min(bar_w)) + &"·".repeat(bar_w.saturating_sub(filled.min(bar_w)));
        lines.push(Line::from(vec![
            RSpan::styled(
                format!(" {:<width$}", elide(name, name_w), width = name_w),
                Style::default().fg(HEAD),
            ),
            RSpan::styled(
                format!("{:>8}", fmt_tokens(tot.total_tokens())),
                Style::default().fg(ACCENT),
            ),
            RSpan::raw("  "),
            RSpan::styled(bar, Style::default().fg(COST)),
            RSpan::raw(" "),
            RSpan::styled(
                format!("{:>9}", fmt_cost(tot.cost)),
                Style::default().fg(COST),
            ),
        ]));
    }
    let count = rows.len();
    let t = Line::from(vec![
        RSpan::styled(
            format!(" {} ", title),
            Style::default().fg(HEAD).add_modifier(Modifier::BOLD),
        ),
        RSpan::styled(format!("({}) ", count), Style::default().fg(DIM)),
    ]);
    let p = Paragraph::new(lines).block(block(t));
    f.render_widget(p, area);
}

// ---------------------------------------------------------------------------
// Provider strip — two compact lines each
// ---------------------------------------------------------------------------
fn render_providers(f: &mut Frame, area: Rect, statuses: &[ProviderStatus], d: &Dashboard) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Ratio(1, statuses.len().max(1) as u32);
            statuses.len().max(1)
        ])
        .split(area);

    for (i, st) in statuses.iter().enumerate() {
        let tot = d.by_provider.iter().find(|(name, _)| name == &st.name);
        let (tokens, cost) = tot
            .map(|(_, t)| (fmt_tokens(t.total_tokens()), fmt_cost(t.cost)))
            .unwrap_or_else(|| ("0".into(), "$0".into()));
        let on = st.detected;
        let line1 = Line::from(vec![
            RSpan::styled(" ● ", Style::default().fg(if on { COST } else { FAINT })),
            RSpan::styled(
                st.name.clone(),
                Style::default()
                    .fg(if on { ACCENT } else { DIM })
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let line2 = Line::from(vec![
            RSpan::raw("   "),
            RSpan::styled(
                format!("{} tok", tokens),
                Style::default().fg(if on { HEAD } else { DIM }),
            ),
            RSpan::styled("  ·  ", Style::default().fg(FAINT)),
            RSpan::styled(cost, Style::default().fg(if on { COST } else { DIM })),
        ]);
        let p = Paragraph::new(vec![line1, line2]).block(block(Line::from("")));
        f.render_widget(p, cols[i]);
    }
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------
fn render_footer(f: &mut Frame, area: Rect) {
    fn key(k: &str) -> RSpan<'static> {
        RSpan::styled(k.to_string(), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
    }
    fn lbl(l: &str) -> RSpan<'static> {
        RSpan::styled(l.to_string(), Style::default().fg(DIM))
    }
    let sep = RSpan::styled("   ", Style::default());
    let help = Line::from(vec![
        key(" 1-5"),
        lbl(" span"),
        sep.clone(),
        key("p"),
        lbl(" projects"),
        sep.clone(),
        key("r"),
        lbl(" rescan"),
        sep,
        key("q"),
        lbl(" quit"),
    ]);
    f.render_widget(Paragraph::new(help), area);
}
