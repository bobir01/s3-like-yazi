use super::{App, LocalEntry};

impl App {
    pub fn list_local_dir(&mut self) {
        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&self.local_path) {
            for entry in read_dir.flatten() {
                let metadata = entry.metadata();
                let is_dir = metadata.as_ref().map_or(false, |m| m.is_dir());
                let size = metadata.as_ref().map_or(0, |m| m.len());
                let name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files
                if name.starts_with('.') {
                    continue;
                }

                entries.push(LocalEntry { name, is_dir, size });
            }
        }

        // Sort: directories first, then alphabetical
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        self.local_entries = entries;
        self.local_state.select(if self.local_entries.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    pub fn local_move_up(&mut self) {
        let i = self.local_state.selected().unwrap_or(0);
        if i > 0 {
            self.local_state.select(Some(i - 1));
        }
    }

    pub fn local_move_down(&mut self) {
        let i = self.local_state.selected().unwrap_or(0);
        if i + 1 < self.local_entries.len() {
            self.local_state.select(Some(i + 1));
        }
    }

    pub fn local_enter(&mut self) {
        if let Some(idx) = self.local_state.selected() {
            if idx < self.local_entries.len() && self.local_entries[idx].is_dir {
                let name = self.local_entries[idx].name.clone();
                self.local_path.push(&name);
                self.list_local_dir();
            }
        }
    }

    pub fn local_go_back(&mut self) {
        if let Some(parent) = self.local_path.parent() {
            self.local_path = parent.to_path_buf();
            self.list_local_dir();
        }
    }

    pub fn local_path_display(&self) -> String {
        let path = self.local_path.display().to_string();
        // Abbreviate home directory
        if let Some(home) = dirs::home_dir() {
            if let Some(rest) = path.strip_prefix(&home.display().to_string()) {
                return format!("~{}", rest);
            }
        }
        path
    }
}
