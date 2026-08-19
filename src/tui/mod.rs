use crate::aggregator::Dashboard;
use crate::model::{fmt_cost, fmt_tokens, ProviderStatus, UsageTotals};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as RSpan},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph},
    Frame,
};

const ACCENT: Color = Color::Cyan;
const GREEN: Color = Color::Green;
const DIM: Color = Color::DarkGray;

/// Render the full dashboard.
pub fn render(f: &mut Frame, d: &Dashboard, statuses: &[ProviderStatus]) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Length(10), // burn graph
            Constraint::Min(8),     // middle: projects + models
            Constraint::Length(7),  // providers strip
            Constraint::Length(1),  // footer
        ])
        .split(f.area());

    render_header(f, rows[0], d);
    render_burn(f, rows[1], d);
    render_middle(f, rows[2], d);
    render_providers(f, rows[3], statuses, d);
    render_footer(f, rows[4]);
}

fn render_header(f: &mut Frame, area: Rect, d: &Dashboard) {
    let t = &d.totals;
    let spans = vec![
        RSpan::styled(
            " aitop ",
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        RSpan::raw("  "),
        RSpan::styled(
            format!("{} tok", fmt_tokens(t.total_tokens())),
            Style::default().fg(ACCENT),
        ),
        RSpan::raw("  "),
        RSpan::styled(fmt_cost(t.cost), Style::default().fg(GREEN)),
        RSpan::raw("   in "),
        RSpan::styled(fmt_tokens(t.input_tokens), Style::default().fg(DIM)),
        RSpan::raw(" out "),
        RSpan::styled(fmt_tokens(t.output_tokens), Style::default().fg(DIM)),
        RSpan::raw(" cache "),
        RSpan::styled(
            format!(
                "{}r/{}w",
                fmt_tokens(t.cache_read_tokens),
                fmt_tokens(t.cache_write_tokens)
            ),
            Style::default().fg(DIM),
        ),
        RSpan::raw("   "),
        RSpan::styled(format!("{} calls", t.events), Style::default().fg(DIM)),
        RSpan::raw("   span "),
        RSpan::styled(d.span.to_string(), Style::default().fg(WARN_COLOR)),
    ];
    let p = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

const WARN_COLOR: Color = Color::Yellow;

/// Cost-over-time bar chart (btop-style).
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
    let title = format!(" cost burn · peak {} · {} ", fmt_cost(max_cost), d.span);
    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(title))
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

fn render_middle(f: &mut Frame, area: Rect, d: &Dashboard) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)])
        .split(area);
    render_table(f, cols[0], "projects", &d.by_project);
    render_table(f, cols[1], "models", &d.by_model);
}

fn render_table(f: &mut Frame, area: Rect, title: &str, rows: &[(String, UsageTotals)]) {
    let max_cost = rows.iter().map(|(_, t)| t.cost).fold(0.0f64, f64::max);
    let mut lines = vec![Line::from(vec![
        RSpan::styled(format!(" {:<20}", "name"), Style::default().fg(DIM)),
        RSpan::styled(format!("{:>8}  ", "tokens"), Style::default().fg(DIM)),
        RSpan::styled(format!("{:<14}", "cost"), Style::default().fg(DIM)),
    ])];
    let capacity = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;
    // name(20) + tokens(8) + cost(9) + spaces(4) → remainder is the bar
    let bar_w = inner_w.saturating_sub(41).clamp(3, 12);
    for (name, tot) in rows.iter().take(capacity.saturating_sub(1)) {
        let frac = if max_cost > 0.0 {
            tot.cost / max_cost
        } else {
            0.0
        };
        let filled = (frac * bar_w as f64).round() as usize;
        let bar =
            "█".repeat(filled.min(bar_w)) + &"·".repeat(bar_w.saturating_sub(filled.min(bar_w)));
        let short: String = name.chars().take(19).collect();
        lines.push(Line::from(vec![
            RSpan::styled(format!(" {:<19}", short), Style::default().fg(Color::Reset)),
            RSpan::styled(
                format!("{:>8}", fmt_tokens(tot.total_tokens())),
                Style::default().fg(ACCENT),
            ),
            RSpan::raw("  "),
            RSpan::styled(bar, Style::default().fg(GREEN)),
            RSpan::raw(" "),
            RSpan::styled(
                format!("{:>8}", fmt_cost(tot.cost)),
                Style::default().fg(GREEN),
            ),
        ]));
    }
    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", title)),
    );
    f.render_widget(p, area);
}

fn render_providers(f: &mut Frame, area: Rect, statuses: &[ProviderStatus], d: &Dashboard) {
    let n = statuses.len().max(1) as u32;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, n); statuses.len().max(1)])
        .split(area);

    for (i, st) in statuses.iter().enumerate() {
        let tot = d.by_provider.iter().find(|(name, _)| name == &st.name);
        let (tokens, cost) = tot
            .map(|(_, t)| (fmt_tokens(t.total_tokens()), fmt_cost(t.cost)))
            .unwrap_or_else(|| ("0".into(), "$0".into()));
        let lines = vec![
            Line::from(RSpan::styled(
                format!(" {}", st.name),
                Style::default()
                    .fg(if st.detected { ACCENT } else { DIM })
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                RSpan::styled(format!(" {} tok", tokens), Style::default().fg(DIM)),
                RSpan::styled(format!("  {}", cost), Style::default().fg(GREEN)),
            ]),
            Line::from(RSpan::styled(
                if st.detected {
                    " ● detected"
                } else {
                    " ○ not found"
                },
                Style::default().fg(if st.detected { GREEN } else { DIM }),
            )),
        ];
        let p = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));
        f.render_widget(p, cols[i]);
    }
}

fn render_footer(f: &mut Frame, area: Rect) {
    let help = Line::from(vec![
        RSpan::styled(" 1/2/3/4/5", Style::default().fg(ACCENT)),
        RSpan::styled(" span   ", Style::default().fg(DIM)),
        RSpan::styled("r", Style::default().fg(ACCENT)),
        RSpan::styled(" refresh   ", Style::default().fg(DIM)),
        RSpan::styled("q", Style::default().fg(ACCENT)),
        RSpan::styled(" quit", Style::default().fg(DIM)),
    ]);
    f.render_widget(Paragraph::new(help), area);
}
