# s3-like-yazi Knowledgebase

This document captures the current implementation of `s3-like-yazi` for future
contributors and coding agents. Treat the source code as the source of truth;
`research.md` is historical planning material and may describe older design
ideas that no longer match the app.

## Project Purpose

`s3-like-yazi` is a Rust terminal file manager for S3-compatible storage such as
MinIO, AWS S3, Backblaze B2, and Wasabi. It uses Ratatui and Crossterm for the
terminal interface, the AWS S3 SDK for object storage operations, and Tokio for
background tasks.

The app starts from existing MinIO client credentials, shows configured remotes,
lets the user browse buckets and object prefixes, and supports search, metadata
inspection, recursive delete, local downloads, manual bucket entry, presigned
download URLs, and file previews.

## Build And Verification

Common commands:

```bash
cargo test --no-run
cargo build
cargo build --release
```

There are currently no dedicated unit or integration tests. `cargo test
--no-run` is the baseline compile verification command and should succeed before
and after documentation or code changes.

Runtime requirements:

- Rust 1.85 or newer, because the crate uses edition 2024.
- A MinIO client config at `~/.mc/config.json` or `~/.mcli/config.json`.
- `ffplay` from ffmpeg for image and video previews. Text previews work without
  ffmpeg.

## Architecture Map

Top-level flow:

- `src/main.rs` loads the MinIO config, constructs `App`, and enters `ui::run`.
- `src/credentials.rs` parses `mc`/`mcli` config aliases, including optional
  per-alias region.
- `src/app/mod.rs` owns the central state machine: active pane, navigation
  location, entries, metadata, search state, indexing state, download state,
  preview state, manual bucket input, cached S3 clients, and navigation history.
- `src/s3_client.rs` wraps AWS SDK S3 calls for buckets, objects, indexing,
  deletion, metadata, byte ranges, presigned URLs, and downloads.
- `src/ui/mod.rs` owns terminal setup, the event loop, key dispatch, background
  channel draining, and terminal cleanup.
- `src/ui/render.rs`, `src/ui/status.rs`, `src/ui/popups.rs`, and
  `src/ui/local_fs.rs` render the main panes, status/search bars, dialogs, text
  preview, metadata, and local download chooser.

Feature modules:

- Navigation: `src/app/navigation.rs` moves through remotes, buckets, prefixes,
  metadata, refresh, back navigation, manual bucket entry, and presigned links.
- Search: `src/app/search.rs` handles search mode, substring matching, glob
  matching with `*` and `?`, regex matching with `/pattern/`, and jumping to a
  selected object.
- Background indexing: `src/app/indexing.rs` starts and drains a task that
  streams every object in the active bucket into the search pool.
- Delete: `src/app/delete.rs` confirms and deletes files or recursively deletes
  all objects under a prefix. Bucket deletion is intentionally unsupported.
- Download: `src/app/download.rs` enters local save mode, allows rename, starts
  object or prefix downloads, and drains progress updates.
- Local filesystem: `src/app/local_fs.rs` lists local directories for download
  destinations and hides dotfiles.
- Preview: `src/app/preview.rs` detects preview kind by content type or file
  extension. Text previews download a byte range inline; image/video previews
  generate a presigned URL and open `ffplay`.

## Data And Control Flow

Startup:

1. `main` calls `McConfig::load`.
2. Config loading checks `~/.mc/config.json` first, then
   `~/.mcli/config.json`.
3. `App::new` sorts aliases into the remotes pane and selects the first remote
   when present.
4. `ui::run` enters raw mode, switches to the alternate screen, runs the event
   loop, then restores the terminal on exit.

Remote and bucket browsing:

1. Selecting a remote lazily creates an `S3Client` from that alias config.
2. `list_buckets` populates the browser pane with bucket entries.
3. If bucket listing is denied or fails, the app opens manual bucket input so
   users can type a known bucket name.
4. Selecting a bucket or directory prefix calls `list_objects` with delimiter
   `/`, producing directory entries from common prefixes and file entries from
   object contents.
5. Navigation history remembers cursor positions when moving into buckets and
   prefixes, then restores them when going back.

S3 client behavior:

- Clients use `aws-sdk-s3` with an explicit endpoint URL and
  `force_path_style(true)`, which is important for many S3-compatible services.
- The default region is `us-east-1` when the config does not provide one.
- On `AuthorizationHeaderMalformed` errors containing `expecting '<region>'`,
  the app recreates the client with the hinted region and retries bucket
  listing once.

Search and indexing:

- Entering a bucket or prefix starts background indexing for the whole bucket.
- The indexing task pages through `ListObjectsV2` without a delimiter and sends
  batches over a Tokio channel.
- The event loop calls `drain_index` every tick, extending `search_pool` and
  updating active search results as batches arrive.
- Search within an indexed bucket matches against full object keys. Without an
  index context, it filters the current saved entry list.
- Search supports case-insensitive substring matching, glob patterns containing
  `*` or `?`, and regex mode when the query is wrapped as `/pattern/`.
- Starting indexing for a different bucket aborts the previous indexing task.

Metadata and presigned URLs:

- Pressing Enter on a file calls `head_object` and renders size, content type,
  modified time, ETag, optional version/storage fields, and user metadata.
- Pressing `Shift+L` on a file creates a one-hour presigned GET URL and attempts
  to copy it to the clipboard via `arboard`. If clipboard access fails, the URL
  is still shown in the metadata panel.

Downloads:

- Pressing `Shift+C` on an object enters download mode and shows a local
  filesystem pane.
- The local pane starts at the process current directory, hides dotfiles, sorts
  directories first, and supports entering directories or going to the parent.
- Pressing `n` allows a custom save name. Pressing `c` confirms the download.
- File downloads stream one object to disk and report byte progress.
- Directory downloads list all keys under the selected prefix, then download
  them concurrently with a fixed concurrency of four.
- Downloads create parent directories as needed and report aggregate progress
  through the status bar.

Deletion:

- Pressing `d` or Cmd+Backspace on an object opens a confirmation dialog.
- File deletion uses `DeleteObject`.
- Directory deletion lists all objects under the selected prefix and deletes in
  `DeleteObjects` batches of up to 1000 keys.
- After deletion, the visible entries and search pool are pruned and selection
  is repaired.
- Bucket deletion is not supported.

Previews:

- Pressing `p` previews the selected file.
- Text-like files fetch at most 512 KiB with a range request and render inline.
- JSON text is pretty-printed when possible.
- Text preview mode has its own scroll keys and can be closed with `q` or Esc.
- Image and video previews generate a presigned URL and spawn `ffplay`.
- On macOS, Linux, and Windows the preview module makes a best-effort attempt to
  focus the `ffplay` window.

## Keybindings

Normal mode:

| Key | Action |
| --- | --- |
| `q` | Quit |
| `j` / Down | Move cursor down |
| `k` / Up | Move cursor up |
| `l` / Enter | Open remote, bucket, directory, or fetch file metadata |
| `h` / Backspace | Go back |
| Tab | Switch panes |
| `/` or Ctrl+P | Start search |
| `r` | Refresh current location |
| `Shift+C` | Enter download mode for the selected object or prefix |
| `d` or Cmd+Backspace | Request delete for selected object or prefix |
| `Shift+L` | Generate and copy a one-hour presigned download URL |
| `i` | Enter a bucket name manually from a bucket-list location |
| `p` | Preview selected file |
| `?` | Show help |
| Esc | Dismiss error, metadata, URL, status, progress, or preview |

Search mode:

| Key | Action |
| --- | --- |
| Text input | Filter results |
| Backspace | Remove one query character |
| Up / Down | Move through matches |
| Enter | Jump to selected match |
| Esc | Cancel search and restore previous view |

Search syntax:

- Plain text performs case-insensitive substring matching.
- Queries containing `*` or `?` use glob matching.
- `/pattern/` uses case-insensitive regex matching.

Download mode:

| Key | Action |
| --- | --- |
| `j` / Down | Move in active pane |
| `k` / Up | Move in active pane |
| `l` / Enter | Open selected local directory, or select remote item when browser pane is active |
| `h` / Backspace | Go to parent local directory, or go back remotely when browser pane is active |
| Tab | Switch between remotes, browser, and local filesystem panes |
| `c` | Confirm download to current local path |
| `n` | Rename before saving |
| Esc | Cancel download mode |

Rename input:

| Key | Action |
| --- | --- |
| Text input | Append to target name |
| Backspace | Remove one character |
| Enter | Accept custom name |
| Esc | Cancel rename |

Manual bucket input:

| Key | Action |
| --- | --- |
| Text input | Edit bucket name |
| Backspace | Remove one character |
| Enter | Open typed bucket |
| Esc | Cancel and return to remotes |

Delete confirmation:

| Key | Action |
| --- | --- |
| Tab | Toggle No/Yes |
| Enter | Confirm selected option |
| Esc | Cancel |

Text preview mode:

| Key | Action |
| --- | --- |
| `j` / Down | Scroll down one line |
| `k` / Up | Scroll up one line |
| Ctrl+D | Scroll down 20 lines |
| Ctrl+U | Scroll up 20 lines |
| `g` | Go to top |
| `G` | Go to bottom |
| `q` or Esc | Close preview |

## Implementation Notes And Gotchas

- Keep `research.md` as historical context only. Prefer current source files
  when documenting or changing behavior.
- README currently understates some implemented features, especially downloads,
  manual bucket entry, previews, presigned URLs, and glob/regex search.
- `Cargo.lock` and `.DS_Store` may already be dirty in local worktrees; avoid
  modifying or reverting them unless explicitly requested.
- The app intentionally uses path-style S3 addressing for compatibility with
  MinIO and other S3-compatible endpoints.
- Long-running background work communicates with the UI through Tokio channels
  and is drained from the main event loop. Avoid blocking the event loop.
- Starting a new bucket index aborts the previous index task and clears the
  search pool.
- Preview image/video support depends on an external `ffplay` executable rather
  than embedding media in the TUI.
- Text preview downloads only the first 512 KiB, so it is a preview, not a full
  file viewer.
- Recursive delete and directory download both operate by object-key prefix.
  They do not depend on real directory objects existing in S3.
- Local download destination browsing skips hidden entries by name.
- There are no current automated behavioral tests. For risky code changes,
  prefer adding focused unit tests around pure logic such as search matching,
  parent-prefix navigation, region hint parsing, and state transitions.
