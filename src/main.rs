mod answer_key;
mod api;
mod auth;
mod blobs;
mod config;
mod db;
mod demote;
mod grant;
mod preview;
mod rate_limit;
mod static_files;
#[cfg(test)]
mod tests;
mod view;

use poem::listener::TcpListener;
use poem::middleware::{CookieJarManager, Tracing};
use poem::{EndpointExt, Route, Server, get};
use sqlx::SqlitePool;

/// The app: everything a signed-in person or an agent talks to.
fn app_routes() -> Route {
    let service = api::service();
    let spec = service.spec_endpoint();
    let scalar = service.scalar();
    Route::new()
        .nest("/api", service)
        .at("/api/openapi.json", spec)
        // scalar() returns an inner Route matching "/", so it must be nested
        .nest("/docs", scalar)
        .at("/grant", get(grant::issue))
        .at("/share/:public_id/:slug", get(view::share_page))
        // the preview worker drives a browser at the loopback address, whose
        // Host is neither origin, so the render route answers on both
        .at("/_render/:revision_id/*path", get(view::render_asset))
        .at("/", get(static_files::index))
        .at("/*path", get(static_files::spa))
}

/// Documents and the files they were pushed with. No API, no session, no SPA:
/// a document runs its author's scripts, and this origin holds nothing those
/// scripts could use.
fn docs_routes() -> Route {
    Route::new()
        .at("/", get(view::docs_root))
        .at(
            "/:public_id/:slug",
            get(view::redirect_to_dir).post(view::unlock),
        )
        .at(
            "/:public_id/:slug/rev/:revision",
            get(view::redirect_revision_to_dir),
        )
        .at("/:public_id/:slug/rev/:revision/", get(view::view_revision))
        .at("/_planenv/:name", get(static_files::answer_asset))
        // one route, not a bare directory plus a wildcard: poem prefers the
        // wildcard, so the bare route would be dead and every render would 404
        .at("/_render/:revision_id/*path", get(view::render_asset))
        // registered after the specific routes above, which poem prefers
        .at(
            "/:public_id/:slug/rev/:revision/*path",
            get(view::asset_revision),
        )
        .at(
            "/:public_id/:slug/*path",
            get(view::asset_latest).post(view::unlock_at_dir),
        )
}

fn app(
    pool: SqlitePool,
    app_url: config::AppUrl,
    docs_url: config::DocsUrl,
    secret: config::Secret,
    blobs: Option<blobs::Blobs>,
) -> impl poem::Endpoint {
    let docs_authority = docs_url.0.authority().to_string();
    Origins {
        app: app_routes().around(auth::reject_foreign_writes),
        docs: docs_routes(),
        docs_authority,
    }
    .with(Tracing)
    .with(CookieJarManager::new())
    .data(pool)
    .data(app_url)
    .data(docs_url)
    .data(secret)
    .data(blobs)
    .data(rate_limit::RateLimiter::default())
}

/// Which set of routes answers is decided by the host the reader asked for, so
/// one process serves both origins and neither can be reached through the
/// other's hostname.
struct Origins<A, D> {
    app: A,
    docs: D,
    docs_authority: String,
}

impl<A, D> poem::Endpoint for Origins<A, D>
where
    A: poem::Endpoint<Output = poem::Response>,
    D: poem::Endpoint<Output = poem::Response>,
{
    type Output = poem::Response;

    async fn call(&self, req: poem::Request) -> poem::Result<Self::Output> {
        let host = req
            .uri()
            .host()
            .map(str::to_owned)
            .or_else(|| req.header(poem::http::header::HOST).map(str::to_owned))
            .unwrap_or_default();
        if host.eq_ignore_ascii_case(&self.docs_authority) {
            self.docs.call(req).await
        } else {
            self.app.call(req).await
        }
    }
}

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("spec") {
        println!("{}", api::service().spec());
        return;
    }

    tracing_subscriber::fmt::init();

    let config = config::Config::from_env();
    let pool = db::connect(&config.database_url)
        .await
        .expect("database setup failed");

    if config.secret == config::DEV_SECRET {
        tracing::warn!("SECRET is unset; visitor access cookies use the insecure dev secret");
    }

    // a bucket that cannot be reached must stop the process here. Serving on
    // regardless would accept pushes, record keys for objects that were never
    // stored, and only show the damage when somebody opens an old document.
    let blobs = match &config.bucket {
        Some(bucket) => {
            let blobs = blobs::Blobs::new(bucket).expect("bucket setup failed");
            blobs.check().await.expect("bucket is not usable");
            tracing::info!(bucket = %bucket.name, "blob storage ready");
            Some(blobs)
        }
        None => {
            tracing::info!("S3_BUCKET is unset; every blob stays in the database");
            None
        }
    };

    if std::env::args().nth(1).as_deref() == Some("demote") {
        let moved = demote::sweep_all(&pool, blobs.as_ref())
            .await
            .expect("demote failed");
        println!("moved {moved} bodies to the bucket");
        return;
    }

    // the worker drives a browser at the loopback render route, so it needs the
    // port this process actually listens on
    let port = config
        .bind
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .expect("BIND must end in :port");
    preview::spawn(pool.clone(), port, blobs.clone());
    demote::spawn(pool.clone(), blobs.clone());

    tracing::info!(
        bind = %config.bind,
        app = %config.app_url.as_str(),
        docs = %config.docs_url.as_str(),
        "listening"
    );
    Server::new(TcpListener::bind(&config.bind))
        .run(app(
            pool,
            config::AppUrl(config.app_url),
            config::DocsUrl(config.docs_url),
            config::Secret(config.secret),
            blobs,
        ))
        .await
        .expect("server failed");
}
