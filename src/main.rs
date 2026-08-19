mod answer_key;
mod api;
mod auth;
mod config;
mod db;
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

fn app(pool: SqlitePool, base_url: config::BaseUrl, secret: config::Secret) -> impl poem::Endpoint {
    let service = api::service();
    let spec = service.spec_endpoint();
    let scalar = service.scalar();
    Route::new()
        .nest("/api", service)
        .at("/api/openapi.json", spec)
        // scalar() returns an inner Route matching "/", so it must be nested
        .nest("/docs", scalar)
        .at(
            "/:public_id/:slug",
            get(view::redirect_to_dir).post(view::unlock),
        )
        .at(
            "/:public_id/:slug/rev/:revision",
            get(view::redirect_revision_to_dir),
        )
        .at("/:public_id/:slug/rev/:revision/", get(view::view_revision))
        .at("/:public_id/:slug/share", get(view::share_page))
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
        .at("/", get(static_files::index))
        .at("/*path", get(static_files::spa))
        .with(Tracing)
        .with(CookieJarManager::new())
        .data(pool)
        .data(base_url)
        .data(secret)
        .data(rate_limit::RateLimiter::default())
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

    // the worker drives a browser at the loopback render route, so it needs the
    // port this process actually listens on
    let port = config
        .bind
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .expect("BIND must end in :port");
    preview::spawn(pool.clone(), port);

    tracing::info!(bind = %config.bind, "listening");
    Server::new(TcpListener::bind(&config.bind))
        .run(app(
            pool,
            config::BaseUrl(config.base_url),
            config::Secret(config.secret),
        ))
        .await
        .expect("server failed");
}
