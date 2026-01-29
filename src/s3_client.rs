use std::collections::HashMap;

use anyhow::Result;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_sdk_s3::Client;

#[derive(Clone)]
pub struct S3Client {
    client: Client,
    #[allow(dead_code)]
    pub alias: String,
}

#[derive(Debug, Clone)]
pub struct BucketInfo {
    pub name: String,
    pub creation_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectEntry {
    pub key: String,
    pub display_name: String,
    pub size: i64,
    pub last_modified: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ObjectMetadata {
    pub key: String,
    pub size: i64,
    pub content_type: Option<String>,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
    pub version_id: Option<String>,
    pub storage_class: Option<String>,
    pub user_metadata: HashMap<String, String>,
    pub content_encoding: Option<String>,
    pub cache_control: Option<String>,
}

/// Messages sent from the background indexing task to the UI.
pub enum IndexMsg {
    Batch(Vec<ObjectEntry>),
    Done,
    Error(String),
}

fn format_aws_datetime(dt: &aws_sdk_s3::primitives::DateTime) -> String {
    chrono::DateTime::from_timestamp(dt.secs(), dt.subsec_nanos())
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

impl S3Client {
    pub fn new(alias: &str, url: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        let credentials =
            Credentials::new(access_key, secret_key, None, None, "yazi-like-s3");

        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .endpoint_url(url)
            .region(Region::new("us-east-1"))
            .credentials_provider(credentials)
            .force_path_style(true)
            .build();

        let client = Client::from_conf(config);

        Ok(Self {
            client,
            alias: alias.to_string(),
        })
    }

    pub async fn list_buckets(&self) -> Result<Vec<BucketInfo>> {
        let output = self.client.list_buckets().send().await?;
        let buckets = output
            .buckets()
            .iter()
            .filter_map(|b| {
                b.name().map(|name| BucketInfo {
                    name: name.to_string(),
                    creation_date: b.creation_date().map(format_aws_datetime),
                })
            })
            .collect();
        Ok(buckets)
    }

    pub async fn list_objects(&self, bucket: &str, prefix: &str) -> Result<Vec<ObjectEntry>> {
        let mut builder = self.client.list_objects_v2().bucket(bucket).delimiter("/");

        if !prefix.is_empty() {
            builder = builder.prefix(prefix);
        }

        let output = builder.send().await?;
        let mut entries = Vec::new();

        // Directories (common prefixes) first
        for cp in output.common_prefixes() {
            if let Some(p) = cp.prefix() {
                let display = p.strip_prefix(prefix).unwrap_or(p);
                let display = display.trim_end_matches('/');
                if !display.is_empty() {
                    entries.push(ObjectEntry {
                        key: p.to_string(),
                        display_name: display.to_string(),
                        size: 0,
                        last_modified: None,
                        is_dir: true,
                    });
                }
            }
        }

        // Files
        for obj in output.contents() {
            if let Some(key) = obj.key() {
                // Skip the prefix itself if returned as an object
                if key == prefix {
                    continue;
                }
                let display = key.strip_prefix(prefix).unwrap_or(key);
                entries.push(ObjectEntry {
                    key: key.to_string(),
                    display_name: display.to_string(),
                    size: obj.size().unwrap_or(0),
                    last_modified: obj.last_modified().map(format_aws_datetime),
                    is_dir: false,
                });
            }
        }

        Ok(entries)
    }

    /// Stream ALL objects in a bucket to a channel, page by page.
    /// Runs as a background task — sends batches so the UI stays responsive.
    pub async fn stream_all_objects(
        &self,
        bucket: &str,
        tx: tokio::sync::mpsc::Sender<IndexMsg>,
    ) {
        let mut continuation_token: Option<String> = None;

        loop {
            let mut builder = self.client.list_objects_v2().bucket(bucket);

            if let Some(token) = &continuation_token {
                builder = builder.continuation_token(token);
            }

            match builder.send().await {
                Ok(output) => {
                    let mut batch = Vec::new();
                    for obj in output.contents() {
                        if let Some(key) = obj.key() {
                            if key.ends_with('/') {
                                continue;
                            }
                            batch.push(ObjectEntry {
                                key: key.to_string(),
                                display_name: key.to_string(),
                                size: obj.size().unwrap_or(0),
                                last_modified: obj.last_modified().map(format_aws_datetime),
                                is_dir: false,
                            });
                        }
                    }
                    if !batch.is_empty() {
                        if tx.send(IndexMsg::Batch(batch)).await.is_err() {
                            return; // receiver dropped, stop
                        }
                    }
                    match output.next_continuation_token() {
                        Some(token) => continuation_token = Some(token.to_string()),
                        None => break,
                    }
                }
                Err(e) => {
                    let _ = tx.send(IndexMsg::Error(e.to_string())).await;
                    return;
                }
            }
        }
        let _ = tx.send(IndexMsg::Done).await;
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await?;
        Ok(())
    }

    /// Recursively delete all objects under `prefix`. Returns the count deleted.
    pub async fn delete_prefix(&self, bucket: &str, prefix: &str) -> Result<usize> {
        let mut deleted = 0usize;
        let mut continuation_token: Option<String> = None;

        loop {
            let mut builder = self.client.list_objects_v2().bucket(bucket).prefix(prefix);
            if let Some(token) = &continuation_token {
                builder = builder.continuation_token(token);
            }

            let output = builder.send().await?;
            let keys: Vec<String> = output
                .contents()
                .iter()
                .filter_map(|obj| obj.key().map(|k| k.to_string()))
                .collect();

            // Delete in batches of 1000 (S3 limit)
            for chunk in keys.chunks(1000) {
                let objects: Vec<ObjectIdentifier> = chunk
                    .iter()
                    .map(|k| ObjectIdentifier::builder().key(k).build().unwrap())
                    .collect();
                let delete = Delete::builder()
                    .set_objects(Some(objects))
                    .quiet(true)
                    .build()?;
                self.client
                    .delete_objects()
                    .bucket(bucket)
                    .delete(delete)
                    .send()
                    .await?;
                deleted += chunk.len();
            }

            match output.next_continuation_token() {
                Some(token) => continuation_token = Some(token.to_string()),
                None => break,
            }
        }

        Ok(deleted)
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMetadata> {
        let output = self.client.head_object().bucket(bucket).key(key).send().await?;

        Ok(ObjectMetadata {
            key: key.to_string(),
            size: output.content_length().unwrap_or(0),
            content_type: output.content_type().map(|s| s.to_string()),
            last_modified: output.last_modified().map(format_aws_datetime),
            etag: output.e_tag().map(|s| s.to_string()),
            version_id: output.version_id().map(|s| s.to_string()),
            storage_class: output.storage_class().map(|s| s.as_str().to_string()),
            user_metadata: output
                .metadata()
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default(),
            content_encoding: output.content_encoding().map(|s| s.to_string()),
            cache_control: output.cache_control().map(|s| s.to_string()),
        })
    }
}
