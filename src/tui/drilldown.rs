use crate::aggregator::Dashboard;
use crate::model::UsageTotals;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as RSpan},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

const ACCENT: Color = Color::Cyan;
const GREEN: Color = Color::Green;
const DIM: Color = Color::DarkGray;
fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Per-project drill-down screen.
pub struct Drilldown {
    /// (project name, totals) sorted by cost desc
    pub projects: Vec<(String, UsageTotals)>,
    pub state: ListState,
    /// when set, show project detail instead of the list
    pub selected: Option<usize>,
    /// model breakdown for the selected project, sorted by cost desc
    pub detail_models: Vec<(String, UsageTotals)>,
    /// hourly burn of selected project over the last 24h
    pub detail_burn: Vec<(String, u64)>,
}

impl Drilldown {
    pub fn new(dash: &Dashboard) -> Self {
        let mut projects: Vec<(String, UsageTotals)> = dash
            .by_project
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        projects.sort_by(|a, b| {
            b.1.cost
                .partial_cmp(&a.1.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut state = ListState::default();
        state.select(Some(0));
        Drilldown {
            projects,
            state,
            selected: None,
            detail_models: Vec::new(),
            detail_burn: Vec::new(),
        }
    }

    pub fn refresh_detail(&mut self, dash: &Dashboard) {
        let Some(sel) = self.selected else { return };
        let Some((name, _)) = self.projects.get(sel) else {
            return;
        };
        let mut models: Vec<(String, UsageTotals)> = dash
            .events
            .iter()
            .filter(|e| e.project.as_deref() == Some(name.as_str()))
            .fold(Vec::new(), |mut acc, e| {
                // group by model
                if let Some(slot) = acc.iter_mut().find(|(m, _)| m == &e.model) {
                    slot.1.add(e);
                } else {
                    let mut t = UsageTotals::default();
                    t.add(e);
                    acc.push((e.model.clone(), t));
                }
                acc
            });
        models.sort_by(|a, b| {
            b.1.cost
                .partial_cmp(&a.1.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.detail_models = models;

        // hourly buckets over last 24h
        let now = crate::aggregator::now_hint();
        let mut buckets = vec![0u64; 24];
        for e in dash
            .events
            .iter()
            .filter(|e| e.project.as_deref() == Some(name.as_str()))
        {
            let age = now.saturating_sub(e.ts);
            if age < 86_400 {
                let idx = ((86_400 - age) / 3_600).min(23) as usize;
                buckets[idx] += e.cost.unwrap_or(0.0).max(0.0) as u64;
            }
        }
        self.detail_burn = buckets
            .into_iter()
            .enumerate()
            .map(|(i, c)| (format!("{}h", 23 - i), c * 1000))
            .collect();
    }

    pub fn up(&mut self) {
        if self.selected.is_some() {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some(i.saturating_sub(1)));
    }

    pub fn down(&mut self) {
        if self.selected.is_some() {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        let max = self.projects.len().saturating_sub(1);
        self.state.select(Some((i + 1).min(max)));
    }

    pub fn enter(&mut self) -> bool {
        if self.selected.is_none() && !self.projects.is_empty() {
            self.selected = self.state.selected();
            true
        } else {
            false
        }
    }

    pub fn back(&mut self) {
        self.selected = None;
    }
}

pub fn render(f: &mut Frame, dd: &mut Drilldown, span_label: &str) {
    match dd.selected {
        None => render_list(f, dd, span_label),
        Some(_) => render_detail(f, dd, span_label),
    }
}

fn render_list(f: &mut Frame, dd: &mut Drilldown, span_label: &str) {
    let items: Vec<ListItem> = dd
        .projects
        .iter()
        .map(|(name, t)| {
            ListItem::new(Line::from(vec![
                RSpan::styled(format!(" {:<32}", name), Style::default()),
                RSpan::styled(
                    format!("{:>10}", crate::model::fmt_tokens(t.total_tokens())),
                    Style::default().fg(ACCENT),
                ),
                RSpan::raw("  "),
                RSpan::styled(
                    format!("{:>9}", crate::model::fmt_cost(t.cost)),
                    Style::default().fg(GREEN),
                ),
                RSpan::raw(format!("  {} calls", t.events)),
            ]))
        })
        .collect();
    let help = "↑/↓ select · enter drill · r rescan · q quit";
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" projects ({}) · {} ", span_label, help)),
    );
    f.render_stateful_widget(list, f.area(), &mut dd.state);
}

fn render_detail(f: &mut Frame, dd: &mut Drilldown, span_label: &str) {
    let Some(sel) = dd.selected else { return };
    let Some((name, total)) = dd.projects.get(sel) else {
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // summary
            Constraint::Length(12), // model breakdown
            Constraint::Min(8),     // burn
            Constraint::Length(1),  // footer
        ])
        .split(f.area());

    // summary line
    let summary = Paragraph::new(Line::from(vec![
        RSpan::styled(format!(" {} ", name), bold().fg(ACCENT)),
        RSpan::raw(format!(
            "  {} tok  {}  {} calls  ({})",
            crate::model::fmt_tokens(total.total_tokens()),
            crate::model::fmt_cost(total.cost),
            total.events,
            span_label
        )),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(summary, rows[0]);

    // model breakdown table
    let mut lines = vec![Line::from(vec![
        RSpan::styled(format!(" {:<28}", "model"), Style::default().fg(DIM)),
        RSpan::styled(format!("{:>10}", "tokens"), Style::default().fg(DIM)),
        RSpan::raw("  "),
        RSpan::styled(format!("{:>9}", "cost"), Style::default().fg(DIM)),
    ])];
    let max_cost = dd
        .detail_models
        .iter()
        .map(|(_, t)| t.cost)
        .fold(0.0f64, f64::max);
    for (m, t) in dd.detail_models.iter().take(9) {
        let frac = if max_cost > 0.0 {
            t.cost / max_cost
        } else {
            0.0
        };
        let filled = (frac * 10.0).round() as usize;
        let bar = "█".repeat(filled.min(10)) + &"·".repeat(10 - filled.min(10));
        lines.push(Line::from(vec![
            RSpan::styled(format!(" {:<28}", m), Style::default()),
            RSpan::styled(
                format!("{:>10}", crate::model::fmt_tokens(t.total_tokens())),
                Style::default().fg(ACCENT),
            ),
            RSpan::raw("  "),
            RSpan::styled(bar, Style::default().fg(GREEN)),
            RSpan::raw(" "),
            RSpan::styled(
                format!("{:>9}", crate::model::fmt_cost(t.cost)),
                Style::default().fg(GREEN),
            ),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" models ")),
        rows[1],
    );

    // hourly burn — reuse BarChart
    let bars: Vec<ratatui::widgets::Bar> = dd
        .detail_burn
        .iter()
        .map(|(label, v)| {
            ratatui::widgets::Bar::default()
                .label(label.as_str().into())
                .value(*v)
        })
        .collect();
    let chart = ratatui::widgets::BarChart::default()
        .data(ratatui::widgets::BarGroup::default().bars(&bars))
        .bar_width(2)
        .bar_gap(1);
    f.render_widget(
        chart,
        Rect {
            x: rows[2].x + 1,
            y: rows[2].y + 1,
            width: rows[2].width.saturating_sub(2),
            height: rows[2].height.saturating_sub(2),
        },
    );
    // border on top of chart area (chart doesn't take a block in this layout)
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" cost burn · last 24h "),
        rows[2],
    );

    let footer = Paragraph::new(Line::from(vec![RSpan::styled(
        " backspace/esc back · r rescan · q quit",
        Style::default().fg(DIM),
    )]));
    f.render_widget(footer, rows[3]);
}
