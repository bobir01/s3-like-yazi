use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{App, Entry, Location};

/// Messages sent from background preview task to the UI.
pub enum PreviewMsg {
    /// Text content ready to display inline.
    TextReady(String),
    /// Progress while reading a remote MCAP summary/index.
    MCapProgress {
        bytes_read: u64,
        total_bytes: Option<u64>,
    },
    /// Error during preview.
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreviewKind {
    Image,
    Video,
    Text,
    MCap,
}

pub struct PreviewProgress {
    pub label: String,
    pub bytes_read: u64,
    pub total_bytes: Option<u64>,
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
    /// Progress for remote range-based preview loading.
    pub progress: Option<PreviewProgress>,
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
            progress: None,
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
        self.progress = None;
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
            self.scroll_offset =
                (self.scroll_offset + lines).min(self.line_count.saturating_sub(1));
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
        "mcap" | "svo2" => Some(PreviewKind::MCap),
        "txt" | "md" | "markdown" | "json" | "yaml" | "yml" | "toml" | "xml" | "csv" | "tsv"
        | "log" | "ini" | "cfg" | "conf" | "env" | "sh" | "bash" | "zsh" | "fish" | "py" | "rs"
        | "go" | "js" | "ts" | "jsx" | "tsx" | "html" | "htm" | "css" | "scss" | "less" | "sql"
        | "rb" | "lua" | "c" | "cpp" | "h" | "hpp" | "java" | "kt" | "swift" | "r" | "R" | "pl"
        | "pm" | "php" | "ex" | "exs" | "erl" | "hs" | "ml" | "tf" | "hcl" | "dockerfile"
        | "makefile" | "cmake" | "gitignore" | "dockerignore" | "editorconfig" | "properties" => {
            Some(PreviewKind::Text)
        }
        _ => None,
    }
}

impl App {
    /// Drain preview messages from background task.
    pub fn drain_preview(&mut self) {
        let is_json = self
            .preview
            .current_key
            .as_deref()
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
                    self.preview.progress = None;
                    let text = if is_json {
                        try_pretty_json(&text)
                    } else {
                        text
                    };
                    self.preview.line_count = text.lines().count();
                    self.preview.scroll_offset = 0;
                    self.preview.text_content = Some(text);
                    self.status_message = Some("Preview ready".to_string());
                }
                PreviewMsg::MCapProgress {
                    bytes_read,
                    total_bytes,
                } => {
                    self.preview.progress = Some(PreviewProgress {
                        label: "Reading MCAP index".to_string(),
                        bytes_read,
                        total_bytes,
                    });
                }
                PreviewMsg::Error(e) => {
                    self.preview.loading = false;
                    self.preview.progress = None;
                    self.preview.error = Some(e);
                    self.status_message = None;
                }
            }
        }
    }

    pub fn copy_preview_to_clipboard(&mut self) {
        let Some(text) = self.preview.text_content.as_ref() else {
            return;
        };

        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.clone())) {
            Ok(()) => {
                self.status_message = Some("Preview copied to clipboard".to_string());
            }
            Err(e) => {
                self.status_message = Some(format!("Clipboard copy failed: {}", e));
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
            PreviewKind::MCap => {
                self.preview.loading = true;
                self.preview.progress = Some(PreviewProgress {
                    label: "Reading MCAP index".to_string(),
                    bytes_read: 0,
                    total_bytes: None,
                });
                self.status_message = Some("Reading MCAP index from remote...".into());

                let size = size.max(0) as u64;
                let handle = tokio::spawn(async move {
                    let result = read_remote_mcap_summary(&client, &bucket, &key_clone, size, &tx)
                        .await
                        .map(format_mcap_summary);

                    let msg = match result {
                        Ok(text) => PreviewMsg::TextReady(text),
                        Err(e) => PreviewMsg::Error(e.to_string()),
                    };
                    let _ = tx.send(msg).await;
                });
                self.preview.handle = Some(handle);
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
                                "-v".to_string(),
                                "warning".to_string(),
                                "-autoexit".to_string(),
                                "-alwaysontop".to_string(),
                                "-window_title".to_string(),
                                key_clone.clone(),
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
                                    })
                                    .await;
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
                    let ct = self.metadata.as_ref().and_then(|m| m.content_type.clone());
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

struct RangeCache {
    start: u64,
    bytes: Vec<u8>,
}

impl RangeCache {
    fn contains(&self, start: u64, end: u64) -> bool {
        let cache_end = self.start.saturating_add(self.bytes.len() as u64);
        start >= self.start && end <= cache_end
    }

    fn slice(&self, start: u64, end: u64) -> anyhow::Result<&[u8]> {
        if !self.contains(start, end) {
            anyhow::bail!("requested range is outside the MCAP preview cache");
        }

        let local_start = (start - self.start) as usize;
        let local_end = (end - self.start) as usize;
        Ok(&self.bytes[local_start..local_end])
    }
}

async fn read_remote_mcap_summary(
    client: &crate::s3_client::S3Client,
    bucket: &str,
    key: &str,
    size: u64,
    tx: &mpsc::Sender<PreviewMsg>,
) -> anyhow::Result<mcap::Summary> {
    if size == 0 {
        anyhow::bail!("Object is empty");
    }

    let mut reader = mcap::sans_io::SummaryReader::new_with_options(
        mcap::sans_io::SummaryReaderOptions::default().with_file_size(size),
    );
    let mut cache: Option<RangeCache> = None;
    let mut pos = 0u64;
    let mut bytes_read = 0u64;
    let mut total_needed: Option<u64> = None;

    while let Some(event) = reader.next_event() {
        match event? {
            mcap::sans_io::SummaryReadEvent::ReadRequest(need) => {
                let end = pos
                    .checked_add(need as u64)
                    .ok_or_else(|| anyhow::anyhow!("MCAP range overflow"))?;
                let was_cached = cache.as_ref().is_some_and(|cache| cache.contains(pos, end));
                let bytes = if was_cached {
                    cache
                        .as_ref()
                        .expect("cache checked above")
                        .slice(pos, end)?
                        .to_vec()
                } else {
                    client.get_object_range(bucket, key, pos, end).await?
                };
                let read = bytes.len().min(need);
                reader.insert(need)[..read].copy_from_slice(&bytes[..read]);
                reader.notify_read(read);
                pos = pos.saturating_add(read as u64);

                if !was_cached {
                    bytes_read = bytes_read.saturating_add(read as u64);
                    send_mcap_progress(tx, bytes_read, total_needed);
                }
            }
            mcap::sans_io::SummaryReadEvent::SeekRequest(to) => {
                let next = seek_position(size, pos, to)?;
                reader.notify_seeked(next);
                pos = next;

                if pos < size && cache.as_ref().is_none_or(|cache| !cache.contains(pos, pos)) {
                    let range_len = size - pos;
                    total_needed = Some(bytes_read.saturating_add(range_len));
                    send_mcap_progress(tx, bytes_read, total_needed);

                    let bytes = client.get_object_range(bucket, key, pos, size).await?;
                    let loaded = bytes.len() as u64;
                    cache = Some(RangeCache { start: pos, bytes });
                    bytes_read = bytes_read.saturating_add(loaded);
                    send_mcap_progress(tx, bytes_read, total_needed);
                }
            }
        }
    }

    reader
        .finish()
        .ok_or_else(|| anyhow::anyhow!("MCAP file has no summary/index section"))
}

fn send_mcap_progress(tx: &mpsc::Sender<PreviewMsg>, bytes_read: u64, total_bytes: Option<u64>) {
    let _ = tx.try_send(PreviewMsg::MCapProgress {
        bytes_read,
        total_bytes,
    });
}

fn seek_position(size: u64, current: u64, seek: std::io::SeekFrom) -> anyhow::Result<u64> {
    let pos = match seek {
        std::io::SeekFrom::Start(pos) => pos as i128,
        std::io::SeekFrom::End(delta) => size as i128 + delta as i128,
        std::io::SeekFrom::Current(delta) => current as i128 + delta as i128,
    };
    if pos < 0 || pos > size as i128 {
        anyhow::bail!("MCAP reader requested invalid seek position {}", pos);
    }
    Ok(pos as u64)
}

fn format_mcap_time(ns: u64) -> String {
    if ns == 0 {
        return "-".to_string();
    }

    let secs = (ns / 1_000_000_000) as i64;
    let nanos = (ns % 1_000_000_000) as u32;
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string())
        .unwrap_or_else(|| ns.to_string())
}

fn format_duration_ns(start: u64, end: u64) -> String {
    if end <= start {
        return "-".to_string();
    }

    let secs = (end - start) as f64 / 1_000_000_000.0;
    if secs >= 3600.0 {
        format!("{:.2} h", secs / 3600.0)
    } else if secs >= 60.0 {
        format!("{:.2} min", secs / 60.0)
    } else {
        format!("{:.3} s", secs)
    }
}

fn format_mcap_summary(summary: mcap::Summary) -> String {
    let mut lines = Vec::new();

    lines.push("MCAP Summary".to_string());
    lines.push(String::new());

    if let Some(stats) = &summary.stats {
        lines.push(format!("Messages:        {}", stats.message_count));
        lines.push(format!("Schemas:         {}", stats.schema_count));
        lines.push(format!("Channels:        {}", stats.channel_count));
        lines.push(format!("Chunks:          {}", stats.chunk_count));
        lines.push(format!("Attachments:     {}", stats.attachment_count));
        lines.push(format!("Metadata:        {}", stats.metadata_count));
        lines.push(format!(
            "Start time:      {}",
            format_mcap_time(stats.message_start_time)
        ));
        lines.push(format!(
            "End time:        {}",
            format_mcap_time(stats.message_end_time)
        ));
        lines.push(format!(
            "Duration:        {}",
            format_duration_ns(stats.message_start_time, stats.message_end_time)
        ));
    } else {
        lines.push("Statistics:      not present".to_string());
        lines.push(format!("Schemas:         {}", summary.schemas.len()));
        lines.push(format!("Channels:        {}", summary.channels.len()));
        lines.push(format!("Chunks:          {}", summary.chunk_indexes.len()));
        lines.push(format!(
            "Attachments:     {}",
            summary.attachment_indexes.len()
        ));
        lines.push(format!(
            "Metadata:        {}",
            summary.metadata_indexes.len()
        ));
    }

    lines.push(String::new());
    lines.push("Channels".to_string());
    let mut channels: Vec<_> = summary.channels.values().collect();
    channels.sort_by_key(|channel| channel.id);
    if channels.is_empty() {
        lines.push("  none".to_string());
    } else {
        for channel in channels.iter().take(200) {
            let schema_name = channel
                .schema
                .as_ref()
                .map(|schema| schema.name.as_str())
                .unwrap_or("-");
            lines.push(format!(
                "  {:>4}  {}  [{}]  schema: {}",
                channel.id, channel.topic, channel.message_encoding, schema_name
            ));
        }
        if channels.len() > 200 {
            lines.push(format!("  ... {} more channels", channels.len() - 200));
        }
    }

    if !summary.schemas.is_empty() {
        lines.push(String::new());
        lines.push("Schemas".to_string());
        let mut schemas: Vec<_> = summary.schemas.values().collect();
        schemas.sort_by_key(|schema| schema.id);
        for schema in schemas.iter().take(100) {
            lines.push(format!(
                "  {:>4}  {}  [{}]  {}",
                schema.id,
                schema.name,
                schema.encoding,
                humansize::format_size(schema.data.len() as u64, humansize::BINARY)
            ));
        }
        if schemas.len() > 100 {
            lines.push(format!("  ... {} more schemas", schemas.len() - 100));
        }
    }

    if let Some(stats) = &summary.stats {
        if !stats.channel_message_counts.is_empty() {
            lines.push(String::new());
            lines.push("Message Counts By Channel".to_string());
            for (channel_id, count) in &stats.channel_message_counts {
                let topic = summary
                    .channels
                    .get(channel_id)
                    .map(|channel| channel.topic.as_str())
                    .unwrap_or("-");
                lines.push(format!("  {:>4}  {:>12}  {}", channel_id, count, topic));
            }
        }
    }

    if !summary.attachment_indexes.is_empty() {
        lines.push(String::new());
        lines.push("Attachments".to_string());
        for attachment in &summary.attachment_indexes {
            lines.push(format!(
                "  {}  [{}]  {}",
                attachment.name,
                attachment.media_type,
                humansize::format_size(attachment.data_size, humansize::BINARY)
            ));
        }
    }

    if !summary.metadata_indexes.is_empty() {
        lines.push(String::new());
        lines.push("Metadata Records".to_string());
        for metadata in &summary.metadata_indexes {
            lines.push(format!(
                "  {}  {}",
                metadata.name,
                humansize::format_size(metadata.length, humansize::BINARY)
            ));
        }
    }

    lines.join("\n")
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
