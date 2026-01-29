use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, HighlightSpacing, List, ListItem, Paragraph, Row, Table, Wrap,
};
use ratatui::Frame;

use crate::app::{App, Entry, Pane};

use super::local_fs;
use super::popups;
use super::status;

pub fn render(frame: &mut Frame, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Title bar
            Constraint::Min(8),    // Main content
            Constraint::Length(7), // Metadata panel
            Constraint::Length(1), // Status / search bar
        ])
        .split(frame.area());

    // Title bar
    let title = Line::from(vec![
        Span::styled(
            " S3 Explorer ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", app.location_display()),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), outer[0]);

    // Main content: remotes + browser (+ local FS on right when downloading)
    if app.download_mode {
        let content = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22),
                Constraint::Min(30),
                Constraint::Percentage(40),
            ])
            .split(outer[1]);

        render_remotes(frame, app, content[0]);
        render_browser(frame, app, content[1]);
        local_fs::render_local_fs(frame, app, content[2]);

        // Show download target info in the metadata area
        let meta_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(outer[2]);

        local_fs::render_download_target(frame, app, meta_layout[0]);
        render_metadata(frame, app, meta_layout[1]);
    } else {
        let content = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(22), Constraint::Min(30)])
            .split(outer[1]);

        render_remotes(frame, app, content[0]);
        render_browser(frame, app, content[1]);
        render_metadata(frame, app, outer[2]);
    }

    if app.search_active {
        status::render_search_bar(frame, app, outer[3]);
    } else {
        status::render_status_bar(frame, app, outer[3]);
    }

    if app.confirm_delete.is_some() {
        popups::render_confirm_delete(frame, app);
    }

    if app.show_help {
        popups::render_help(frame);
    }
}

fn render_remotes(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let border_style = if app.pane == Pane::Remotes {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .remotes
        .iter()
        .map(|name| ListItem::new(format!("  {}", name)))
        .collect();

    let list = List::new(items)
        .block(
            Block::bordered()
                .title(" Remotes ")
                .border_style(border_style),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.remote_state);
}

fn render_browser(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let border_style = if app.pane == Pane::Browser {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let row_data: Vec<(String, String, String, String, Color, Color)> = app
        .entries
        .iter()
        .map(|entry| match entry {
            Entry::Bucket(b) => {
                let date = b.creation_date.clone().unwrap_or_default();
                ("B".into(), b.name.clone(), "bucket".into(), date, Color::Yellow, Color::White)
            }
            Entry::Object(obj) if obj.is_dir => (
                "D".into(),
                format!("{}/", obj.display_name),
                "dir".into(),
                String::new(),
                Color::Blue,
                Color::Blue,
            ),
            Entry::Object(obj) => {
                let size = humansize::format_size(obj.size as u64, humansize::BINARY);
                let date = obj.last_modified.clone().unwrap_or_default();
                (" ".into(), obj.display_name.clone(), size, date, Color::Reset, Color::White)
            }
        })
        .collect();

    let visible_len = row_data.len();

    let rows: Vec<Row> = row_data
        .iter()
        .map(|(icon, name, size, date, icon_color, name_color)| {
            let size_color = if icon.trim().is_empty() { Color::Green } else { Color::DarkGray };
            Row::new(vec![
                Cell::from(icon.as_str()).style(Style::default().fg(*icon_color)),
                Cell::from(name.as_str()).style(Style::default().fg(*name_color)),
                Cell::from(format!("{:>10}", size)).style(Style::default().fg(size_color)),
                Cell::from(format!("{:>16}", date)).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(1),  // icon
        Constraint::Min(20),    // name (fills remaining)
        Constraint::Length(10), // size / type
        Constraint::Length(16), // date
    ];

    let title = if app.search_active {
        format!(
            " {} [{} matches] ",
            app.location_display(),
            visible_len
        )
    } else {
        format!(" {} ", app.location_display())
    };

    let table = Table::new(rows, widths)
        .block(
            Block::bordered()
                .title(title)
                .border_style(border_style),
        )
        .column_spacing(1)
        .row_highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
        .highlight_spacing(HighlightSpacing::Always);

    frame.render_stateful_widget(table, area, &mut app.browser_state);
}

fn render_metadata(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let content = if let Some(meta) = &app.metadata {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("  Key:          ", Style::default().fg(Color::Cyan)),
                Span::raw(&meta.key),
            ]),
            Line::from(vec![
                Span::styled("  Size:         ", Style::default().fg(Color::Cyan)),
                Span::raw(format!(
                    "{} ({})",
                    meta.size,
                    humansize::format_size(meta.size as u64, humansize::BINARY)
                )),
            ]),
            Line::from(vec![
                Span::styled("  Content-Type: ", Style::default().fg(Color::Cyan)),
                Span::raw(meta.content_type.as_deref().unwrap_or("unknown")),
            ]),
            Line::from(vec![
                Span::styled("  Modified:     ", Style::default().fg(Color::Cyan)),
                Span::raw(meta.last_modified.as_deref().unwrap_or("-")),
            ]),
            Line::from(vec![
                Span::styled("  ETag:         ", Style::default().fg(Color::Cyan)),
                Span::raw(meta.etag.as_deref().unwrap_or("-")),
            ]),
        ];

        if !meta.user_metadata.is_empty() {
            let meta_str: Vec<String> = meta
                .user_metadata
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            lines.push(Line::from(vec![
                Span::styled("  Metadata:     ", Style::default().fg(Color::Cyan)),
                Span::raw(meta_str.join(", ")),
            ]));
        }

        lines
    } else {
        vec![Line::from(Span::styled(
            "  Press Enter on a file to view metadata",
            Style::default().fg(Color::DarkGray),
        ))]
    };

    let block = Block::bordered()
        .title(" Metadata ")
        .border_style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(content).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
