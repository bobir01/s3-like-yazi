use tokio::sync::mpsc;

use crate::s3_client::IndexMsg;

use super::App;

impl App {
    pub fn index_object_count(&self) -> usize {
        self.search_pool.len()
    }

    pub(crate) fn start_indexing(&mut self, remote: &str, bucket: &str) {
        let new_key = (remote.to_string(), bucket.to_string());
        if self.index_key.as_ref() == Some(&new_key) {
            return;
        }

        self.cancel_indexing();

        let (tx, rx) = mpsc::channel(64);
        let client = self.clients[remote].clone();
        let bucket_owned = bucket.to_string();

        let handle = tokio::spawn(async move {
            client.stream_all_objects(&bucket_owned, tx).await;
        });

        self.index_rx = Some(rx);
        self.index_handle = Some(handle);
        self.index_key = Some(new_key);
        self.search_pool.clear();
        self.index_complete = false;
    }

    pub(crate) fn cancel_indexing(&mut self) {
        if let Some(handle) = self.index_handle.take() {
            handle.abort();
        }
        self.index_rx = None;
        self.index_key = None;
        self.index_complete = false;
        self.search_pool.clear();
    }

    pub fn drain_index(&mut self) {
        let rx = match &mut self.index_rx {
            Some(rx) => rx,
            None => return,
        };

        let mut got_new = false;

        loop {
            match rx.try_recv() {
                Ok(IndexMsg::Batch(batch)) => {
                    self.search_pool.extend(batch);
                    got_new = true;
                }
                Ok(IndexMsg::Done) => {
                    self.index_complete = true;
                    break;
                }
                Ok(IndexMsg::Error(e)) => {
                    if self.search_active {
                        self.error = Some(format!("Index error: {}", e));
                    }
                    self.index_complete = true;
                    break;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.index_complete = true;
                    break;
                }
            }
        }

        if got_new && self.search_active && self.index_key.is_some() {
            self.update_search_filter();
        }
    }
}
