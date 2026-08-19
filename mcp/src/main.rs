mod api;
mod config;
mod projection;

use api::Api;
use projection::{View, project};
use rmcp::{
    ErrorData, ServiceExt,
    handler::server::wrapper::Parameters,
    schemars::{self, JsonSchema},
    tool, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
struct PushRequest {
    #[schemars(
        description = "A lowercase, hyphen-separated document slug. Reusing it creates a new revision."
    )]
    slug: String,
    #[schemars(description = "Complete self-contained HTML document.")]
    html: String,
    #[schemars(description = "Optional document title. Existing titles remain when omitted.")]
    title: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DocumentRequest {
    #[schemars(description = "A document slug or a plan.env.md document URL.")]
    document: String,
}

#[derive(Deserialize, JsonSchema)]
struct ReadRequest {
    #[schemars(
        description = "A document slug or a plan.env.md document URL. URLs may include /rev/<revision>."
    )]
    document: String,
    #[schemars(
        description = "Optional revision number. It must agree with a revision in document URL."
    )]
    revision: Option<i64>,
    #[schemars(
        description = "html preserves source, text is readable content, outline is token-reduced structure, and a11y reports semantic structure."
    )]
    view: Option<View>,
}

#[derive(Serialize)]
struct ReadResult {
    slug: String,
    revision: Option<i64>,
    view: View,
    content: String,
}

struct PlanServer {
    api: Api,
    base_url: reqwest::Url,
}

impl PlanServer {
    fn resolve_document(
        &self,
        document: &str,
        revision: Option<i64>,
    ) -> Result<(String, Option<i64>), String> {
        if valid_slug(document) {
            return Ok((document.to_string(), revision));
        }
        let url = reqwest::Url::parse(document)
            .map_err(|_| "document must be a slug or a plan.env.md URL".to_string())?;
        if url.origin() != self.base_url.origin() {
            return Err("document URL must use the configured plan.env.md origin".to_string());
        }
        let segments = url
            .path_segments()
            .ok_or_else(|| "document URL has no path".to_string())?
            .collect::<Vec<_>>();
        let (slug, pinned) = match segments.as_slice() {
            [_, slug] => (*slug, None),
            [_, slug, "rev", value] => (
                *slug,
                Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| "document URL has an invalid revision".to_string())?,
                ),
            ),
            _ => return Err("document URL has an unsupported path".to_string()),
        };
        if !valid_slug(slug) {
            return Err("document URL has an invalid slug".to_string());
        }
        if revision.is_some() && revision != pinned {
            return Err("revision conflicts with the revision in the document URL".to_string());
        }
        Ok((slug.to_string(), pinned.or(revision)))
    }
}

fn valid_slug(slug: &str) -> bool {
    (1..=64).contains(&slug.len())
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn tool_error(message: String) -> ErrorData {
    ErrorData::invalid_params(message, None)
}

fn json<T: Serialize>(value: &T) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(value)
        .map_err(|_| ErrorData::internal_error("cannot serialize tool result", None))
}

#[tool_router(server_handler)]
impl PlanServer {
    #[tool(
        description = "Upload a complete HTML plan. Reusing a slug appends a revision at the same document URL."
    )]
    async fn plan_push(
        &self,
        Parameters(request): Parameters<PushRequest>,
    ) -> Result<String, ErrorData> {
        if !valid_slug(&request.slug) {
            return Err(tool_error("slug must match [a-z0-9-]{1,64}".to_string()));
        }
        let pushed = self
            .api
            .push(&request.slug, request.html, request.title)
            .await
            .map_err(tool_error)?;
        json(&pushed)
    }

    #[tool(
        description = "Read a plan as exact HTML, readable text, a token-reduced outline, or an accessibility-oriented structural report."
    )]
    async fn plan_read(
        &self,
        Parameters(request): Parameters<ReadRequest>,
    ) -> Result<String, ErrorData> {
        let (slug, revision) = self
            .resolve_document(&request.document, request.revision)
            .map_err(tool_error)?;
        let view = request.view.unwrap_or_default();
        let html = self.api.raw(&slug, revision).await.map_err(tool_error)?;
        let projection = project(&html, view);
        json(&ReadResult {
            slug,
            revision,
            view: projection.view,
            content: projection.content,
        })
    }

    #[tool(description = "Get document metadata and its ordered revision index.")]
    async fn plan_info(
        &self,
        Parameters(request): Parameters<DocumentRequest>,
    ) -> Result<String, ErrorData> {
        let (slug, _) = self
            .resolve_document(&request.document, None)
            .map_err(tool_error)?;
        json(&self.api.info(&slug).await.map_err(tool_error)?)
    }

    #[tool(
        description = "List documents owned by the configured plan.env.md account, newest first."
    )]
    async fn plan_list(&self) -> Result<String, ErrorData> {
        json(&self.api.list().await.map_err(tool_error)?)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::from_env().map_err(std::io::Error::other)?;
    let server = PlanServer {
        api: Api::new(config.base_url.clone(), config.token).map_err(std::io::Error::other)?,
        base_url: config.base_url,
    };
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PlanServer, valid_slug};

    #[test]
    fn validates_slugs() {
        assert!(valid_slug("project-plan-2"));
        assert!(!valid_slug("Project_Plan"));
    }

    #[test]
    fn projections_are_deterministic() {
        let html = "<html lang=\"en\"><head><title>Plan</title></head><body><main><h1 id=\"p1\">Title</h1><p>Text</p><img src=\"x\"><a href=\"/next\">Next</a></main></body></html>";
        assert_eq!(
            crate::projection::project(html, crate::projection::View::Text).content,
            "Title\n\nText"
        );
        assert!(
            crate::projection::project(html, crate::projection::View::Outline)
                .content
                .contains("H1 [p1] Title")
        );
        assert!(
            crate::projection::project(html, crate::projection::View::A11y)
                .content
                .contains("IMAGES_MISSING_ALT: 1")
        );
    }

    #[test]
    fn url_resolution_rejects_other_origins() {
        let base_url = reqwest::Url::parse("https://plan.env.md/").expect("valid URL");
        let api = crate::api::Api::new(base_url.clone(), "token".to_string()).expect("client");
        let server = PlanServer { api, base_url };
        assert!(
            server
                .resolve_document("https://example.com/id/plan", None)
                .is_err()
        );
        assert_eq!(
            server
                .resolve_document("https://plan.env.md/id/plan/rev/2", None)
                .expect("document"),
            ("plan".to_string(), Some(2))
        );
    }
}
