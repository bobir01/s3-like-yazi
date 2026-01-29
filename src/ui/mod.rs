pub mod local_fs;
mod popups;
mod render;
mod status;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use crate::app::{App, Pane};

pub async fn run(app: &mut App) -> anyhow::Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = event_loop(&mut terminal, app).await;

    terminal::disable_raw_mode()?;
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
        app.drain_download();

        terminal.draw(|frame| render::render(frame, app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if app.confirm_delete.is_some() {
                    // ── Delete confirmation ──
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
                } else if app.rename_active {
                    // ── Rename input mode (sub-mode of download) ──
                    match key.code {
                        KeyCode::Esc => app.cancel_rename(),
                        KeyCode::Enter => app.finish_rename(),
                        KeyCode::Backspace => app.rename_backspace(),
                        KeyCode::Char(c) => app.rename_char(c),
                        _ => {}
                    }
                } else if app.download_mode {
                    // ── Download mode: local FS navigation ──
                    match key.code {
                        KeyCode::Esc => app.cancel_download_mode(),
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.pane == Pane::LocalFs {
                                // Check if we're at the "../" position (local_state is None)
                                if app.local_state.selected().is_none() {
                                    // Already at top
                                } else {
                                    let i = app.local_state.selected().unwrap();
                                    if i == 0 {
                                        app.local_state.select(None); // go to "../"
                                    } else {
                                        app.local_move_up();
                                    }
                                }
                            } else {
                                app.move_up();
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.pane == Pane::LocalFs {
                                if app.local_state.selected().is_none() {
                                    if !app.local_entries.is_empty() {
                                        app.local_state.select(Some(0));
                                    }
                                } else {
                                    app.local_move_down();
                                }
                            } else {
                                app.move_down();
                            }
                        }
                        KeyCode::Enter | KeyCode::Char('l') => {
                            if app.pane == Pane::LocalFs {
                                if app.local_state.selected().is_none() {
                                    // "../" selected → go to parent
                                    app.local_go_back();
                                } else {
                                    let idx = app.local_state.selected().unwrap();
                                    if idx < app.local_entries.len()
                                        && app.local_entries[idx].is_dir
                                    {
                                        app.local_enter();
                                    }
                                }
                            } else {
                                app.select().await;
                            }
                        }
                        KeyCode::Backspace | KeyCode::Char('h') => {
                            if app.pane == Pane::LocalFs {
                                app.local_go_back();
                            } else {
                                app.go_back().await;
                            }
                        }
                        KeyCode::Char('c') => {
                            // Confirm download to current local dir
                            app.confirm_download().await;
                        }
                        KeyCode::Char('n') => {
                            if app.pane == Pane::LocalFs {
                                app.start_rename();
                            }
                        }
                        KeyCode::Tab => app.switch_pane(),
                        _ => {}
                    }
                } else if app.show_help {
                    app.show_help = false;
                } else if app.search_active {
                    // ── Search mode ──
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
                    // ── Normal mode ──
                    match key.code {
                        KeyCode::Char('q') => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('p')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.start_search();
                        }
                        KeyCode::Char('C') => app.start_download_mode(),
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
                            app.download_progress = None;
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
