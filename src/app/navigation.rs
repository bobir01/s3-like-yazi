use super::{parent_prefix, App, Entry, Location, Pane};

impl App {
    pub fn move_up(&mut self) {
        match self.pane {
            Pane::Remotes => {
                let i = self.remote_state.selected().unwrap_or(0);
                if i > 0 {
                    self.remote_state.select(Some(i - 1));
                }
            }
            Pane::Browser => {
                let i = self.browser_state.selected().unwrap_or(0);
                if i > 0 {
                    self.browser_state.select(Some(i - 1));
                }
                self.metadata = None;
                self.preview.clear();
            }
            Pane::LocalFs => self.local_move_up(),
        }
    }

    pub fn move_down(&mut self) {
        match self.pane {
            Pane::Remotes => {
                let i = self.remote_state.selected().unwrap_or(0);
                if i + 1 < self.remotes.len() {
                    self.remote_state.select(Some(i + 1));
                }
            }
            Pane::Browser => {
                let i = self.browser_state.selected().unwrap_or(0);
                if i + 1 < self.entries.len() {
                    self.browser_state.select(Some(i + 1));
                }
                self.metadata = None;
                self.preview.clear();
            }
            Pane::LocalFs => self.local_move_down(),
        }
    }

    pub fn switch_pane(&mut self) {
        if self.search_active {
            return;
        }
        self.pane = match self.pane {
            Pane::Remotes => Pane::Browser,
            Pane::Browser => {
                if self.download_mode {
                    Pane::LocalFs
                } else {
                    Pane::Remotes
                }
            }
            Pane::LocalFs => Pane::Remotes,
        };
    }

    pub async fn select(&mut self) {
        self.error = None;
        self.status_message = None;
        match self.pane {
            Pane::Remotes => {
                if let Some(i) = self.remote_state.selected() {
                    let alias = self.remotes[i].clone();
                    self.enter_remote(&alias).await;
                }
            }
            Pane::Browser => {
                if let Some(idx) = self.browser_state.selected() {
                    if idx >= self.entries.len() {
                        return;
                    }
                    let entry = self.entries[idx].clone();

                    if self.search_active {
                        self.finish_search_select(entry).await;
                        return;
                    }

                    match entry {
                        Entry::Bucket(b) => {
                            if let Location::BucketList { ref remote } = self.location {
                                let remote = remote.clone();
                                self.enter_bucket(&remote, &b.name).await;
                            }
                        }
                        Entry::Object(obj) => {
                            if obj.is_dir {
                                if let Location::ObjectList {
                                    ref remote,
                                    ref bucket,
                                    ..
                                } = self.location
                                {
                                    let remote = remote.clone();
                                    let bucket = bucket.clone();
                                    self.enter_prefix(&remote, &bucket, &obj.key).await;
                                }
                            } else {
                                if let Location::ObjectList {
                                    ref remote,
                                    ref bucket,
                                    ..
                                } = self.location
                                {
                                    let remote = remote.clone();
                                    let bucket = bucket.clone();
                                    self.fetch_metadata(&remote, &bucket, &obj.key).await;
                                }
                            }
                        }
                    }
                }
            }
            Pane::LocalFs => {}
        }
    }

    pub async fn go_back(&mut self) {
        if self.search_active {
            self.cancel_search();
            return;
        }
        self.error = None;
        self.status_message = None;
        self.metadata = None;
        self.preview.clear();
        match self.location.clone() {
            Location::RemoteList => {}
            Location::BucketList { .. } => {
                self.cancel_indexing();
                self.location = Location::RemoteList;
                self.entries.clear();
                self.browser_state.select(None);
                self.pane = Pane::Remotes;
            }
            Location::ObjectList {
                remote,
                bucket,
                prefix,
            } => {
                if prefix.is_empty() {
                    self.enter_remote(&remote).await;
                } else {
                    let parent = parent_prefix(&prefix);
                    self.enter_prefix(&remote, &bucket, &parent).await;
                }
            }
        }
    }

    pub async fn refresh(&mut self) {
        self.error = None;
        if self.search_active {
            self.cancel_search();
        }
        match self.location.clone() {
            Location::RemoteList => {}
            Location::BucketList { remote } => {
                self.enter_remote(&remote).await;
            }
            Location::ObjectList {
                remote,
                bucket,
                prefix,
            } => {
                self.cancel_indexing();
                self.enter_prefix(&remote, &bucket, &prefix).await;
            }
        }
    }

    // ── S3 operations ───────────────────────────────────────────

    pub(crate) async fn enter_remote(&mut self, alias: &str) {
        if let Err(e) = self.ensure_client(alias) {
            self.error = Some(format!("Connection failed: {}", e));
            return;
        }

        let client = self.clients[alias].clone();
        match client.list_buckets().await {
            Ok(buckets) => {
                self.entries = buckets.into_iter().map(Entry::Bucket).collect();
                self.location = Location::BucketList {
                    remote: alias.to_string(),
                };
                self.browser_state.select(if self.entries.is_empty() {
                    None
                } else {
                    Some(0)
                });
                self.pane = Pane::Browser;
            }
            Err(e) => {
                let err_str = format!("{}", e);
                let is_permission = err_str.contains("Access Denied")
                    || err_str.contains("AccessDenied")
                    || err_str.contains("Forbidden")
                    || err_str.contains("403");
                if is_permission {
                    self.show_bucket_input(alias);
                    self.error = Some(
                        "ListBuckets denied — enter a bucket name manually".to_string(),
                    );
                } else {
                    self.error = Some(format!("Failed to list buckets: {}", e));
                }
            }
        }
    }

    pub(crate) fn show_bucket_input(&mut self, alias: &str) {
        self.bucket_input_active = true;
        self.bucket_input.clear();
        self.bucket_input_remote = Some(alias.to_string());
        self.location = Location::BucketList {
            remote: alias.to_string(),
        };
        self.entries.clear();
        self.browser_state.select(None);
        self.pane = Pane::Browser;
    }

    pub async fn submit_bucket_input(&mut self) {
        let bucket = self.bucket_input.trim().to_string();
        if bucket.is_empty() {
            return;
        }
        let remote = match self.bucket_input_remote.clone() {
            Some(r) => r,
            None => return,
        };
        self.bucket_input_active = false;
        self.bucket_input.clear();
        self.bucket_input_remote = None;
        self.error = None;
        self.enter_bucket(&remote, &bucket).await;
    }

    pub fn cancel_bucket_input(&mut self) {
        self.bucket_input_active = false;
        self.bucket_input.clear();
        self.bucket_input_remote = None;
        self.error = None;
        self.location = Location::RemoteList;
        self.entries.clear();
        self.browser_state.select(None);
        self.pane = Pane::Remotes;
    }

    pub(crate) async fn enter_bucket(&mut self, remote: &str, bucket: &str) {
        self.enter_prefix(remote, bucket, "").await;
    }

    pub(crate) async fn enter_prefix(&mut self, remote: &str, bucket: &str, prefix: &str) {
        let client = match self.clients.get(remote) {
            Some(c) => c.clone(),
            None => {
                self.error = Some("Not connected to remote".to_string());
                return;
            }
        };

        match client.list_objects(bucket, prefix).await {
            Ok(objects) => {
                self.entries = objects.into_iter().map(Entry::Object).collect();
                self.location = Location::ObjectList {
                    remote: remote.to_string(),
                    bucket: bucket.to_string(),
                    prefix: prefix.to_string(),
                };
                self.browser_state.select(if self.entries.is_empty() {
                    None
                } else {
                    Some(0)
                });

                self.start_indexing(remote, bucket);
            }
            Err(e) => {
                self.error = Some(format!("Failed to list objects: {}", e));
            }
        }
    }

    async fn fetch_metadata(&mut self, remote: &str, bucket: &str, key: &str) {
        let client = match self.clients.get(remote) {
            Some(c) => c.clone(),
            None => {
                self.error = Some("Not connected to remote".to_string());
                return;
            }
        };

        match client.head_object(bucket, key).await {
            Ok(meta) => {
                self.metadata = Some(meta);
            }
            Err(e) => {
                self.error = Some(format!("Failed to get metadata: {}", e));
            }
        }
    }
}
