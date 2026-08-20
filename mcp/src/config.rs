use std::path::PathBuf;

use reqwest::Url;

/// The API answers on the app's origin; documents are served from their own, so
/// the URL a document is read at is not the URL it is pushed to.
pub const DEFAULT_BASE_URL: &str = "https://env.md";
pub const DEFAULT_DOCS_URL: &str = "https://plan.env.md";

pub struct Config {
    pub base_url: Url,
    pub docs_url: Url,
    pub token: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let base_url = absolute("PLAN_ENV_MD_URL", DEFAULT_BASE_URL)?;
        let docs_url = absolute("PLAN_ENV_MD_DOCS_URL", DEFAULT_DOCS_URL)?;

        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "cannot locate the plan.env.md credential".to_string())?;
        let token = std::fs::read_to_string(home.join(".config/plan-env-md/config"))
            .map_err(|_| "cannot read the plan.env.md credential".to_string())?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err("the plan.env.md credential is empty".to_string());
        }

        Ok(Self {
            base_url,
            docs_url,
            token,
        })
    }
}

/// A base URL always ends in a slash, so joining a path onto it keeps the whole
/// address rather than replacing its last segment.
fn absolute(key: &str, default: &str) -> Result<Url, String> {
    let raw = std::env::var(key).unwrap_or_else(|_| default.into());
    let mut url = Url::parse(&raw).map_err(|_| format!("{key} must be an absolute URL"))?;
    if url.cannot_be_a_base() {
        return Err(format!("{key} must be an absolute URL"));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}
