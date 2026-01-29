use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use super::local_fs;

pub fn render_search_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let index_status = if app.index_complete {
        format!("{} objects", app.index_object_count())
    } else {
        format!("indexing {}...", app.index_object_count())
    };

    let line = Line::from(vec![
        Span::styled(
            " /",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(&app.search_query),
        Span::styled("_", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("  {} matches", app.entries.len()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("  ({})", index_status),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

pub fn render_status_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if let Some(err) = &app.error {
        let content = Line::from(Span::styled(
            format!(" Error: {} (press Esc to dismiss)", err),
            Style::default().fg(Color::Red),
        ));
        frame.render_widget(Paragraph::new(content), area);
        return;
    }

    // Show download progress if active
    if let Some(progress_line) = local_fs::render_download_progress(app, area.width) {
        frame.render_widget(Paragraph::new(progress_line), area);
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(
            app.status_message.as_ref().map_or(0, |m| m.len() as u16 + 2),
        )])
        .split(area);

    let hints = if app.download_mode {
        Line::from(vec![
            Span::styled(" j/k", Style::default().fg(Color::Yellow)),
            Span::raw(" nav  "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" open dir  "),
            Span::styled("h/Bksp", Style::default().fg(Color::Yellow)),
            Span::raw(" back  "),
            Span::styled("c", Style::default().fg(Color::Yellow)),
            Span::raw(" confirm  "),
            Span::styled("n", Style::default().fg(Color::Yellow)),
            Span::raw(" rename  "),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(" pane  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled(" q", Style::default().fg(Color::Yellow)),
            Span::raw(" quit  "),
            Span::styled("j/k", Style::default().fg(Color::Yellow)),
            Span::raw(" nav  "),
            Span::styled("Enter/l", Style::default().fg(Color::Yellow)),
            Span::raw(" open  "),
            Span::styled("h/Bksp", Style::default().fg(Color::Yellow)),
            Span::raw(" back  "),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(" pane  "),
            Span::styled("r", Style::default().fg(Color::Yellow)),
            Span::raw(" refresh  "),
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(" search  "),
            Span::styled("?", Style::default().fg(Color::Yellow)),
            Span::raw(" help"),
        ])
    };
    frame.render_widget(Paragraph::new(hints), cols[0]);

    if let Some(msg) = &app.status_message {
        let status = Line::from(Span::styled(
            format!(" {} ", msg),
            Style::default().fg(Color::Green),
        ));
        frame.render_widget(Paragraph::new(status), cols[1]);
    }
}
