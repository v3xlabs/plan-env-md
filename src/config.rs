pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub base_url: String,
    pub secret: String,
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
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
