pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub app_url: Origin,
    pub docs_url: Origin,
    pub secret: String,
    /// None when S3_BUCKET is unset, which keeps every blob inline.
    pub bucket: Option<Bucket>,
}

/// Object storage for blobs that have grown too large or gone cold. The
/// cluster supplies these through a Rook ObjectBucketClaim, so the endpoint is
/// a bare in-cluster host and the gateway needs path style addressing.
pub struct Bucket {
    pub name: String,
    pub endpoint: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl Bucket {
    /// Every field but the region is required, so a half configured bucket
    /// fails at startup rather than on the first push that needs it.
    fn from_env() -> Option<Bucket> {
        let host = std::env::var("S3_ENDPOINT").ok()?;
        Some(Bucket {
            name: std::env::var("S3_BUCKET").ok()?,
            endpoint: endpoint(&host, std::env::var("S3_PORT").ok().as_deref()),
            region: env_or("S3_REGION", "us-east-1"),
            access_key_id: std::env::var("S3_ACCESS").ok()?,
            secret_access_key: std::env::var("S3_SECRET").ok()?,
        })
    }
}

/// The claim hands out a host and a port separately and neither carries a
/// scheme, so both are assembled here. Port 443 is the only one that implies
/// TLS; a Ceph gateway inside the cluster is plain HTTP on anything else.
fn endpoint(host: &str, port: Option<&str>) -> String {
    if host.starts_with("http://") || host.starts_with("https://") {
        return host.to_string();
    }
    match port {
        Some("443") => format!("https://{host}:443"),
        Some(port) => format!("http://{host}:{port}"),
        None => format!("http://{host}"),
    }
}

/// One of the two origins this service answers on.
///
/// They are separate origins and not one host with two paths because a document
/// runs its author's scripts. Keeping documents off the app's origin is what
/// stops those scripts from reaching the session, the API, or each other's
/// cookies, and it is what lets a document load its own uploaded files as
/// ordinary same-origin subresources.
#[derive(Clone)]
pub struct Origin(pub String);

impl Origin {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_https(&self) -> bool {
        self.0.starts_with("https://")
    }

    /// Host and port, as a request's `Host` header spells them.
    pub fn authority(&self) -> &str {
        self.0
            .split_once("://")
            .map_or(self.0.as_str(), |(_, rest)| rest)
            .trim_end_matches('/')
    }
}

/// Where the app lives: the SPA, the API and the session.
#[derive(Clone)]
pub struct AppUrl(pub Origin);

/// Where documents and their files live.
#[derive(Clone)]
pub struct DocsUrl(pub Origin);

/// Server secret that signs visitor access cookies.
#[derive(Clone)]
pub struct Secret(pub String);

pub const DEV_SECRET: &str = "insecure-dev-secret";

impl Config {
    /// The two development defaults are one port under two hostnames, which is
    /// all a browser needs to treat them as separate origins.
    pub fn from_env() -> Config {
        Config {
            bind: env_or("BIND", "127.0.0.1:3000"),
            database_url: env_or("DATABASE_URL", "sqlite://data/dev.db"),
            app_url: Origin(env_or("APP_URL", "http://127.0.0.1:3000")),
            docs_url: Origin(env_or("DOCS_URL", "http://localhost:3000")),
            secret: env_or("SECRET", DEV_SECRET),
            bucket: Bucket::from_env(),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
