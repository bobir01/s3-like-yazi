use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{App, Entry, Location};

/// Messages sent from background preview task to the UI.
pub enum PreviewMsg {
    /// Text content ready to display inline.
    TextReady(String),
    /// Error during preview.
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreviewKind {
    Image,
    Video,
    Text,
}

/// Current state of the preview system.
pub struct PreviewState {
    /// The S3 key currently being previewed.
    pub current_key: Option<String>,
    /// Text content for inline preview.
    pub text_content: Option<String>,
    /// Whether preview is loading.
    pub loading: bool,
    /// Error message if preview failed.
    pub error: Option<String>,
    /// Scroll offset (line index) for text preview.
    pub scroll_offset: usize,
    /// Total line count of text_content (cached).
    pub line_count: usize,
    /// Background task channel.
    pub rx: Option<mpsc::Receiver<PreviewMsg>>,
    /// Background task handle.
    pub handle: Option<JoinHandle<()>>,
}

/// Max bytes to download for text preview (512 KB).
const MAX_TEXT_BYTES: i64 = 512 * 1024;

impl PreviewState {
    pub fn new() -> Self {
        Self {
            current_key: None,
            text_content: None,
            loading: false,
            error: None,
            scroll_offset: 0,
            line_count: 0,
            rx: None,
            handle: None,
        }
    }

    pub fn clear(&mut self) {
        self.current_key = None;
        self.text_content = None;
        self.loading = false;
        self.error = None;
        self.scroll_offset = 0;
        self.line_count = 0;
        self.rx = None;
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        if self.line_count > 0 {
            self.scroll_offset = (self.scroll_offset + lines).min(self.line_count.saturating_sub(1));
        }
    }
}

/// Try to parse and pretty-print JSON. Falls back to the original text on failure.
fn try_pretty_json(text: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(val) => serde_json::to_string_pretty(&val).unwrap_or_else(|_| text.to_string()),
        Err(_) => text.to_string(),
    }
}

fn content_type_to_kind(content_type: &str) -> Option<PreviewKind> {
    let ct = content_type.to_lowercase();
    if ct.starts_with("image/") {
        Some(PreviewKind::Image)
    } else if ct.starts_with("video/") {
        Some(PreviewKind::Video)
    } else if ct.starts_with("text/")
        || ct == "application/json"
        || ct == "application/xml"
        || ct == "application/javascript"
        || ct == "application/x-yaml"
        || ct == "application/toml"
        || ct == "application/x-sh"
    {
        Some(PreviewKind::Text)
    } else {
        None
    }
}

fn extension_to_kind(key: &str) -> Option<PreviewKind> {
    let ext = key.rsplit('.').next()?.to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "ico" | "tiff" | "tif" | "svg" => {
            Some(PreviewKind::Image)
        }
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "3gp" => {
            Some(PreviewKind::Video)
        }
        "txt" | "md" | "markdown" | "json" | "yaml" | "yml" | "toml" | "xml" | "csv"
        | "tsv" | "log" | "ini" | "cfg" | "conf" | "env" | "sh" | "bash" | "zsh"
        | "fish" | "py" | "rs" | "go" | "js" | "ts" | "jsx" | "tsx" | "html" | "htm"
        | "css" | "scss" | "less" | "sql" | "rb" | "lua" | "c" | "cpp" | "h" | "hpp"
        | "java" | "kt" | "swift" | "r" | "R" | "pl" | "pm" | "php" | "ex" | "exs"
        | "erl" | "hs" | "ml" | "tf" | "hcl" | "dockerfile" | "makefile" | "cmake"
        | "gitignore" | "dockerignore" | "editorconfig" | "properties" => {
            Some(PreviewKind::Text)
        }
        _ => None,
    }
}

impl App {
    /// Drain preview messages from background task.
    pub fn drain_preview(&mut self) {
        let is_json = self.preview.current_key.as_deref()
            .and_then(|k| k.rsplit('.').next())
            .map(|ext| ext.eq_ignore_ascii_case("json"))
            .unwrap_or(false);

        let Some(rx) = &mut self.preview.rx else {
            return;
        };

        while let Ok(msg) = rx.try_recv() {
            match msg {
                PreviewMsg::TextReady(text) => {
                    self.preview.loading = false;
                    let text = if is_json {
                        try_pretty_json(&text)
                    } else {
                        text
                    };
                    self.preview.line_count = text.lines().count();
                    self.preview.scroll_offset = 0;
                    self.preview.text_content = Some(text);
                }
                PreviewMsg::Error(e) => {
                    self.preview.loading = false;
                    self.preview.error = Some(e);
                }
            }
        }
    }

    /// Request preview for the currently selected entry.
    /// Triggered explicitly by the user pressing 'p'.
    pub fn request_preview(&mut self) {
        let (remote, bucket, key, content_type, size) = match self.selected_file_info() {
            Some(info) => info,
            None => {
                self.status_message = Some("No file selected for preview".into());
                return;
            }
        };

        // Determine preview kind from content_type (metadata) or extension
        let kind = content_type
            .as_deref()
            .and_then(content_type_to_kind)
            .or_else(|| extension_to_kind(&key));

        let kind = match kind {
            Some(k) => k,
            None => {
                self.status_message = Some("Unsupported file type for preview".into());
                return;
            }
        };

        // Cancel previous
        self.preview.clear();
        self.preview.current_key = Some(key.clone());

        let client = match self.clients.get(&remote) {
            Some(c) => c.clone(),
            None => return,
        };

        let (tx, rx) = mpsc::channel(4);
        self.preview.rx = Some(rx);

        let bucket = bucket.clone();
        let key_clone = key.clone();

        match kind {
            PreviewKind::Text => {
                self.preview.loading = true;
                self.status_message = Some("Loading text preview...".into());

                let fetch_size = size.min(MAX_TEXT_BYTES) as u64;
                tokio::spawn(async move {
                    match client
                        .get_object_range(&bucket, &key_clone, 0, fetch_size)
                        .await
                    {
                        Ok(bytes) => {
                            let text = String::from_utf8_lossy(&bytes).to_string();
                            let _ = tx.send(PreviewMsg::TextReady(text)).await;
                        }
                        Err(e) => {
                            let _ = tx.send(PreviewMsg::Error(e.to_string())).await;
                        }
                    }
                });
            }
            PreviewKind::Image | PreviewKind::Video => {
                let label = match kind {
                    PreviewKind::Image => "image",
                    PreviewKind::Video => "video",
                    _ => unreachable!(),
                };
                self.status_message = Some(format!("Opening {} in ffplay...", label));

                let extra_args: Vec<String> = match kind {
                    PreviewKind::Image => vec!["-loop".into(), "0".into()],
                    PreviewKind::Video => vec!["-showmode".into(), "video".into()],
                    _ => unreachable!(),
                };

                tokio::spawn(async move {
                    match client.presign_get_object(&bucket, &key_clone).await {
                        Ok(url) => {
                            let mut args = vec![
                                "-v".to_string(), "warning".to_string(),
                                "-autoexit".to_string(),
                                "-alwaysontop".to_string(),
                                "-window_title".to_string(), key_clone.clone(),
                            ];
                            args.extend(extra_args);
                            args.push(url);

                            let result = std::process::Command::new("ffplay")
                                .args(&args)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn();

                            match result {
                                Ok(child) => {
                                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                    focus_window().await;
                                    let _ = tokio::task::spawn_blocking(move || {
                                        child.wait_with_output()
                                    }).await;
                                }
                                Err(_) => {
                                    let _ = tx
                                        .send(PreviewMsg::Error(
                                            "ffplay not found - install ffmpeg for preview".into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx
                                .send(PreviewMsg::Error(format!("Presign failed: {}", e)))
                                .await;
                        }
                    }
                });
            }
        }
    }

    /// Extract info about the currently selected file for preview.
    fn selected_file_info(&self) -> Option<(String, String, String, Option<String>, i64)> {
        let idx = self.browser_state.selected()?;
        let entry = self.entries.get(idx)?;

        match entry {
            Entry::Object(obj) if !obj.is_dir => {
                if let Location::ObjectList {
                    ref remote,
                    ref bucket,
                    ..
                } = self.location
                {
                    let ct = self
                        .metadata
                        .as_ref()
                        .and_then(|m| m.content_type.clone());
                    Some((
                        remote.clone(),
                        bucket.clone(),
                        obj.key.clone(),
                        ct,
                        obj.size,
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Clean up temp files on exit.
    pub fn cleanup_preview(&self) {
        let temp_dir = std::env::temp_dir().join("s3-like-yazi-preview");
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}

/// Bring the ffplay window to front and give it keyboard focus.
async fn focus_window() {
    #[cfg(target_os = "macos")]
    {
        let script = r#"tell application "System Events"
    set frontmost of every process whose name is "ffplay" to true
end tell"#;

        let _ = tokio::process::Command::new("osascript")
            .args(["-e", script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await;
    }

    #[cfg(target_os = "linux")]
    {
        let wmctrl = tokio::process::Command::new("wmctrl")
            .args(["-a", "ffplay"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await;

        if wmctrl.is_err() || !wmctrl.unwrap().status.success() {
            let _ = tokio::process::Command::new("xdotool")
                .args(["search", "--name", "ffplay", "windowactivate"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .await;
        }
    }

    #[cfg(target_os = "windows")]
    {
        let script = r#"Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class Win { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd); [DllImport("user32.dll")] public static extern IntPtr FindWindow(string lpClassName, string lpWindowName); }'; $h = [Win]::FindWindow([NullString]::Value, (Get-Process ffplay -ErrorAction SilentlyContinue | Select-Object -First 1).MainWindowTitle); if ($h) { [Win]::SetForegroundWindow($h) }"#;

        let _ = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await;
    }
}
