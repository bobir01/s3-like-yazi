# MinIO/S3 TUI Explorer - Project Plan

## Project Overview

A Rust-based terminal user interface (TUI) application for exploring and managing S3-compatible remote storage (MinIO, AWS S3, Backblaze B2, Wasabi, etc.). The application will parse existing MinIO client (`mc`) credentials and provide an intuitive file browser with metadata inspection capabilities.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    MinIO TUI Explorer                           │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │   TUI Layer  │  │  S3 Client   │  │  Credential Manager  │   │
│  │  (Ratatui)   │  │  (minio-rs)  │  │     (keyring-rs)     │   │
│  └──────────────┘  └──────────────┘  └──────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Core Business Logic                         │   │
│  │  • Remote Browser  • Metadata Inspector  • Config Parser │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Technology Stack

### Core Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `minio` | latest | Official MinIO SDK - S3 operations |
| `rust-s3` | latest | Alternative S3 client (backup option) |
| `ratatui` | 0.30+ | Terminal UI framework |
| `crossterm` | latest | Cross-platform terminal manipulation |
| `tokio` | 1.x | Async runtime |
| `serde` / `serde_json` | latest | Config file parsing |
| `keyring` | 3.x | Secure credential storage |
| `dirs` | latest | Platform-specific directories |
| `chrono` | latest | Date/time handling |
| `humansize` | latest | Human-readable file sizes |
| `clap` | 4.x | CLI argument parsing |

### Cargo.toml Template

```toml
[package]
name = "minio-explorer"
version = "0.1.0"
edition = "2024"
authors = ["Your Name"]
description = "TUI file explorer for MinIO/S3 compatible storage"
license = "MIT"
repository = "https://github.com/yourusername/minio-explorer"

[dependencies]
# S3 Client
minio = "0.2"

# TUI Framework
ratatui = { version = "0.30", features = ["crossterm"] }
crossterm = "0.28"

# Async Runtime
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Credential Management
keyring = { version = "3", features = [
    "apple-native",
    "windows-native", 
    "sync-secret-service"
]}

# Utilities
dirs = "6"
chrono = { version = "0.4", features = ["serde"] }
humansize = "2"
clap = { version = "4", features = ["derive"] }
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

---

## Feature Modules

### 1. Credential Manager (`src/credentials/`)

**Purpose**: Parse existing `mc` config and manage credentials securely.

**MinIO Client Config Location**: `~/.mc/config.json`

**Config Format**:
```json
{
  "version": "10",
  "aliases": {
    "myminio": {
      "url": "https://minio.example.com:9000",
      "accessKey": "AKIAIOSFODNN7EXAMPLE",
      "secretKey": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
      "api": "s3v4",
      "path": "auto"
    },
    "play": {
      "url": "https://play.min.io",
      "accessKey": "Q3AM3UQ867SPQQA43P2F",
      "secretKey": "zuf+tfteSlswRu7BJ86wekitnifILbZam1KYY3TG",
      "api": "s3v4",
      "path": "auto"
    }
  }
}
```

**Implementation**:
```rust
// src/credentials/mod.rs
pub mod mc_config;
pub mod secure_store;

// src/credentials/mc_config.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct McConfig {
    pub version: String,
    pub aliases: HashMap<String, AliasConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AliasConfig {
    pub url: String,
    #[serde(rename = "accessKey")]
    pub access_key: String,
    #[serde(rename = "secretKey")]
    pub secret_key: String,
    pub api: Option<String>,
    pub path: Option<String>,
}

impl McConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Self::config_path()?;
        let content = std::fs::read_to_string(&config_path)?;
        Ok(serde_json::from_str(&content)?)
    }
    
    fn config_path() -> anyhow::Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
        Ok(home.join(".mc").join("config.json"))
    }
}
```

**Secure Storage (Optional Enhancement)**:
```rust
// src/credentials/secure_store.rs
use keyring::Entry;

pub struct SecureCredentialStore {
    service_name: String,
}

impl SecureCredentialStore {
    pub fn new() -> Self {
        Self {
            service_name: "minio-explorer".to_string(),
        }
    }
    
    pub fn store_secret(&self, alias: &str, secret_key: &str) -> anyhow::Result<()> {
        let entry = Entry::new(&self.service_name, alias)?;
        entry.set_password(secret_key)?;
        Ok(())
    }
    
    pub fn get_secret(&self, alias: &str) -> anyhow::Result<String> {
        let entry = Entry::new(&self.service_name, alias)?;
        Ok(entry.get_password()?)
    }
}
```

---

### 2. S3 Client Wrapper (`src/s3/`)

**Purpose**: Abstract S3 operations with async support.

```rust
// src/s3/mod.rs
pub mod client;
pub mod operations;
pub mod types;

// src/s3/client.rs
use minio::s3::MinioClient;
use minio::s3::creds::StaticProvider;
use minio::s3::http::BaseUrl;

pub struct S3Client {
    client: MinioClient,
    alias: String,
}

impl S3Client {
    pub async fn new(alias: &str, config: &AliasConfig) -> anyhow::Result<Self> {
        let base_url = config.url.parse::<BaseUrl>()?;
        let provider = StaticProvider::new(
            &config.access_key,
            &config.secret_key,
            None,
        );
        
        let client = MinioClient::new(base_url, Some(provider), None, None)?;
        
        Ok(Self {
            client,
            alias: alias.to_string(),
        })
    }
}

// src/s3/operations.rs
use minio::s3::types::S3Api;

impl S3Client {
    pub async fn list_buckets(&self) -> anyhow::Result<Vec<BucketInfo>> {
        let response = self.client.list_buckets().send().await?;
        // Transform to internal types
    }
    
    pub async fn list_objects(
        &self, 
        bucket: &str, 
        prefix: Option<&str>,
        delimiter: Option<&str>,
    ) -> anyhow::Result<Vec<ObjectInfo>> {
        let mut builder = self.client.list_objects_v2(bucket);
        if let Some(p) = prefix {
            builder = builder.prefix(p);
        }
        if let Some(d) = delimiter {
            builder = builder.delimiter(d);
        }
        let response = builder.send().await?;
        // Transform response
    }
    
    pub async fn head_object(
        &self, 
        bucket: &str, 
        key: &str
    ) -> anyhow::Result<ObjectMetadata> {
        let response = self.client
            .stat_object(bucket, key)
            .send()
            .await?;
        // Extract metadata
    }
}

// src/s3/types.rs
#[derive(Debug, Clone)]
pub struct BucketInfo {
    pub name: String,
    pub creation_date: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub last_modified: Option<chrono::DateTime<chrono::Utc>>,
    pub etag: Option<String>,
    pub storage_class: Option<String>,
    pub is_prefix: bool, // For "folder" representation
}

#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub key: String,
    pub size: u64,
    pub content_type: Option<String>,
    pub last_modified: Option<chrono::DateTime<chrono::Utc>>,
    pub etag: Option<String>,
    pub version_id: Option<String>,
    pub user_metadata: std::collections::HashMap<String, String>,
    pub system_metadata: SystemMetadata,
}

#[derive(Debug, Clone)]
pub struct SystemMetadata {
    pub content_length: u64,
    pub content_encoding: Option<String>,
    pub content_disposition: Option<String>,
    pub cache_control: Option<String>,
    pub storage_class: Option<String>,
}
```

---

### 3. TUI Application (`src/ui/`)

**Purpose**: Terminal-based file explorer interface.

**Layout**:
```
┌─ MinIO Explorer ──────────────────────────────────────────────────┐
│ ┌─ Remotes ─────┐ ┌─ Browser ───────────────────────────────────┐ │
│ │ > myminio     │ │ 📁 documents/                               │ │
│ │   play        │ │ 📁 images/                                  │ │
│ │   s3          │ │ 📄 readme.md              2.3 KB  Jan 15    │ │
│ │   backblaze   │ │ 📄 config.json            512 B   Jan 14    │ │
│ │               │ │ 📄 data.csv               1.2 MB  Jan 10    │ │
│ └───────────────┘ └─────────────────────────────────────────────┘ │
│ ┌─ Metadata Inspector ──────────────────────────────────────────┐ │
│ │ Key:           documents/readme.md                            │ │
│ │ Size:          2,350 bytes (2.3 KB)                           │ │
│ │ Content-Type:  text/markdown                                  │ │
│ │ Last Modified: 2025-01-15 14:32:18 UTC                        │ │
│ │ ETag:          "d41d8cd98f00b204e9800998ecf8427e"             │ │
│ │ User Metadata: author=john, project=docs                      │ │
│ └───────────────────────────────────────────────────────────────┘ │
│ [q]uit  [r]efresh  [↑↓]navigate  [←→]switch pane  [Enter]select  │
└───────────────────────────────────────────────────────────────────┘
```

**Key Components**:

```rust
// src/ui/mod.rs
pub mod app;
pub mod widgets;
pub mod event;
pub mod state;

// src/ui/state.rs
#[derive(Debug, Clone, PartialEq)]
pub enum ActivePane {
    Remotes,
    Browser,
    Metadata,
}

#[derive(Debug, Clone)]
pub enum BrowserLocation {
    Root,                           // Listing remotes
    Buckets(String),                // Listing buckets of a remote
    Objects {
        remote: String,
        bucket: String,
        prefix: String,
    },
}

pub struct AppState {
    pub active_pane: ActivePane,
    pub remotes: Vec<String>,
    pub selected_remote: Option<usize>,
    pub browser_location: BrowserLocation,
    pub browser_items: Vec<BrowserItem>,
    pub selected_item: Option<usize>,
    pub current_metadata: Option<ObjectMetadata>,
    pub loading: bool,
    pub error_message: Option<String>,
    pub scroll_offset: usize,
}

#[derive(Debug, Clone)]
pub enum BrowserItem {
    Bucket(BucketInfo),
    Folder(String),
    Object(ObjectInfo),
}

// src/ui/app.rs
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph, Table, Row},
};

pub struct App {
    state: AppState,
    s3_clients: HashMap<String, S3Client>,
    config: McConfig,
}

impl App {
    pub fn new(config: McConfig) -> Self {
        let remotes: Vec<String> = config.aliases.keys().cloned().collect();
        Self {
            state: AppState {
                active_pane: ActivePane::Remotes,
                remotes,
                selected_remote: Some(0),
                browser_location: BrowserLocation::Root,
                browser_items: vec![],
                selected_item: None,
                current_metadata: None,
                loading: false,
                error_message: None,
                scroll_offset: 0,
            },
            s3_clients: HashMap::new(),
            config,
        }
    }
    
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Initialize terminal
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        
        loop {
            terminal.draw(|f| self.ui(f))?;
            
            if let Some(action) = self.handle_events().await? {
                match action {
                    Action::Quit => break,
                    Action::Navigate(direction) => self.navigate(direction),
                    Action::Select => self.select_item().await?,
                    Action::Back => self.go_back().await?,
                    Action::Refresh => self.refresh().await?,
                    Action::SwitchPane => self.switch_pane(),
                }
            }
        }
        
        // Cleanup
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen
        )?;
        
        Ok(())
    }
    
    fn ui(&self, frame: &mut Frame) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),      // Title
                Constraint::Min(10),        // Main content
                Constraint::Length(3),      // Metadata
                Constraint::Length(1),      // Status bar
            ])
            .split(frame.area());
        
        // Title bar
        let title = Paragraph::new("MinIO Explorer")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
        frame.render_widget(title, main_layout[0]);
        
        // Main content area (split into remotes and browser)
        let content_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(80),
            ])
            .split(main_layout[1]);
        
        self.render_remotes_pane(frame, content_layout[0]);
        self.render_browser_pane(frame, content_layout[1]);
        self.render_metadata_pane(frame, main_layout[2]);
        self.render_status_bar(frame, main_layout[3]);
    }
    
    fn render_remotes_pane(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.state.remotes
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let style = if Some(i) == self.state.selected_remote {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let prefix = if Some(i) == self.state.selected_remote { "▶ " } else { "  " };
                ListItem::new(format!("{}{}", prefix, name)).style(style)
            })
            .collect();
        
        let border_style = if self.state.active_pane == ActivePane::Remotes {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        
        let list = List::new(items)
            .block(Block::default()
                .title(" Remotes ")
                .borders(Borders::ALL)
                .border_style(border_style));
        
        frame.render_widget(list, area);
    }
    
    fn render_browser_pane(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.state.browser_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let (icon, name, size, date) = match item {
                    BrowserItem::Bucket(b) => ("🪣", b.name.clone(), "-".to_string(), 
                        b.creation_date.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()),
                    BrowserItem::Folder(name) => ("📁", format!("{}/", name), "-".to_string(), "".to_string()),
                    BrowserItem::Object(o) => ("📄", o.key.clone(), 
                        humansize::format_size(o.size, humansize::BINARY),
                        o.last_modified.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()),
                };
                
                let style = if Some(i) == self.state.selected_item {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                
                ListItem::new(format!("{} {:<40} {:>10} {}", icon, name, size, date))
                    .style(style)
            })
            .collect();
        
        let title = match &self.state.browser_location {
            BrowserLocation::Root => " Browse ".to_string(),
            BrowserLocation::Buckets(remote) => format!(" {} / ", remote),
            BrowserLocation::Objects { remote, bucket, prefix } => {
                format!(" {} / {} / {} ", remote, bucket, prefix)
            }
        };
        
        let border_style = if self.state.active_pane == ActivePane::Browser {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        
        let list = List::new(items)
            .block(Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style));
        
        frame.render_widget(list, area);
    }
    
    fn render_metadata_pane(&self, frame: &mut Frame, area: Rect) {
        let content = if let Some(meta) = &self.state.current_metadata {
            format!(
                "Key: {}  │  Size: {} ({})  │  Type: {}  │  Modified: {}  │  ETag: {}",
                meta.key,
                meta.size,
                humansize::format_size(meta.size, humansize::BINARY),
                meta.content_type.as_deref().unwrap_or("unknown"),
                meta.last_modified.map(|d| d.to_rfc3339()).unwrap_or_default(),
                meta.etag.as_deref().unwrap_or("-")
            )
        } else {
            "Select an object to view metadata".to_string()
        };
        
        let border_style = if self.state.active_pane == ActivePane::Metadata {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        
        let paragraph = Paragraph::new(content)
            .block(Block::default()
                .title(" Metadata ")
                .borders(Borders::ALL)
                .border_style(border_style))
            .wrap(ratatui::widgets::Wrap { trim: true });
        
        frame.render_widget(paragraph, area);
    }
    
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status = if self.state.loading {
            "Loading...".to_string()
        } else if let Some(err) = &self.state.error_message {
            format!("Error: {}", err)
        } else {
            "[q]uit  [r]efresh  [↑↓]navigate  [←→]pane  [Enter]select  [Backspace]back".to_string()
        };
        
        let style = if self.state.error_message.is_some() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        
        let paragraph = Paragraph::new(status).style(style);
        frame.render_widget(paragraph, area);
    }
}

// src/ui/event.rs
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::time::Duration;

pub enum Action {
    Quit,
    Navigate(Direction),
    Select,
    Back,
    Refresh,
    SwitchPane,
}

pub enum Direction {
    Up,
    Down,
}

impl App {
    pub async fn handle_events(&self) -> anyhow::Result<Option<Action>> {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                return Ok(match code {
                    KeyCode::Char('q') => Some(Action::Quit),
                    KeyCode::Up | KeyCode::Char('k') => Some(Action::Navigate(Direction::Up)),
                    KeyCode::Down | KeyCode::Char('j') => Some(Action::Navigate(Direction::Down)),
                    KeyCode::Enter => Some(Action::Select),
                    KeyCode::Backspace | KeyCode::Left => Some(Action::Back),
                    KeyCode::Char('r') => Some(Action::Refresh),
                    KeyCode::Tab | KeyCode::Right => Some(Action::SwitchPane),
                    _ => None,
                });
            }
        }
        Ok(None)
    }
}
```

---

### 4. Project Structure

```
minio-explorer/
├── Cargo.toml
├── README.md
├── LICENSE
├── .github/
│   └── workflows/
│       └── ci.yml
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── credentials/
│   │   ├── mod.rs
│   │   ├── mc_config.rs
│   │   └── secure_store.rs
│   ├── s3/
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   ├── operations.rs
│   │   └── types.rs
│   └── ui/
│       ├── mod.rs
│       ├── app.rs
│       ├── state.rs
│       ├── event.rs
│       └── widgets/
│           ├── mod.rs
│           ├── browser.rs
│           ├── metadata.rs
│           └── remotes.rs
├── tests/
│   ├── integration/
│   │   └── s3_operations.rs
│   └── unit/
│       └── config_parsing.rs
└── examples/
    └── basic_usage.rs
```

---

## Implementation Phases

### Phase 1: Core Foundation (Week 1-2)
- [ ] Project setup with Cargo workspace
- [ ] Implement MC config parser (`~/.mc/config.json`)
- [ ] Basic S3 client wrapper with minio-rs
- [ ] List buckets and objects functionality
- [ ] Unit tests for config parsing

### Phase 2: TUI Framework (Week 2-3)
- [ ] Basic ratatui application scaffold
- [ ] Three-pane layout (remotes, browser, metadata)
- [ ] Keyboard navigation (vim-style + arrows)
- [ ] Remote selection widget
- [ ] File browser widget with folder navigation

### Phase 3: Metadata Inspector (Week 3-4)
- [ ] HeadObject integration for metadata
- [ ] Display system metadata (size, type, etag, dates)
- [ ] Display user-defined metadata (x-amz-meta-*)
- [ ] Scrollable metadata view for large metadata sets

### Phase 4: Enhanced Features (Week 4-5)
- [ ] Search/filter objects
- [ ] Copy path to clipboard
- [ ] Preview text files (first N bytes)
- [ ] Download object (with progress)
- [ ] Breadcrumb navigation display

### Phase 5: Polish & Distribution (Week 5-6)
- [ ] Error handling and user-friendly messages
- [ ] Loading indicators
- [ ] Configuration file for app settings
- [ ] Cross-platform testing (Linux, macOS, Windows)
- [ ] GitHub Actions CI/CD
- [ ] Release binaries
- [ ] Documentation and README

---

## Installation Methods

### From Source
```bash
git clone https://github.com/yourusername/minio-explorer
cd minio-explorer
cargo install --path .
```

### From crates.io (after publishing)
```bash
cargo install minio-explorer
```

### Pre-built Binaries
Provide binaries for:
- Linux x86_64 (glibc and musl)
- macOS x86_64 and ARM64
- Windows x86_64

---

## CLI Interface

```bash
# Basic usage - interactive TUI
minio-explorer

# Specify custom config location
minio-explorer --config /path/to/mc/config.json

# Start at specific remote/bucket
minio-explorer --remote myminio --bucket mybucket

# Non-interactive: list objects
minio-explorer ls myminio/mybucket/prefix/

# Non-interactive: show metadata
minio-explorer stat myminio/mybucket/myfile.txt

# Version and help
minio-explorer --version
minio-explorer --help
```

---

## Key Bindings Reference

| Key | Action |
|-----|--------|
| `q` | Quit application |
| `↑/k` | Move selection up |
| `↓/j` | Move selection down |
| `Enter` | Select item / Enter folder |
| `Backspace/←` | Go back / Parent directory |
| `Tab/→` | Switch active pane |
| `r` | Refresh current view |
| `/` | Search/filter (future) |
| `y` | Copy path to clipboard (future) |
| `d` | Download selected object (future) |
| `?` | Show help |

---

## Resources & References

### Documentation
- MinIO Rust SDK: https://github.com/minio/minio-rs
- Ratatui: https://ratatui.rs/
- rust-s3 (alternative): https://crates.io/crates/rust-s3
- keyring-rs: https://crates.io/crates/keyring

### Similar Projects for Inspiration
- Yazi (terminal file manager): https://github.com/sxyazi/yazi
- xplr (TUI file explorer): https://github.com/sayanarijit/xplr
- rainfrog (DB TUI): https://github.com/achristmascarl/rainfrog

### S3/MinIO References
- S3 Object Metadata: https://docs.aws.amazon.com/AmazonS3/latest/userguide/UsingMetadata.html
- MC Client Configuration: https://min.io/docs/minio/linux/reference/minio-mc.html

---

## Notes

1. **minio-rs vs rust-s3**: The official `minio` crate is recommended as it's maintained by MinIO and provides the most complete S3 compatibility. `rust-s3` is a good fallback if you encounter issues.

2. **Credential Security**: Consider using the `keyring` crate to optionally store credentials in the system keychain instead of reading plaintext from `~/.mc/config.json`.

3. **Async Considerations**: All S3 operations should be async. Use Tokio channels to communicate between the UI thread and background operations to avoid blocking the TUI.

4. **Cross-Platform**: Test on all three major platforms. The TUI should work identically, but file paths and keyring backends differ.
