//! Renders a thumbnail of each revision, in both colour schemes, and a drawn
//! placeholder from the same settled page.
//!
//! Chromium navigates a loopback-only route that serves the revision without
//! the viewer overlay, so a thumbnail shows the document rather than the
//! document plus a pill cluster. Jobs are claimed one at a time: this is a
//! single-user instance and a second concurrent tab buys nothing but memory.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::{
    SetEmulatedMediaParams, SetEmulatedMediaParamsBuilder,
};
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams, RequestPattern,
};
use chromiumoxide::cdp::browser_protocol::network::ErrorReason;
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
/// A document's scripts share the page with the walk and can stall it.
const WALK_LIMIT: Duration = Duration::from_secs(5);
/// A host that does not resolve in this long is treated as unreachable.
const RESOLVE_LIMIT: Duration = Duration::from_secs(5);

/// The revision the worker is rendering right now, under a name only the worker
/// knows.
///
/// The render route answers any loopback caller, and the browser that calls it
/// runs the author's scripts. With a revision id in the URL, one document could
/// frame any other revision and read it back off its own thumbnail.
#[derive(Clone, Default)]
pub struct Tickets(Arc<Mutex<Option<Ticket>>>);

struct Ticket {
    token: String,
    revision_id: i64,
}

/// The render route serves the revision for as long as this lives.
pub struct Issued {
    tickets: Tickets,
    token: String,
}

impl Tickets {
    pub fn issue(&self, revision_id: i64) -> Issued {
        let token = crate::auth::random_base62(32);
        *self.slot() = Some(Ticket {
            token: token.clone(),
            revision_id,
        });
        Issued {
            tickets: self.clone(),
            token,
        }
    }

    pub fn revision(&self, token: &str) -> Option<i64> {
        self.slot()
            .as_ref()
            .filter(|ticket| ticket.token == token)
            .map(|ticket| ticket.revision_id)
    }

    fn slot(&self) -> MutexGuard<'_, Option<Ticket>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Issued {
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Issued {
    fn drop(&mut self) {
        let mut slot = self.tickets.slot();
        if slot
            .as_ref()
            .is_some_and(|ticket| ticket.token == self.token)
        {
            *slot = None;
        }
    }
}

pub fn spawn(pool: SqlitePool, port: u16, blobs: Option<crate::blobs::Blobs>, tickets: Tickets) {
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

        if let Err(error) = run(pool, port, chromium, blobs, tickets).await {
            tracing::error!(%error, "preview worker stopped");
        }
    });
}

async fn run(
    pool: SqlitePool,
    port: u16,
    chromium: String,
    blobs: Option<crate::blobs::Blobs>,
    tickets: Tickets,
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
    let browser = Arc::new(browser);
    guard_network(&browser, port, tickets.clone()).await?;
    tracing::info!("preview worker ready");

    loop {
        match claim(&pool).await {
            Some(job) => {
                let outcome = render(&browser, port, &tickets, job.revision_id, &job.scheme).await;
                finish(&pool, blobs.as_ref(), &job, outcome).await;
            }
            None => tokio::time::sleep(IDLE_POLL).await,
        }
    }
}

/// Every request the browser makes waits here first. A document is its author's
/// code running inside the pod, and anything it frames comes back in the
/// thumbnail, so it may reach its own render route and hosts that resolve to
/// public addresses, and nothing else on the pod's network.
///
/// Enabled on the browser target rather than per page, so frames that Chromium
/// runs in another process are held too.
async fn guard_network(browser: &Arc<Browser>, port: u16, tickets: Tickets) -> Result<(), String> {
    let mut paused = browser
        .event_listener::<EventRequestPaused>()
        .await
        .map_err(|e| e.to_string())?;
    browser
        .execute(
            EnableParams::builder()
                .pattern(RequestPattern::builder().url_pattern("*").build())
                .build(),
        )
        .await
        .map_err(|e| e.to_string())?;

    let browser = browser.clone();
    tokio::spawn(async move {
        while let Some(event) = paused.next().await {
            let browser = browser.clone();
            let tickets = tickets.clone();
            tokio::spawn(async move {
                let url = &event.request.url;
                let id = event.request_id.clone();
                let outcome = if admits(url, port, &tickets).await {
                    browser
                        .execute(ContinueRequestParams::new(id))
                        .await
                        .map(drop)
                } else {
                    // debug: Chromium asks the render route for /favicon.ico on
                    // every page, so this fires on each render
                    tracing::debug!(url, "preview blocked a request");
                    browser
                        .execute(FailRequestParams::new(id, ErrorReason::BlockedByClient))
                        .await
                        .map(drop)
                };
                if let Err(error) = outcome {
                    tracing::warn!(url, %error, "preview could not settle a request");
                }
            });
        }
    });
    Ok(())
}

/// Chromium hands over canonical URLs: a lowercase host, IPv4 in dotted form,
/// IPv6 in brackets and no default port. That is what keeps a split by hand
/// sound here.
pub(crate) async fn admits(url: &str, port: u16, tickets: &Tickets) -> bool {
    if url.starts_with("data:") || url.starts_with("blob:") {
        return true;
    }
    let render = format!("http://127.0.0.1:{port}/_render/");
    if let Some(rest) = url.strip_prefix(&render) {
        let token = rest.split('/').next().unwrap_or_default();
        return tickets.revision(token).is_some();
    }

    let (default_port, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (443, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (80, rest)
    } else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (host, port) = match host_port.strip_prefix('[') {
        Some(bracketed) => match bracketed.split_once(']') {
            Some((host, rest)) => (host, rest.strip_prefix(':')),
            None => return false,
        },
        None => match host_port.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        },
    };
    let port = match port.map(str::parse::<u16>) {
        None => default_port,
        Some(Ok(port)) => port,
        Some(Err(_)) => return false,
    };

    // a name that resolves to both kinds of address is refused: Chromium may
    // connect to either
    match tokio::time::timeout(RESOLVE_LIMIT, tokio::net::lookup_host((host, port))).await {
        Ok(Ok(addresses)) => {
            let addresses: Vec<_> = addresses.collect();
            !addresses.is_empty() && addresses.iter().all(|address| is_public(address.ip()))
        }
        _ => false,
    }
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_v4(mapped);
            }
            let [first, second, ..] = ip.segments();
            // NAT64 carries an IPv4 address in its last 32 bits
            if first == 0x64 && second == 0xff9b {
                let [.., a, b, c, d] = ip.octets();
                return is_public_v4(Ipv4Addr::new(a, b, c, d));
            }
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                // site-local, deprecated but still routed by some stacks
                || first & 0xffc0 == 0xfec0)
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        // shared address space, which clusters and tailnets hand out
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && ip.octets()[2] == 0)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240)
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

/// What one job produces. The placeholder is optional: a page whose layout
/// cannot be read still has a perfectly good screenshot.
struct Capture {
    image: Vec<u8>,
    placeholder: Option<String>,
}

async fn render(
    browser: &Browser,
    port: u16,
    tickets: &Tickets,
    revision_id: i64,
    scheme: &str,
) -> Result<Capture, String> {
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| e.to_string())?;

    let ticket = tickets.issue(revision_id);
    let url = format!("http://127.0.0.1:{port}/_render/{}/", ticket.token());
    // every early return below would otherwise leave the tab open, and one
    // leaked tab per failed job is what turns a broken render into an OOM kill
    let shot = capture(&page, url, revision_id, scheme).await;
    let _ = page.clone().close().await;
    shot
}

async fn capture(
    page: &chromiumoxide::Page,
    url: String,
    revision_id: i64,
    scheme: &str,
) -> Result<Capture, String> {
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
    let image = page
        .screenshot(
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
        .map_err(|e| e.to_string())?;

    let placeholder = walk(page)
        .await
        .inspect_err(|error| tracing::warn!(revision_id, scheme, %error, "placeholder failed"))
        .ok();
    Ok(Capture { image, placeholder })
}

/// Reads the layout of the page as it is now, after the screenshot, so both
/// describe the same settled frame.
async fn walk(page: &chromiumoxide::Page) -> Result<String, String> {
    let evaluation = tokio::time::timeout(
        WALK_LIMIT,
        page.evaluate_function(crate::placeholder::WALKER),
    )
    .await
    .map_err(|_| "the layout walk timed out".to_string())?
    .map_err(|e| e.to_string())?;
    let layout = evaluation
        .into_value::<serde_json::Value>()
        .map_err(|e| e.to_string())?;
    crate::placeholder::render(layout)
}

async fn finish(
    pool: &SqlitePool,
    blobs: Option<&crate::blobs::Blobs>,
    job: &Job,
    outcome: Result<Capture, String>,
) {
    let width = (f64::from(WIDTH) * SCALE) as i64;
    let height = (f64::from(HEIGHT) * SCALE) as i64;

    // a thumbnail is derived data, so it goes straight to the bucket when there
    // is one rather than being written inline for the sweep to move later. The
    // placeholder is a few kilobytes of text and always stays inline.
    let outcome = match (outcome, blobs) {
        (Ok(capture), Some(blobs)) => blobs
            .put(&capture.image)
            .await
            .map(|key| (None, Some(key), capture.placeholder)),
        (Ok(capture), None) => Ok((Some(capture.image), None, capture.placeholder)),
        (Err(error), _) => Err(error),
    };

    let result = match outcome {
        Ok((image, object_key, placeholder)) => {
            sqlx::query!(
                "UPDATE revision_previews
                 SET status = 'ready', image = ?, object_key = ?, content_type = 'image/webp',
                     width = ?, height = ?, placeholder = ?, error = NULL,
                     updated_at = datetime('now')
                 WHERE revision_id = ? AND scheme = ?",
                image,
                object_key,
                width,
                height,
                placeholder,
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
/// the worker cannot observe a half written revision. A row that already holds
/// a capture keeps it, so a re-render shows the old picture until the new one
/// lands rather than a blank.
pub async fn enqueue(tx: &mut sqlx::SqliteConnection, revision_id: i64) -> Result<(), sqlx::Error> {
    for scheme in ["light", "dark"] {
        sqlx::query!(
            "INSERT INTO revision_previews (revision_id, scheme, status)
             VALUES (?, ?, 'pending')
             ON CONFLICT (revision_id, scheme)
             DO UPDATE SET status = 'pending', attempts = 0, error = NULL,
                           updated_at = datetime('now')",
            revision_id,
            scheme
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}
