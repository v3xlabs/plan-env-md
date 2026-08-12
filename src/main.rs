mod api;
mod auth;
mod config;
mod db;
mod static_files;
#[cfg(test)]
mod tests;
mod view;

use poem::listener::TcpListener;
use poem::middleware::CookieJarManager;
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
            get(view::view_latest).post(view::unlock),
        )
        .at("/:public_id/:slug/rev/:revision", get(view::view_revision))
        .at("/", get(static_files::index))
        .at("/*path", get(static_files::spa))
        .with(CookieJarManager::new())
        .data(pool)
        .data(base_url)
        .data(secret)
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
