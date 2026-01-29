# s3-like-yazi

A blazing-fast terminal file manager for S3-compatible storage (MinIO, AWS S3, etc.), inspired by [Yazi](https://github.com/sxyazi/yazi).

![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange)
![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- **Dual-pane TUI** — remotes list on the left, file browser on the right
- **Vim-style navigation** — `j/k` to move, `l/Enter` to open, `h/Backspace` to go back
- **Instant recursive search** — press `/` or `Ctrl+P` to fuzzy-find across all objects in a bucket
- **Background indexing** — objects are streamed in the background so search is ready before you need it
- **File metadata** — press `Enter` on a file to view size, content-type, ETag, and custom metadata
- **Delete with confirmation** — `d` to delete files or directories recursively, with a Tab/Enter confirmation dialog
- **Multi-remote support** — reads credentials from your existing MinIO client (`mc`) config
- **Help overlay** — press `?` to see all keybindings

## Installation

```bash
# Clone and build
git clone https://github.com/bobir01/s3-like-yazi.git
cd s3-like-yazi
cargo build --release

# Run
./target/release/yazi-like-s3
```

### Requirements

- Rust 1.85+ (edition 2024)
- A MinIO client config at `~/.mc/config.json` or `~/.mcli/config.json`

If you don't have one, set it up with:

```bash
mc alias set myminio http://localhost:9000 ACCESS_KEY SECRET_KEY
```

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `Down` | Move cursor down |
| `k` / `Up` | Move cursor up |
| `l` / `Enter` | Open / select item |
| `h` / `Backspace` | Go back / parent directory |
| `Tab` | Switch between remotes and browser panes |
| `/` or `Ctrl+P` | Search all objects in current bucket |
| `d` | Delete selected file or directory |
| `r` | Refresh current view |
| `?` | Show help overlay |
| `Esc` | Dismiss error / metadata / status |
| `q` | Quit |

### Search mode

| Key | Action |
|-----|--------|
| Type | Filter results by name |
| `Up` / `Down` | Navigate results |
| `Enter` | Jump to selected file |
| `Esc` | Cancel search |

## Architecture

```
src/
├── main.rs           — entry point
├── credentials.rs    — MinIO mc config parser
├── s3_client.rs      — S3 SDK wrapper (list, delete, head, stream)
├── app/
│   ├── mod.rs        — core state machine and types
│   ├── navigation.rs — cursor movement, selection, S3 browsing
│   ├── search.rs     — fuzzy search with live filtering
│   ├── delete.rs     — file/directory deletion with confirmation
│   └── indexing.rs   — background object streaming via channels
└── ui/
    ├── mod.rs        — terminal setup and event loop
    ├── render.rs     — main layout, remotes panel, browser table, metadata
    ├── popups.rs     — help and delete confirmation overlays
    └── status.rs     — status bar and search bar
```

## How it works

1. Reads your `mc` config to discover S3-compatible remotes
2. When you enter a bucket, it starts a background task that streams all object keys via paginated `ListObjectsV2`
3. Pressing `/` instantly opens search mode using the pre-built index — results update live as more objects stream in
4. Deletion uses `DeleteObjects` batch API (up to 1000 keys per call) for fast recursive directory removal

## License

[MIT](LICENSE)
