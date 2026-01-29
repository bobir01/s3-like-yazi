mod delete;
mod indexing;
mod navigation;
mod search;

use std::collections::HashMap;

use ratatui::widgets::TableState;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::credentials::McConfig;
use crate::s3_client::{BucketInfo, IndexMsg, ObjectEntry, ObjectMetadata, S3Client};

#[derive(Debug, Clone, PartialEq)]
pub enum Pane {
    Remotes,
    Browser,
}

#[derive(Debug, Clone)]
pub enum Location {
    RemoteList,
    BucketList {
        remote: String,
    },
    ObjectList {
        remote: String,
        bucket: String,
        prefix: String,
    },
}

#[derive(Debug, Clone)]
pub enum Entry {
    Bucket(BucketInfo),
    Object(ObjectEntry),
}

impl Entry {
    pub fn name(&self) -> &str {
        match self {
            Entry::Bucket(b) => &b.name,
            Entry::Object(o) => &o.display_name,
        }
    }

    pub fn key(&self) -> &str {
        match self {
            Entry::Bucket(b) => &b.name,
            Entry::Object(o) => &o.key,
        }
    }
}

pub struct DeleteConfirm {
    pub display_name: String,
    pub key: String,
    pub is_dir: bool,
    pub selected_yes: bool,
}

pub struct App {
    pub pane: Pane,
    pub remotes: Vec<String>,
    pub remote_state: ratatui::widgets::ListState,
    pub entries: Vec<Entry>,
    pub browser_state: TableState,
    pub location: Location,
    pub metadata: Option<ObjectMetadata>,
    pub error: Option<String>,
    pub should_quit: bool,
    pub show_help: bool,
    pub confirm_delete: Option<DeleteConfirm>,
    pub status_message: Option<String>,

    // Search state
    pub search_active: bool,
    pub search_query: String,
    pub(crate) search_pool: Vec<ObjectEntry>,
    pub(crate) saved_entries: Vec<Entry>,
    pub(crate) saved_location: Option<Location>,
    pub(crate) pre_search_selection: Option<usize>,
    pub(crate) search_context: Option<(String, String)>,

    // Background indexing
    pub(crate) index_rx: Option<mpsc::Receiver<IndexMsg>>,
    pub(crate) index_handle: Option<JoinHandle<()>>,
    pub index_complete: bool,
    pub(crate) index_key: Option<(String, String)>,

    pub(crate) config: McConfig,
    pub(crate) clients: HashMap<String, S3Client>,
}

impl App {
    pub fn new(config: McConfig) -> Self {
        let mut remotes: Vec<String> = config.aliases.keys().cloned().collect();
        remotes.sort();

        let mut remote_state = ratatui::widgets::ListState::default();
        if !remotes.is_empty() {
            remote_state.select(Some(0));
        }

        Self {
            pane: Pane::Remotes,
            remotes,
            remote_state,
            entries: Vec::new(),
            browser_state: TableState::default(),
            location: Location::RemoteList,
            metadata: None,
            error: None,
            should_quit: false,
            show_help: false,
            confirm_delete: None,
            status_message: None,
            search_active: false,
            search_query: String::new(),
            search_pool: Vec::new(),
            saved_entries: Vec::new(),
            saved_location: None,
            pre_search_selection: None,
            search_context: None,
            index_rx: None,
            index_handle: None,
            index_complete: false,
            index_key: None,
            config,
            clients: HashMap::new(),
        }
    }

    pub(crate) fn ensure_client(&mut self, alias: &str) -> anyhow::Result<()> {
        if !self.clients.contains_key(alias) {
            let alias_config = self
                .config
                .aliases
                .get(alias)
                .ok_or_else(|| anyhow::anyhow!("Unknown alias: {}", alias))?;
            let client = S3Client::new(
                alias,
                &alias_config.url,
                &alias_config.access_key,
                &alias_config.secret_key,
            )?;
            self.clients.insert(alias.to_string(), client);
        }
        Ok(())
    }

    pub(crate) fn fix_selection(&mut self) {
        if self.entries.is_empty() {
            self.browser_state.select(None);
        } else {
            let sel = self
                .browser_state
                .selected()
                .unwrap_or(0)
                .min(self.entries.len() - 1);
            self.browser_state.select(Some(sel));
        }
    }

    pub fn location_display(&self) -> String {
        match &self.location {
            Location::RemoteList => "Select a remote".to_string(),
            Location::BucketList { remote } => format!("{} /", remote),
            Location::ObjectList {
                remote,
                bucket,
                prefix,
            } => {
                if prefix.is_empty() {
                    format!("{} / {} /", remote, bucket)
                } else {
                    format!("{} / {} / {}", remote, bucket, prefix)
                }
            }
        }
    }
}

pub(crate) fn parent_prefix(key: &str) -> String {
    let trimmed = key.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(pos) => format!("{}/", &trimmed[..pos]),
        None => String::new(),
    }
}
