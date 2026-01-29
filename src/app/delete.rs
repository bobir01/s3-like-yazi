use super::{App, DeleteConfirm, Entry, Location};

impl App {
    pub fn request_delete(&mut self) {
        if self.search_active {
            return;
        }
        self.status_message = None;
        if let Some(idx) = self.browser_state.selected() {
            if idx >= self.entries.len() {
                return;
            }
            match &self.entries[idx] {
                Entry::Object(obj) => {
                    self.confirm_delete = Some(DeleteConfirm {
                        display_name: obj.display_name.clone(),
                        key: obj.key.clone(),
                        is_dir: obj.is_dir,
                        selected_yes: false,
                    });
                }
                Entry::Bucket(_) => {
                    self.error = Some("Bucket deletion is not supported".to_string());
                }
            }
        }
    }

    pub fn toggle_delete_confirm(&mut self) {
        if let Some(ref mut confirm) = self.confirm_delete {
            confirm.selected_yes = !confirm.selected_yes;
        }
    }

    pub async fn confirm_delete_yes(&mut self) {
        let confirm = match self.confirm_delete.take() {
            Some(v) => v,
            None => return,
        };

        if let Location::ObjectList {
            ref remote,
            ref bucket,
            ..
        } = self.location
        {
            let remote = remote.clone();
            let bucket = bucket.clone();
            let client = match self.clients.get(&remote) {
                Some(c) => c.clone(),
                None => {
                    self.error = Some("Not connected to remote".to_string());
                    return;
                }
            };

            if confirm.is_dir {
                match client.delete_prefix(&bucket, &confirm.key).await {
                    Ok(count) => {
                        self.entries.retain(|e| e.key() != confirm.key);
                        self.search_pool
                            .retain(|o| !o.key.starts_with(&confirm.key));
                        self.fix_selection();
                        self.metadata = None;
                        self.status_message = Some(format!(
                            "Deleted {} objects from {}",
                            count, confirm.display_name
                        ));
                    }
                    Err(e) => {
                        self.error = Some(format!("Delete failed: {}", e));
                    }
                }
            } else {
                match client.delete_object(&bucket, &confirm.key).await {
                    Ok(()) => {
                        self.entries.retain(|e| e.key() != confirm.key);
                        self.search_pool.retain(|o| o.key != confirm.key);
                        self.fix_selection();
                        self.metadata = None;
                        self.status_message =
                            Some(format!("Deleted {}", confirm.display_name));
                    }
                    Err(e) => {
                        self.error = Some(format!("Delete failed: {}", e));
                    }
                }
            }
        }
    }
}
