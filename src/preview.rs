//! Renders a thumbnail of each revision, in both colour schemes.
//!
//! Chromium navigates a loopback-only route that serves the revision without
//! the viewer overlay, so a thumbnail shows the document rather than the
//! document plus a pill cluster. Jobs are claimed one at a time: this is a
//! single-user instance and a second concurrent tab buys nothing but memory.

use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::{
    SetEmulatedMediaParams, SetEmulatedMediaParamsBuilder,
};
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams, Viewport,
};
use chromiumoxide::handler::viewport::Viewport as WindowViewport;
use futures::StreamExt;
use sqlx::SqlitePool;

/// 1280x800 captured at half scale: the shape of a browser window, small enough
/// to store per revision.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
const SCALE: f64 = 0.5;
const MAX_ATTEMPTS: i64 = 3;
/// Time for a pinned esm.sh module (Shiki, charts) to run after load.
const SETTLE: Duration = Duration::from_millis(900);
const IDLE_POLL: Duration = Duration::from_secs(5);

pub fn spawn(pool: SqlitePool, port: u16, blobs: Option<crate::blobs::Blobs>) {
    let Ok(chromium) = std::env::var("PREVIEW_CHROMIUM") else {
        tracing::info!("PREVIEW_CHROMIUM is unset; previews are off");
        return;
    };

    tokio::spawn(async move {
        // a crash mid render leaves a claimed row; give those back on boot
        let _ = sqlx::query!(
            "UPDATE revision_previews SET status = 'pending'
             WHERE status = 'running' AND attempts < ?",
            MAX_ATTEMPTS
        )
        .execute(&pool)
        .await;

        if let Err(error) = run(pool, port, chromium, blobs).await {
            tracing::error!(%error, "preview worker stopped");
        }
    });
}

async fn run(
    pool: SqlitePool,
    port: u16,
    chromium: String,
    blobs: Option<crate::blobs::Blobs>,
) -> Result<(), String> {
    let config = BrowserConfig::builder()
        .chrome_executable(chromium)
        .no_sandbox()
        // containers give /dev/shm 64MB by default, which Chromium outgrows
        .arg("--disable-dev-shm-usage")
        .arg("--hide-scrollbars")
        // this browser only ever loads one local page, so everything it would
        // normally keep warm for a human is memory the pod does not have
        .arg("--disable-background-networking")
        .arg("--disable-extensions")
        .arg("--disable-default-apps")
        .arg("--disable-sync")
        .arg("--no-first-run")
        .arg("--mute-audio")
        .viewport(Some(WindowViewport {
            width: WIDTH,
            height: HEIGHT,
            device_scale_factor: Some(1.0),
            ..WindowViewport::default()
        }))
        .build()?;

    let (browser, mut handler) = Browser::launch(config).await.map_err(|e| e.to_string())?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });
    tracing::info!("preview worker ready");

    loop {
        match claim(&pool).await {
            Some(job) => {
                let outcome = render(&browser, port, job.revision_id, &job.scheme).await;
                finish(&pool, blobs.as_ref(), &job, outcome).await;
            }
            None => tokio::time::sleep(IDLE_POLL).await,
        }
    }
}

struct Job {
    revision_id: i64,
    scheme: String,
}

async fn claim(pool: &SqlitePool) -> Option<Job> {
    sqlx::query_as!(
        Job,
        r#"UPDATE revision_previews
           SET status = 'running', attempts = attempts + 1, updated_at = datetime('now')
           WHERE rowid = (SELECT rowid FROM revision_previews
                          WHERE status = 'pending' ORDER BY revision_id LIMIT 1)
           RETURNING revision_id as "revision_id!: i64", scheme as "scheme!: String""#
    )
    .fetch_optional(pool)
    .await
    .ok()?
}

async fn render(
    browser: &Browser,
    port: u16,
    revision_id: i64,
    scheme: &str,
) -> Result<Vec<u8>, String> {
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| e.to_string())?;

    // every early return below would otherwise leave the tab open, and one
    // leaked tab per failed job is what turns a broken render into an OOM kill
    let shot = capture(&page, port, revision_id, scheme).await;
    let _ = page.clone().close().await;
    shot
}

async fn capture(
    page: &chromiumoxide::Page,
    port: u16,
    revision_id: i64,
    scheme: &str,
) -> Result<Vec<u8>, String> {
    let url = format!("http://127.0.0.1:{port}/_render/{revision_id}/");

    let media: SetEmulatedMediaParams = SetEmulatedMediaParamsBuilder::default()
        .media("screen")
        .features(vec![
            chromiumoxide::cdp::browser_protocol::emulation::MediaFeature {
                name: "prefers-color-scheme".to_string(),
                value: scheme.to_string(),
            },
        ])
        .build();
    page.execute(media).await.map_err(|e| e.to_string())?;

    page.goto(url).await.map_err(|e| e.to_string())?;
    page.wait_for_navigation()
        .await
        .map_err(|e| e.to_string())?;
    tokio::time::sleep(SETTLE).await;

    // the viewport, not the full page: a full capture of a long plan is a tall
    // sliver that reads as noise at thumbnail size
    page.screenshot(
        CaptureScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Webp)
            .quality(75)
            .clip(Viewport {
                x: 0.0,
                y: 0.0,
                width: f64::from(WIDTH),
                height: f64::from(HEIGHT),
                scale: SCALE,
            })
            .capture_beyond_viewport(true)
            .build(),
    )
    .await
    .map_err(|e| e.to_string())
}

async fn finish(
    pool: &SqlitePool,
    blobs: Option<&crate::blobs::Blobs>,
    job: &Job,
    outcome: Result<Vec<u8>, String>,
) {
    let width = (f64::from(WIDTH) * SCALE) as i64;
    let height = (f64::from(HEIGHT) * SCALE) as i64;

    // a thumbnail is derived data, so it goes straight to the bucket when there
    // is one rather than being written inline for the sweep to move later
    let outcome = match (outcome, blobs) {
        (Ok(image), Some(blobs)) => blobs.put(&image).await.map(|key| (None, Some(key))),
        (Ok(image), None) => Ok((Some(image), None)),
        (Err(error), _) => Err(error),
    };

    let result = match outcome {
        Ok((image, object_key)) => {
            sqlx::query!(
                "UPDATE revision_previews
                 SET status = 'ready', image = ?, object_key = ?, content_type = 'image/webp',
                     width = ?, height = ?, error = NULL, updated_at = datetime('now')
                 WHERE revision_id = ? AND scheme = ?",
                image,
                object_key,
                width,
                height,
                job.revision_id,
                job.scheme
            )
            .execute(pool)
            .await
        }
        Err(error) => {
            tracing::warn!(revision = job.revision_id, scheme = job.scheme, %error, "preview failed");
            // back to pending until the attempt cap, so a transient failure retries
            sqlx::query!(
                "UPDATE revision_previews
                 SET status = CASE WHEN attempts >= ? THEN 'failed' ELSE 'pending' END,
                     error = ?, updated_at = datetime('now')
                 WHERE revision_id = ? AND scheme = ?",
                MAX_ATTEMPTS,
                error,
                job.revision_id,
                job.scheme
            )
            .execute(pool)
            .await
        }
    };
    if let Err(error) = result {
        tracing::error!(%error, "could not record preview outcome");
    }
}

/// Queue both schemes for a revision. Called inside the push transaction, so
/// the worker cannot observe a half written revision.
pub async fn enqueue(tx: &mut sqlx::SqliteConnection, revision_id: i64) -> Result<(), sqlx::Error> {
    for scheme in ["light", "dark"] {
        sqlx::query!(
            "INSERT OR REPLACE INTO revision_previews (revision_id, scheme, status)
             VALUES (?, ?, 'pending')",
            revision_id,
            scheme
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}
