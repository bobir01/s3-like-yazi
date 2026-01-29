mod popups;
mod render;
mod status;

use std::io;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{execute, queue};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use crate::app::App;

pub async fn run(app: &mut App) -> anyhow::Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    // Try to enable kitty keyboard protocol for Cmd key detection (best-effort)
    let has_enhanced_keys = queue!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES)
    )
    .is_ok();
    if has_enhanced_keys {
        use std::io::Write;
        let _ = stdout.flush();
    }

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = event_loop(&mut terminal, app).await;

    // Always restore terminal
    terminal::disable_raw_mode()?;
    if has_enhanced_keys {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        app.drain_index();

        terminal.draw(|frame| render::render(frame, app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if app.confirm_delete.is_some() {
                    match key.code {
                        KeyCode::Tab => app.toggle_delete_confirm(),
                        KeyCode::Enter => {
                            let yes = app
                                .confirm_delete
                                .as_ref()
                                .map_or(false, |c| c.selected_yes);
                            if yes {
                                app.confirm_delete_yes().await;
                            } else {
                                app.confirm_delete = None;
                            }
                        }
                        KeyCode::Esc => {
                            app.confirm_delete = None;
                        }
                        _ => {}
                    }
                } else if app.show_help {
                    app.show_help = false;
                } else if app.search_active {
                    match key.code {
                        KeyCode::Esc => app.cancel_search(),
                        KeyCode::Enter => app.select().await,
                        KeyCode::Up => app.move_up(),
                        KeyCode::Down => app.move_down(),
                        KeyCode::Backspace => app.search_backspace(),
                        KeyCode::Char(c) => app.search_input(c),
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('p')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.start_search();
                        }
                        KeyCode::Char('/') => app.start_search(),
                        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                        KeyCode::Enter | KeyCode::Char('l') => app.select().await,
                        KeyCode::Backspace
                            if key.modifiers.contains(KeyModifiers::SUPER) =>
                        {
                            app.request_delete();
                        }
                        KeyCode::Char('d') => app.request_delete(),
                        KeyCode::Backspace | KeyCode::Char('h') => app.go_back().await,
                        KeyCode::Char('r') => app.refresh().await,
                        KeyCode::Tab => app.switch_pane(),
                        KeyCode::Char('?') => app.show_help = true,
                        KeyCode::Esc => {
                            app.error = None;
                            app.metadata = None;
                            app.status_message = None;
                        }
                        _ => {}
                    }
                }

                if app.should_quit {
                    break;
                }
            }
        }
    }
    Ok(())
}
