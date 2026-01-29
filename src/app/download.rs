use std::time::Instant;

use tokio::sync::mpsc;

use super::{App, DownloadProgress, Entry, Location, Pane};

impl App {
    /// Enter download mode: snapshot the selected S3 entry, open local FS pane.
    pub fn start_download_mode(&mut self) {
        if self.search_active || self.download_mode {
            return;
        }

        // Must have something selected in browser
        let idx = match self.browser_state.selected() {
            Some(i) if i < self.entries.len() => i,
            _ => return,
        };

        // Must be in an ObjectList (inside a bucket)
        if !matches!(self.location, Location::ObjectList { .. }) {
            self.error = Some("Navigate into a bucket first".to_string());
            return;
        }

        let entry = &self.entries[idx];
        match entry {
            Entry::Object(obj) => {
                self.download_source = Some((obj.display_name.clone(), obj.key.clone()));
                self.download_source_is_dir = obj.is_dir;
            }
            Entry::Bucket(_) => {
                self.error = Some("Cannot download a bucket".to_string());
                return;
            }
        }

        self.download_mode = true;
        self.rename_input = None;
        self.rename_active = false;
        self.pane = Pane::LocalFs;
        self.list_local_dir();
    }

    /// Cancel download mode and go back to normal 2-pane layout.
    pub fn cancel_download_mode(&mut self) {
        self.download_mode = false;
        self.download_source = None;
        self.rename_input = None;
        self.rename_active = false;
        self.local_entries.clear();
        if self.pane == Pane::LocalFs {
            self.pane = Pane::Browser;
        }
    }

    /// Start typing a custom filename.
    pub fn start_rename(&mut self) {
        if let Some((ref display_name, _)) = self.download_source {
            self.rename_active = true;
            self.rename_input = Some(display_name.clone());
        }
    }

    pub fn rename_char(&mut self, c: char) {
        if let Some(ref mut input) = self.rename_input {
            input.push(c);
        }
    }

    pub fn rename_backspace(&mut self) {
        if let Some(ref mut input) = self.rename_input {
            input.pop();
        }
    }

    pub fn finish_rename(&mut self) {
        self.rename_active = false;
        // rename_input stays set — it's the custom name to use
    }

    pub fn cancel_rename(&mut self) {
        self.rename_active = false;
        self.rename_input = None;
    }

    /// The filename that will be used for downloading (custom or original).
    pub fn download_target_name(&self) -> Option<String> {
        if let Some(ref custom) = self.rename_input {
            if !custom.is_empty() {
                return Some(custom.clone());
            }
        }
        self.download_source
            .as_ref()
            .map(|(display, _)| display.clone())
    }

    /// Confirm download: start downloading to current local_path.
    pub async fn confirm_download(&mut self) {
        let (display_name, key) = match self.download_source.take() {
            Some(v) => v,
            None => return,
        };
        let is_dir = self.download_source_is_dir;

        let Location::ObjectList {
            ref remote,
            ref bucket,
            ..
        } = self.location
        else {
            return;
        };

        let client = match self.clients.get(remote) {
            Some(c) => c.clone(),
            None => {
                self.error = Some("Not connected to remote".to_string());
                return;
            }
        };

        let bucket = bucket.clone();
        let dest_dir = self.local_path.clone();

        let target_name = self
            .rename_input
            .take()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| display_name.clone());

        // Close the download mode pane
        self.download_mode = false;
        self.rename_active = false;
        self.local_entries.clear();
        self.pane = Pane::Browser;

        // Set up progress tracking
        let (tx, rx) = mpsc::channel(64);
        self.download_rx = Some(rx);
        self.download_started_at = Some(Instant::now());
        self.download_progress = Some(DownloadProgress {
            filename: target_name.clone(),
            bytes_downloaded: 0,
            total_bytes: 0,
            speed_bps: 0.0,
            files_done: 0,
            files_total: if is_dir { 0 } else { 1 },
            complete: false,
            error: None,
        });

        if is_dir {
            let dest = dest_dir.join(&target_name);
            let handle = tokio::spawn(async move {
                let result = client
                    .download_prefix(&bucket, &key, &dest, tx.clone(), 4)
                    .await;
                let msg = match result {
                    Ok(()) => crate::s3_client::DownloadMsg {
                        bytes_downloaded: 0,
                        total_bytes: 0,
                        files_done: 0,
                        files_total: 0,
                        complete: true,
                        error: None,
                    },
                    Err(e) => crate::s3_client::DownloadMsg {
                        bytes_downloaded: 0,
                        total_bytes: 0,
                        files_done: 0,
                        files_total: 0,
                        complete: true,
                        error: Some(e.to_string()),
                    },
                };
                let _ = tx.send(msg).await;
            });
            self.download_handle = Some(handle);
        } else {
            let dest = dest_dir.join(&target_name);
            let handle = tokio::spawn(async move {
                let result = client.download_object(&bucket, &key, &dest, &tx).await;
                let msg = match result {
                    Ok(()) => crate::s3_client::DownloadMsg {
                        bytes_downloaded: 0,
                        total_bytes: 0,
                        files_done: 1,
                        files_total: 1,
                        complete: true,
                        error: None,
                    },
                    Err(e) => crate::s3_client::DownloadMsg {
                        bytes_downloaded: 0,
                        total_bytes: 0,
                        files_done: 0,
                        files_total: 1,
                        complete: true,
                        error: Some(e.to_string()),
                    },
                };
                let _ = tx.send(msg).await;
            });
            self.download_handle = Some(handle);
        }
    }

    /// Non-blocking drain of download progress channel. Call every tick.
    pub fn drain_download(&mut self) {
        let rx = match &mut self.download_rx {
            Some(rx) => rx,
            None => return,
        };

        let elapsed_secs = self
            .download_started_at
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(1.0)
            .max(0.01);

        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    if msg.complete {
                        if let Some(ref mut progress) = self.download_progress {
                            progress.complete = true;
                            progress.error = msg.error;
                            if progress.error.is_none() {
                                self.status_message = Some(format!(
                                    "Downloaded {}",
                                    progress.filename
                                ));
                            } else {
                                self.error = Some(format!(
                                    "Download failed: {}",
                                    progress.error.as_deref().unwrap_or("unknown")
                                ));
                            }
                        }
                        self.download_rx = None;
                        self.download_handle = None;
                        self.download_started_at = None;
                        // Keep progress briefly for display, clear on next action
                        return;
                    }
                    if let Some(ref mut progress) = self.download_progress {
                        progress.bytes_downloaded = msg.bytes_downloaded;
                        progress.total_bytes = msg.total_bytes;
                        progress.files_done = msg.files_done;
                        progress.files_total = msg.files_total;
                        progress.speed_bps = msg.bytes_downloaded as f64 / elapsed_secs;
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if let Some(ref mut progress) = self.download_progress {
                        if !progress.complete {
                            progress.complete = true;
                        }
                    }
                    self.download_rx = None;
                    break;
                }
            }
        }
    }
}
