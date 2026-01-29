use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn render_confirm_delete(frame: &mut Frame, app: &App) {
    let confirm = match &app.confirm_delete {
        Some(v) => v,
        None => return,
    };

    let area = frame.area();
    let width = 54u16.min(area.width.saturating_sub(4));
    let height = 8u16;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = ratatui::layout::Rect::new(x, y, width, height);

    let label = if confirm.is_dir {
        format!("  Delete directory \"{}\" recursively?", confirm.display_name)
    } else {
        format!("  Delete \"{}\"?", confirm.display_name)
    };

    let (no_style, yes_style) = if confirm.selected_yes {
        (
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::Red)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::DarkGray),
        )
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            label,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("          "),
            Span::styled(" No ", no_style),
            Span::raw("     "),
            Span::styled(" Yes ", yes_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Tab switch  Enter confirm  Esc cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::bordered()
        .title(" Confirm Delete ")
        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Red));

    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

pub fn render_help(frame: &mut Frame) {
    let area = frame.area();

    let width = 52u16.min(area.width.saturating_sub(4));
    let height = 23u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = ratatui::layout::Rect::new(x, y, width, height);

    let key = |k: &str| Span::styled(format!(" {:<14}", k), Style::default().fg(Color::Yellow));
    let desc = |d: &str| Span::styled(d.to_string(), Style::default().fg(Color::White));

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Navigation",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![key("j / Down"), desc("Move cursor down")]),
        Line::from(vec![key("k / Up"), desc("Move cursor up")]),
        Line::from(vec![key("l / Enter"), desc("Open / select item")]),
        Line::from(vec![key("h / Bksp"), desc("Go back / parent dir")]),
        Line::from(vec![key("Tab"), desc("Switch pane")]),
        Line::from(""),
        Line::from(Span::styled(
            "  Actions",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![key("/ or Ctrl+P"), desc("Search all objects")]),
        Line::from(vec![key("r"), desc("Refresh current view")]),
        Line::from(vec![key("d / Cmd+Bksp"), desc("Delete file or directory")]),
        Line::from(vec![key("Esc"), desc("Dismiss error / metadata")]),
        Line::from(vec![key("q"), desc("Quit")]),
        Line::from(""),
        Line::from(Span::styled(
            "  Search Mode",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![key("Type"), desc("Filter by name")]),
        Line::from(vec![key("Up / Down"), desc("Navigate results")]),
        Line::from(vec![key("Enter"), desc("Jump to file")]),
        Line::from(vec![key("Esc"), desc("Cancel search")]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::bordered()
        .title(" Keybindings ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}
