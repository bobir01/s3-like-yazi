use super::{parent_prefix, App, Entry, Location, Pane};

impl App {
    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
        self.pre_search_selection = self.browser_state.selected();
        self.saved_entries = self.entries.clone();
        self.saved_location = Some(self.location.clone());
        self.pane = Pane::Browser;

        match &self.location {
            Location::ObjectList { remote, bucket, .. } => {
                self.search_context = Some((remote.clone(), bucket.clone()));
                self.entries = self
                    .search_pool
                    .iter()
                    .cloned()
                    .map(Entry::Object)
                    .collect();
                self.browser_state.select(if self.entries.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Location::BucketList { remote } => {
                self.search_context = Some((remote.clone(), String::new()));
            }
            Location::RemoteList => {
                self.search_context = None;
            }
        }
    }

    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.entries = std::mem::take(&mut self.saved_entries);
        if let Some(loc) = self.saved_location.take() {
            self.location = loc;
        }
        self.browser_state
            .select(self.pre_search_selection.take());
        self.search_context = None;
    }

    pub fn search_input(&mut self, c: char) {
        self.search_query.push(c);
        self.update_search_filter();
    }

    pub fn search_backspace(&mut self) {
        self.search_query.pop();
        self.update_search_filter();
    }

    pub(crate) fn update_search_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        let prev_sel = self.browser_state.selected();

        if self.index_key.is_some() {
            if query.is_empty() {
                self.entries = self
                    .search_pool
                    .iter()
                    .cloned()
                    .map(Entry::Object)
                    .collect();
            } else {
                self.entries = self
                    .search_pool
                    .iter()
                    .filter(|obj| obj.key.to_lowercase().contains(&query))
                    .cloned()
                    .map(Entry::Object)
                    .collect();
            }
        } else {
            if query.is_empty() {
                self.entries = self.saved_entries.clone();
            } else {
                self.entries = self
                    .saved_entries
                    .iter()
                    .filter(|e| e.name().to_lowercase().contains(&query))
                    .cloned()
                    .collect();
            }
        }

        if self.entries.is_empty() {
            self.browser_state.select(None);
        } else {
            let sel = prev_sel.unwrap_or(0).min(self.entries.len() - 1);
            self.browser_state.select(Some(sel));
        }
    }

    pub(crate) async fn finish_search_select(&mut self, entry: Entry) {
        self.search_active = false;
        self.search_query.clear();

        if self.index_key.is_some() {
            let target_key = entry.key().to_string();

            if let Some((remote, bucket)) = self.search_context.take() {
                let parent = parent_prefix(&target_key);
                self.enter_prefix(&remote, &bucket, &parent).await;

                if let Some(pos) = self.entries.iter().position(|e| e.key() == target_key) {
                    self.browser_state.select(Some(pos));
                }
            }
            self.saved_entries.clear();
            self.saved_location = None;
        } else {
            let target_name = entry.name().to_string();
            self.entries = std::mem::take(&mut self.saved_entries);
            if let Some(loc) = self.saved_location.take() {
                self.location = loc;
            }
            if let Some(pos) = self.entries.iter().position(|e| e.name() == target_name) {
                self.browser_state.select(Some(pos));
            }
            self.search_context = None;
        }
    }
}
