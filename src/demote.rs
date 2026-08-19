//! Moves cold bodies out of SQLite and into the bucket.
//!
//! Cold is not measured, it is derived. A document is read at its latest
//! revision, and every older one is reachable only through a pinned /rev/N
//! URL, so a revision goes cold the moment the next one is pushed. That fact
//! is already in `revisions.revision`, which costs nothing to query and needs
//! no write on the read path to maintain.
//!
//! The sweep runs outside the push transaction on purpose. A push should not
//! wait on the bucket, and a SQLite write lock should never span a network
//! call.

use std::time::Duration;

use sqlx::SqlitePool;

use crate::blobs::Blobs;

const IDLE_POLL: Duration = Duration::from_secs(60);
/// One body at a time, like the preview worker: this is a single user instance
/// and a second concurrent upload buys nothing but memory.
const BATCH: i64 = 1;

pub fn spawn(pool: SqlitePool, blobs: Option<Blobs>) {
    let Some(blobs) = blobs else {
        return;
    };

    tokio::spawn(async move {
        loop {
            match sweep(&pool, &blobs, BATCH).await {
                Ok(0) => tokio::time::sleep(IDLE_POLL).await,
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "demote sweep failed");
                    tokio::time::sleep(IDLE_POLL).await;
                }
            }
        }
    });
}

/// Runs until nothing is left to move. This is the `demote` subcommand, used
/// to drain a backlog in one go rather than at the sweep's pace.
pub async fn sweep_all(pool: &SqlitePool, blobs: Option<&Blobs>) -> Result<u64, String> {
    let Some(blobs) = blobs else {
        return Err("S3_BUCKET is unset, so there is nowhere to move bodies to".to_string());
    };

    let mut total = 0;
    loop {
        let moved = sweep(pool, blobs, 64).await?;
        if moved == 0 {
            return Ok(total);
        }
        total += moved;
    }
}

/// Returns how many bodies moved, so the caller can tell a drained queue from
/// a busy one.
async fn sweep(pool: &SqlitePool, blobs: &Blobs, limit: i64) -> Result<u64, String> {
    let mut moved = demote_files(pool, blobs, limit).await?;
    moved += demote_previews(pool, blobs, limit).await?;
    Ok(moved)
}

/// A revision file is cold when a higher revision of the same document exists.
/// Large bodies are taken whatever their revision, which is the size backstop:
/// without it a fresh multi-megabyte asset would sit inline until its own
/// revision is superseded, which may be never.
async fn demote_files(pool: &SqlitePool, blobs: &Blobs, limit: i64) -> Result<u64, String> {
    let rows = sqlx::query!(
        r#"SELECT f.id as "id!: i64", f.content as "content!: Vec<u8>"
           FROM revision_files f
           JOIN revisions r ON r.id = f.revision_id
           WHERE f.content IS NOT NULL
             AND (f.size_bytes > ?
                  OR r.revision < (SELECT MAX(revision) FROM revisions
                                   WHERE document_id = r.document_id))
           LIMIT ?"#,
        crate::blobs::INLINE_LIMIT,
        limit
    )
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?;

    let mut moved = 0;
    for row in rows {
        let key = blobs.put(&row.content).await?;
        sqlx::query!(
            "UPDATE revision_files SET content = NULL, object_key = ? WHERE id = ?",
            key,
            row.id
        )
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
        moved += 1;
    }
    Ok(moved)
}

/// Every stored preview is eligible, not only the cold ones. A thumbnail is
/// derived data that a re-render can rebuild, so there is nothing to protect
/// by keeping it close.
async fn demote_previews(pool: &SqlitePool, blobs: &Blobs, limit: i64) -> Result<u64, String> {
    let rows = sqlx::query!(
        r#"SELECT revision_id as "revision_id!: i64", scheme as "scheme!: String",
                  image as "image!: Vec<u8>"
           FROM revision_previews
           WHERE image IS NOT NULL AND object_key IS NULL AND status = 'ready'
           LIMIT ?"#,
        limit
    )
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?;

    let mut moved = 0;
    for row in rows {
        let key = blobs.put(&row.image).await?;
        sqlx::query!(
            "UPDATE revision_previews SET image = NULL, object_key = ?
             WHERE revision_id = ? AND scheme = ?",
            key,
            row.revision_id,
            row.scheme
        )
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
        moved += 1;
    }
    Ok(moved)
}
