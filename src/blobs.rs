//! Blob bodies that do not belong inline in SQLite.
//!
//! Keys are the SHA-256 of the content, so a write is idempotent and an asset
//! repeated across revisions is stored once. Revisions are permanent, so
//! nothing here is ever deleted: no reference counting and no collector.
//!
//! Every writer stores the object before the row stops carrying the bytes. A
//! crash between the two leaves an orphan object, which costs a few kilobytes,
//! rather than a row pointing at nothing, which loses a document.

use std::sync::Arc;

use object_store::aws::AmazonS3Builder;
use object_store::{ObjectStore, ObjectStoreExt};
use object_store::path::Path;
use sha2::{Digest, Sha256};

use crate::config::Bucket;

/// Inline bodies below this. It sits above the largest blob stored when this
/// was written, so it only ever acts on something genuinely new.
pub const INLINE_LIMIT: i64 = 256 * 1024;

#[derive(Clone)]
pub struct Blobs(Arc<dyn ObjectStore>);

impl Blobs {
    pub fn new(bucket: &Bucket) -> Result<Blobs, String> {
        let store = AmazonS3Builder::new()
            .with_bucket_name(&bucket.name)
            .with_endpoint(&bucket.endpoint)
            .with_region(&bucket.region)
            .with_access_key_id(&bucket.access_key_id)
            .with_secret_access_key(&bucket.secret_access_key)
            // a Ceph gateway serves one host, not bucket.host
            .with_virtual_hosted_style_request(false)
            .with_allow_http(true)
            .build()
            .map_err(|error| format!("cannot reach the bucket: {error}"))?;
        Ok(Blobs(Arc::new(store)))
    }

    /// A store with the same semantics and no network, so the split storage
    /// read paths are exercised by the suite rather than only in production.
    #[cfg(test)]
    pub fn in_memory() -> Blobs {
        Blobs(Arc::new(object_store::memory::InMemory::new()))
    }

    pub async fn put(&self, content: &[u8]) -> Result<String, String> {
        let key = format!("blobs/{:x}", Sha256::digest(content));
        self.0
            .put(&Path::from(key.as_str()), content.to_vec().into())
            .await
            .map_err(|error| format!("cannot store {key}: {error}"))?;
        Ok(key)
    }

    pub async fn get(&self, key: &str) -> Result<Vec<u8>, String> {
        let object = self
            .0
            .get(&Path::from(key))
            .await
            .map_err(|error| format!("cannot read {key}: {error}"))?;
        let bytes = object
            .bytes()
            .await
            .map_err(|error| format!("cannot read {key}: {error}"))?;
        Ok(bytes.to_vec())
    }

    /// Removes an object. Only for a body no row points at any more: keys are
    /// content addressed, so two documents holding identical bytes share one
    /// object and the caller has to establish that before calling this.
    pub async fn delete(&self, key: &str) -> Result<(), String> {
        self.0
            .delete(&Path::from(key))
            .await
            .map_err(|error| format!("cannot delete {key}: {error}"))
    }

    /// Round trips a small object so a misconfigured bucket stops the server
    /// at startup instead of at the first push.
    pub async fn check(&self) -> Result<(), String> {
        let key = self.put(b"plan-env-md").await?;
        self.get(&key).await.map(|_| ())
    }
}

/// The body of a stored file, wherever it lives. Callers that already hold a
/// row select both columns and hand them here.
pub async fn resolve(
    blobs: Option<&Blobs>,
    content: Option<Vec<u8>>,
    object_key: Option<String>,
) -> Result<Vec<u8>, String> {
    match (content, object_key) {
        (Some(content), _) => Ok(content),
        (None, Some(key)) => match blobs {
            Some(blobs) => blobs.get(&key).await,
            None => Err(format!("{key} needs a bucket, and none is configured")),
        },
        (None, None) => Err("the row carries neither content nor a key".to_string()),
    }
}
