use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::{App, Pane};

pub fn render_local_fs(frame: &mut Frame, app: &mut App, area: Rect) {
    let border_style = if app.pane == Pane::LocalFs {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut items: Vec<ListItem> = vec![ListItem::new(Span::styled(
        "  ../",
        Style::default().fg(Color::Blue),
    ))];

    for entry in &app.local_entries {
        let (icon, color) = if entry.is_dir {
            ("D ", Color::Blue)
        } else {
            ("  ", Color::White)
        };
        let display = if entry.is_dir {
            format!("{}{}/", icon, entry.name)
        } else {
            format!("{}{}", icon, entry.name)
        };
        items.push(ListItem::new(Span::styled(display, Style::default().fg(color))));
    }

    let path_display = app.local_path_display();
    let title = format!(" Save to: {} ", path_display);

    let bottom_hint = Line::from(vec![
        Span::styled(" c", Style::default().fg(Color::Yellow)),
        Span::raw(": save here "),
        Span::styled("n", Style::default().fg(Color::Yellow)),
        Span::raw(": rename "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(": cancel "),
    ]);

    let list = List::new(items)
        .block(
            Block::bordered()
                .title(title)
                .title_bottom(bottom_hint)
                .border_style(border_style),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // Offset selection by 1 because of the "../" entry.
    // We adjust in-place to preserve scroll offset, then restore after render.
    let real_sel = app.local_state.selected();
    let adjusted = real_sel.map(|s| s + 1).unwrap_or(0);
    app.local_state.select(Some(adjusted));

    frame.render_stateful_widget(list, area, &mut app.local_state);

    // Restore the real selection (undo the +1 offset)
    let rendered_sel = app.local_state.selected().unwrap_or(0);
    if rendered_sel == 0 {
        app.local_state.select(None); // "../" selected
    } else {
        app.local_state.select(Some(rendered_sel - 1));
    }
}

pub fn render_download_target(frame: &mut Frame, app: &App, area: Rect) {
    let target = app.download_target_name().unwrap_or_default();
    let label = if app.rename_active {
        Line::from(vec![
            Span::styled(" Name: ", Style::default().fg(Color::Cyan)),
            Span::raw(app.rename_input.as_deref().unwrap_or("")),
            Span::styled("_", Style::default().fg(Color::DarkGray)),
        ])
    } else if app.rename_input.is_some() {
        Line::from(vec![
            Span::styled(" Save as: ", Style::default().fg(Color::Cyan)),
            Span::styled(&target, Style::default().fg(Color::Green)),
            Span::styled("  n", Style::default().fg(Color::Yellow)),
            Span::raw(" rename"),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Save as: ", Style::default().fg(Color::Cyan)),
            Span::raw(&target),
            Span::styled("  n", Style::default().fg(Color::Yellow)),
            Span::raw(" rename"),
        ])
    };

    frame.render_widget(Paragraph::new(label), area);
}

pub fn render_download_progress(app: &App, area_width: u16) -> Option<Line<'static>> {
    let progress = match &app.download_progress {
        Some(p) if !p.complete => p,
        _ => return None,
    };

    let pct = if progress.total_bytes > 0 {
        (progress.bytes_downloaded as f64 / progress.total_bytes as f64 * 100.0) as u16
    } else {
        0
    };

    // Progress bar
    let bar_width = 16u16.min(area_width.saturating_sub(50));
    let filled = (bar_width as f64 * pct as f64 / 100.0) as usize;
    let empty = bar_width as usize - filled;
    let bar = format!(
        "{}{}",
        "\u{2588}".repeat(filled),   // █
        "\u{2591}".repeat(empty),    // ░
    );

    // Speed
    let speed = humansize::format_size(progress.speed_bps as u64, humansize::BINARY);

    // ETA
    let eta = if progress.speed_bps > 0.0 && progress.total_bytes > progress.bytes_downloaded {
        let remaining = progress.total_bytes - progress.bytes_downloaded;
        let secs = (remaining as f64 / progress.speed_bps) as u64;
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m{}s", secs / 60, secs % 60)
        }
    } else {
        "-".to_string()
    };

    // File count for directory downloads
    let files_info = if progress.files_total > 1 {
        format!(" {}/{} files", progress.files_done, progress.files_total)
    } else {
        String::new()
    };

    Some(Line::from(vec![
        Span::styled(
            format!(" \u{2193} {} ", progress.filename), // ↓
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("[{}]", bar),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!(" {}%", pct),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("  {}/s", speed),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("  ETA {}", eta),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(files_info, Style::default().fg(Color::DarkGray)),
    ]))
}
