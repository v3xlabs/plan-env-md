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
    #[schemars(
        description = "The complete HTML document. Use this or files, not both. Assets need files."
    )]
    html: Option<String>,
    #[schemars(
        description = "Files to upload, read from disk by this server. One must be index.html. Paths are relative and may nest, for example img/chart.webp."
    )]
    files: Option<Vec<PushFile>>,
    #[schemars(description = "Optional document title. Existing titles remain when omitted.")]
    title: Option<String>,
    #[schemars(
        description = "Project this document belongs to. Created on first use; an existing alias resolves to its project. Call plan_projects first to reuse the right name."
    )]
    project: Option<String>,
    #[schemars(
        description = "Loose tags such as plan, review, pr-review, audit, roadmap, spec, status, explainer, comparison, mockup, research. Normalised to lowercase hyphens. Omitting leaves existing tags alone; an empty list clears them."
    )]
    tags: Option<Vec<String>>,
    #[schemars(
        description = "Decisions to ask the reader, answered in the document itself. Each needs key, prompt and at least two options; anchor links a question to an element id in the page. The reader can always write their own answer or add a note, so do not add an option for that."
    )]
    questions: Option<serde_json::Value>,
}

#[derive(Deserialize, JsonSchema)]
struct PushFile {
    #[schemars(
        description = "Path inside the document, for example index.html or img/chart.webp."
    )]
    path: String,
    #[schemars(description = "Absolute path on this machine to read the bytes from.")]
    source: String,
}

#[derive(Deserialize, JsonSchema)]
struct ProjectDocumentsRequest {
    #[schemars(description = "Project slug or alias.")]
    project: String,
    #[schemars(description = "How many of the most recent documents to return. Defaults to 10.")]
    limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct SetFaviconRequest {
    #[schemars(description = "Project slug or alias.")]
    project: String,
    #[schemars(
        description = "Absolute path to a PNG, SVG, WebP, GIF or ICO of at most 64 KB. Square, legible at 16 pixels."
    )]
    source: String,
    #[schemars(
        description = "Which colour scheme this icon is for: light or dark. Upload both so the tab icon matches the reader's theme."
    )]
    scheme: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct AddAliasRequest {
    #[schemars(description = "The canonical project slug that documents are grouped under.")]
    project: String,
    #[schemars(
        description = "Another name that should resolve to it, for example openlv for open-lavatory."
    )]
    alias: String,
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
    /// Questions the document asks and what the reader decided. A sibling of
    /// content, not appended to it, so every view stays a pure projection and
    /// an unanswered question is unambiguously null.
    questions: Vec<serde_json::Value>,
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

/// Read each declared file from disk. This grants no capability the agent did
/// not already have, and failing here gives a clear local message instead of a
/// 422 from the server.
fn read_files(files: Vec<PushFile>) -> Result<Vec<api::FilePart>, ErrorData> {
    if !files.iter().any(|file| file.path == "index.html") {
        return Err(tool_error("one file must be index.html".to_string()));
    }
    files
        .into_iter()
        .map(|file| {
            let bytes = std::fs::read(&file.source)
                .map_err(|error| tool_error(format!("cannot read {}: {error}", file.source)))?;
            Ok(api::FilePart {
                path: file.path,
                bytes,
            })
        })
        .collect()
}

fn json<T: Serialize>(value: &T) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(value)
        .map_err(|_| ErrorData::internal_error("cannot serialize tool result", None))
}

#[tool_router(server_handler)]
impl PlanServer {
    #[tool(
        description = "Upload an HTML plan, optionally with assets, a project, tags and questions for the reader to answer. Reusing a slug appends a revision at the same document URL."
    )]
    async fn plan_push(
        &self,
        Parameters(request): Parameters<PushRequest>,
    ) -> Result<String, ErrorData> {
        if !valid_slug(&request.slug) {
            return Err(tool_error("slug must match [a-z0-9-]{1,64}".to_string()));
        }

        // html and files are two ways to say the same thing, so supplying both
        // is a mistake rather than a precedence question
        let files = match (request.html, request.files) {
            (Some(_), Some(_)) => {
                return Err(tool_error(
                    "supply either html or files, not both".to_string(),
                ));
            }
            (Some(html), None) => vec![api::FilePart {
                path: "index.html".to_string(),
                bytes: html.into_bytes(),
            }],
            (None, Some(files)) => read_files(files)?,
            (None, None) => return Err(tool_error("supply html or files".to_string())),
        };

        let mut meta = serde_json::Map::new();
        if let Some(title) = request.title {
            meta.insert("title".to_string(), title.into());
        }
        if let Some(project) = request.project {
            meta.insert("project".to_string(), project.into());
        }
        if let Some(tags) = request.tags {
            meta.insert("tags".to_string(), tags.into());
        }
        if let Some(questions) = request.questions {
            meta.insert("questions".to_string(), questions);
        }

        let pushed = self
            .api
            .push(&request.slug, meta.into(), files)
            .await
            .map_err(tool_error)?;
        json(&pushed)
    }

    #[tool(
        description = "List projects with their aliases, document counts and whether an icon is set. Call this before pushing so a document joins an existing project instead of starting a near-duplicate. A project with no icon is worth offering to set one for with plan_set_project_icon."
    )]
    async fn plan_projects(&self) -> Result<String, ErrorData> {
        json(&self.api.projects().await.map_err(tool_error)?)
    }

    #[tool(
        description = "Metadata for a project's most recent documents, newest first. Use this to catch up on a project, then plan_read the ones that matter."
    )]
    async fn plan_project_documents(
        &self,
        Parameters(request): Parameters<ProjectDocumentsRequest>,
    ) -> Result<String, ErrorData> {
        let documents = self
            .api
            .list(Some(&request.project), Some(request.limit.unwrap_or(10)))
            .await
            .map_err(tool_error)?;
        json(&documents)
    }

    #[tool(
        description = "Set a project's icon from a local image file. Every document in the project then serves it, so the reader's browser tab says which project they are looking at. Upload a light and a dark variant."
    )]
    async fn plan_set_project_icon(
        &self,
        Parameters(request): Parameters<SetFaviconRequest>,
    ) -> Result<String, ErrorData> {
        let scheme = request.scheme.unwrap_or_else(|| "light".to_string());
        if scheme != "light" && scheme != "dark" {
            return Err(tool_error("scheme must be light or dark".to_string()));
        }
        let bytes = std::fs::read(&request.source)
            .map_err(|error| tool_error(format!("cannot read {}: {error}", request.source)))?;
        self.api
            .set_favicon(&request.project, &scheme, bytes)
            .await
            .map_err(tool_error)?;
        json(&serde_json::json!({ "project": request.project, "scheme": scheme }))
    }

    #[tool(
        description = "Point another name at a project, so pushes naming either land in one place. Use when you notice two names for the same thing, such as openlv and open-lavatory."
    )]
    async fn plan_add_project_alias(
        &self,
        Parameters(request): Parameters<AddAliasRequest>,
    ) -> Result<String, ErrorData> {
        self.api
            .add_alias(&request.project, &request.alias)
            .await
            .map_err(tool_error)?;
        json(&serde_json::json!({ "project": request.project, "alias": request.alias }))
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
        let questions = self
            .api
            .info(&slug)
            .await
            .map(|info| info.questions)
            .unwrap_or_default();
        json(&ReadResult {
            slug,
            revision,
            view: projection.view,
            content: projection.content,
            questions,
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
        json(&self.api.list(None, None).await.map_err(tool_error)?)
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
