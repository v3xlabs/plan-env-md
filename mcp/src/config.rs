use std::path::PathBuf;

use reqwest::Url;

pub const DEFAULT_BASE_URL: &str = "https://plan.env.md";

pub struct Config {
    pub base_url: Url,
    pub token: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let base_url = std::env::var("PLAN_ENV_MD_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        let mut base_url = Url::parse(&base_url)
            .map_err(|_| "PLAN_ENV_MD_URL must be an absolute URL".to_string())?;
        if base_url.cannot_be_a_base() {
            return Err("PLAN_ENV_MD_URL must be an absolute URL".to_string());
        }
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }

        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "cannot locate the plan.env.md credential".to_string())?;
        let token = std::fs::read_to_string(home.join(".config/plan-env-md/config"))
            .map_err(|_| "cannot read the plan.env.md credential".to_string())?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err("the plan.env.md credential is empty".to_string());
        }

        Ok(Self { base_url, token })
    }
}
