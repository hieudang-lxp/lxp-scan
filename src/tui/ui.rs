use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::output::highlight;
use crate::tui::app::{App, PackState};

pub fn render(frame: &mut Frame, app: &App) {
    let [main, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(32), Constraint::Percentage(68)]).areas(main);
    render_symbol_list(frame, app, left);
    render_detail(frame, app, right);
    render_footer(frame, app, footer);
}

fn render_symbol_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .map(|&idx| {
            let symbol = &app.symbols[idx];
            ListItem::new(Line::from(vec![
                Span::raw(symbol.name.clone()),
                Span::styled(
                    format!("  {}", symbol.repos().join(", ")),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let title = format!(" Components ({}) ", app.filtered.len());
    let filter_line = if app.filter.is_empty() {
        " type to filter ".to_string()
    } else {
        format!(" filter: {}█ ", app.filter)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_bottom(filter_line),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(if app.filtered.is_empty() {
        None
    } else {
        Some(app.selected)
    });
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(symbol) = app.selected_symbol() else {
        let empty = Paragraph::new("no component matches the filter")
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(empty, area);
        return;
    };
    let title = format!(" {} — {} ", symbol.name, symbol.repos().join(", "));
    let lines = match app.current_pack() {
        None | Some(PackState::Loading) => vec![Line::from("scanning usages …".dark_gray())],
        Some(PackState::Failed(err)) => vec![Line::from(format!("scan failed: {err}").red())],
        Some(PackState::Ready(pack)) => pack_lines(app, pack),
    };
    let detail = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(detail, area);
}

fn pack_lines(app: &App, pack: &crate::features::context::ContextPack) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(format!(
        "{} sites · {} files · {} repos",
        pack.total_sites, pack.total_files, pack.total_repos
    ))];
    if !pack.prop_counts.is_empty() {
        let props: Vec<String> = pack
            .prop_counts
            .iter()
            .take(8)
            .map(|(prop, count)| format!("{prop} ×{count}"))
            .collect();
        lines.push(Line::from(format!("props: {}", props.join(" · ")).cyan()));
    }
    if let Some(excerpt) = pack.excerpts.get(app.excerpt_idx) {
        lines.push(Line::default());
        lines.push(Line::from(
            format!(
                "── Usage {}/{} · {} · {}:{} ",
                app.excerpt_idx + 1,
                pack.excerpts.len(),
                excerpt.repo,
                excerpt.file,
                excerpt.line
            )
            .bold(),
        ));
        lines.extend(code_lines(&excerpt.code));
    }
    if let Some(def) = &pack.definition {
        lines.push(Line::default());
        lines.push(Line::from(
            format!("── Definition · {} · {}:{} ", def.repo, def.file, def.line).bold(),
        ));
        lines.extend(code_lines(&def.excerpt));
    }
    if !pack.same_name.is_empty() {
        lines.push(Line::default());
        for group in &pack.same_name {
            lines.push(Line::from(
                format!(
                    "note: {} other site(s) import a different {} from {}",
                    group.sites, pack.symbol, group.repo
                )
                .yellow(),
            ));
        }
    }
    lines
}

fn code_lines(code: &str) -> Vec<Line<'static>> {
    highlight::highlight_lines(code)
        .into_iter()
        .map(|runs| {
            Line::from(
                runs.into_iter()
                    .map(|(style, text)| {
                        let fg = style.foreground;
                        Span::styled(text, Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)))
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let footer = match &app.status {
        Some(status) => Line::from(status.clone().red()),
        None => {
            let mut hint =
                " ↑↓ select · Tab next usage · Enter open in editor · Esc quit".to_string();
            if app.scan_warnings > 0 {
                hint.push_str(&format!(" · {} scan warning(s)", app.scan_warnings));
            }
            Line::from(hint.dark_gray())
        }
    };
    frame.render_widget(Paragraph::new(footer), area);
}
