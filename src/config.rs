pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub base_url: String,
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

/// Public origin used to build document URLs in API responses.
#[derive(Clone)]
pub struct BaseUrl(pub String);

impl BaseUrl {
    pub fn is_https(&self) -> bool {
        self.0.starts_with("https://")
    }
}

/// Server secret that signs visitor access cookies.
#[derive(Clone)]
pub struct Secret(pub String);

pub const DEV_SECRET: &str = "insecure-dev-secret";

impl Config {
    pub fn from_env() -> Config {
        Config {
            bind: env_or("BIND", "127.0.0.1:3000"),
            database_url: env_or("DATABASE_URL", "sqlite://data/dev.db"),
            base_url: env_or("BASE_URL", "http://127.0.0.1:3000"),
            secret: env_or("SECRET", DEV_SECRET),
            bucket: Bucket::from_env(),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
