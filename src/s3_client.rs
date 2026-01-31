use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_sdk_s3::Client;
use tokio::sync::{mpsc, Semaphore};

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

/// Progress updates sent from download tasks to the UI.
#[derive(Clone)]
pub struct DownloadMsg {
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub files_done: usize,
    pub files_total: usize,
    pub complete: bool,
    pub error: Option<String>,
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

    /// Download a byte range of an object into memory.
    /// Uses the HTTP Range header to avoid downloading the entire file.
    pub async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>> {
        let range = format!("bytes={}-{}", start, end.saturating_sub(1));
        let output = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .range(range)
            .send()
            .await?;
        let bytes = output.body.collect().await?.into_bytes().to_vec();
        Ok(bytes)
    }

    /// Generate a presigned GET URL for an object (for ffmpeg streaming).
    /// The URL is valid for 1 hour and allows ffmpeg to seek within the file.
    pub async fn presign_get_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<String> {
        use aws_sdk_s3::presigning::PresigningConfig;
        use std::time::Duration;

        let presigning_config = PresigningConfig::builder()
            .expires_in(Duration::from_secs(3600))
            .build()?;

        let presigned = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(presigning_config)
            .await?;

        Ok(presigned.uri().to_string())
    }

    /// Download a single object to a local file, reporting progress.
    pub async fn download_object(
        &self,
        bucket: &str,
        key: &str,
        dest: &Path,
        tx: &mpsc::Sender<DownloadMsg>,
    ) -> Result<()> {
        // Get object size first via head
        let head = self.client.head_object().bucket(bucket).key(key).send().await?;
        let total_bytes = head.content_length().unwrap_or(0) as u64;

        // Start download
        let output = self.client.get_object().bucket(bucket).key(key).send().await?;
        let mut body = output.body.into_async_read();

        // Ensure parent directory exists
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = tokio::fs::File::create(dest).await?;
        let mut downloaded: u64 = 0;
        let mut last_report = Instant::now();
        let mut buf = vec![0u8; 8192];

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        loop {
            let n = body.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).await?;
            downloaded += n as u64;

            // Report progress every 100ms or at completion
            if last_report.elapsed().as_millis() >= 100 || downloaded == total_bytes {
                let _ = tx
                    .send(DownloadMsg {
                        bytes_downloaded: downloaded,
                        total_bytes,
                        files_done: 0,
                        files_total: 1,
                        complete: false,
                        error: None,
                    })
                    .await;
                last_report = Instant::now();
            }
        }

        file.flush().await?;
        Ok(())
    }

    /// Download all objects under `prefix` to a local directory with concurrency.
    /// Reports aggregate progress through the channel.
    pub async fn download_prefix(
        &self,
        bucket: &str,
        prefix: &str,
        dest_dir: &Path,
        tx: mpsc::Sender<DownloadMsg>,
        concurrency: usize,
    ) -> Result<()> {
        // First, list all objects under the prefix
        let mut all_keys: Vec<(String, u64)> = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut builder = self.client.list_objects_v2().bucket(bucket).prefix(prefix);
            if let Some(token) = &continuation_token {
                builder = builder.continuation_token(token);
            }
            let output = builder.send().await?;
            for obj in output.contents() {
                if let Some(key) = obj.key() {
                    if key.ends_with('/') {
                        continue;
                    }
                    all_keys.push((key.to_string(), obj.size().unwrap_or(0) as u64));
                }
            }
            match output.next_continuation_token() {
                Some(token) => continuation_token = Some(token.to_string()),
                None => break,
            }
        }

        let files_total = all_keys.len();
        let total_bytes: u64 = all_keys.iter().map(|(_, s)| s).sum();
        let bytes_downloaded = Arc::new(AtomicU64::new(0));
        let files_done = Arc::new(AtomicUsize::new(0));
        let semaphore = Arc::new(Semaphore::new(concurrency));

        let mut handles = Vec::new();

        for (key, _size) in &all_keys {
            let permit = semaphore.clone().acquire_owned().await?;
            let client = self.client.clone();
            let bucket = bucket.to_string();
            let key = key.clone();
            let rel_path = key.strip_prefix(prefix).unwrap_or(&key).to_string();
            let dest = dest_dir.join(&rel_path);
            let bytes_downloaded = bytes_downloaded.clone();
            let files_done = files_done.clone();
            let tx = tx.clone();

            let handle = tokio::spawn(async move {
                let result: Result<()> = async {
                    let output = client.get_object().bucket(&bucket).key(&key).send().await?;
                    let mut body = output.body.into_async_read();

                    if let Some(parent) = dest.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }

                    let mut file = tokio::fs::File::create(&dest).await?;
                    let mut buf = vec![0u8; 8192];
                    let mut last_report = Instant::now();

                    use tokio::io::{AsyncReadExt, AsyncWriteExt};

                    loop {
                        let n = body.read(&mut buf).await?;
                        if n == 0 {
                            break;
                        }
                        file.write_all(&buf[..n]).await?;
                        let prev = bytes_downloaded.fetch_add(n as u64, Ordering::Relaxed);

                        if last_report.elapsed().as_millis() >= 200 {
                            let _ = tx
                                .send(DownloadMsg {
                                    bytes_downloaded: prev + n as u64,
                                    total_bytes,
                                    files_done: files_done.load(Ordering::Relaxed),
                                    files_total,
                                    complete: false,
                                    error: None,
                                })
                                .await;
                            last_report = Instant::now();
                        }
                    }
                    file.flush().await?;
                    files_done.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                .await;

                drop(permit);
                result
            });
            handles.push(handle);
        }

        // Wait for all downloads
        let mut errors = Vec::new();
        for handle in handles {
            if let Ok(Err(e)) = handle.await {
                errors.push(e.to_string());
            }
        }

        if !errors.is_empty() {
            anyhow::bail!("{} files failed: {}", errors.len(), errors[0]);
        }

        Ok(())
    }
}
