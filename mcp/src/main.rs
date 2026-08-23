mod api;
mod config;
mod projection;
mod render;

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
        description = "Decisions to ask the reader, answered in the document itself. Every revision declares its own set, so a push that omits this clears them: repeat them to keep them. An answer outlives the revision that asked it, so re-declaring a key brings its answer back. The reader can always write their own answer or add a note, so do not add an option for that."
    )]
    questions: Option<Vec<Question>>,
}

/// A decision the document asks its reader to make.
///
/// Typed rather than free JSON: an untyped field reaches the client as a
/// schema it cannot fill in, so the whole feature was unreachable through this
/// tool and had to be pushed with curl instead.
#[derive(Serialize, Deserialize, JsonSchema)]
struct Question {
    #[schemars(
        description = "Stable across revisions. An answer is keyed by this, so keep it when rewording the prompt."
    )]
    key: String,
    #[schemars(description = "The question itself, as the reader will read it.")]
    prompt: String,
    #[schemars(
        description = "Optional line under the prompt for context the prompt cannot carry."
    )]
    detail: Option<String>,
    #[schemars(
        description = "An element id in the page. The card is placed after that element, so the question sits with the section it is about."
    )]
    anchor: Option<String>,
    #[schemars(description = "Allow more than one option to be selected. Defaults to false.")]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    multiple: bool,
    #[schemars(description = "At least two, at most twelve.")]
    options: Vec<QuestionOption>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct QuestionOption {
    #[schemars(
        description = "Stable identifier for this choice, reported back as the answer. Lowercase and hyphenated reads best."
    )]
    value: String,
    #[schemars(description = "The choice as the reader sees it. A few words.")]
    label: String,
    #[schemars(
        description = "Optional line under the label, for a tradeoff the label cannot carry."
    )]
    detail: Option<String>,
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
struct ListRequest {
    #[schemars(
        description = "Project slug or alias. Pass the project you are working in unless the user asked across projects."
    )]
    project: Option<String>,
    #[schemars(description = "How many of the most recent documents to return. Defaults to 20.")]
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

struct PlanServer {
    api: Api,
    /// Where documents are read, which is not where the API answers.
    docs_url: reqwest::Url,
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
        if url.origin() != self.docs_url.origin() {
            return Err("document URL must use the configured documents origin".to_string());
        }
        // the service redirects a document to its directory form, so the URL a
        // reader copies out of the address bar ends in a slash and yields a
        // trailing empty segment
        let mut segments = url
            .path_segments()
            .ok_or_else(|| "document URL has no path".to_string())?
            .collect::<Vec<_>>();
        if segments.last() == Some(&"") {
            segments.pop();
        }
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

/// The html view is the whole source, which is the one response that can run to
/// hundreds of kilobytes. It goes to a file, and the model reads the part of it
/// that it wants.
fn write_source(
    slug: &str,
    revision: Option<i64>,
    html: &str,
) -> Result<std::path::PathBuf, ErrorData> {
    let directory = std::env::temp_dir().join("plan-env-md");
    std::fs::create_dir_all(&directory)
        .map_err(|error| tool_error(format!("cannot create {}: {error}", directory.display())))?;
    let name = match revision {
        Some(revision) => format!("{slug}-rev{revision}.html"),
        None => format!("{slug}-latest.html"),
    };
    let path = directory.join(name);
    std::fs::write(&path, html)
        .map_err(|error| tool_error(format!("cannot write {}: {error}", path.display())))?;
    Ok(path)
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

        let asked = request.questions.as_ref().map(Vec::len);
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
            let questions = serde_json::to_value(questions)
                .map_err(|error| tool_error(format!("cannot encode the questions: {error}")))?;
            meta.insert("questions".to_string(), questions);
        }

        let pushed = self
            .api
            .push(&request.slug, meta.into(), files)
            .await
            .map_err(tool_error)?;
        Ok(render::push(&pushed, asked))
    }

    #[tool(
        description = "List projects with their aliases, document counts and whether an icon is set. Call this before pushing so a document joins an existing project instead of starting a near-duplicate. A project with no icon is worth offering to set one for with plan_set_project_icon."
    )]
    async fn plan_projects(&self) -> Result<String, ErrorData> {
        let projects = self.api.projects().await.map_err(tool_error)?;
        Ok(render::projects(&projects, render::now()))
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
        Ok(format!("{scheme} icon set on {}", request.project))
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
        Ok(format!(
            "{} now resolves to {}",
            request.alias, request.project
        ))
    }

    #[tool(
        description = "Read a plan as readable text, a token-reduced outline, or an accessibility-oriented structural report. The html view writes the exact source to a file and returns its path."
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
        let pinned = match revision {
            Some(revision) => format!("rev {revision}"),
            None => "latest revision".to_string(),
        };
        let mut lines = vec![format!("{slug}  {pinned}  {}", view.name())];

        if let View::Html = view {
            let path = write_source(&slug, revision, &projection.content)?;
            lines.push(format!(
                "{} written to {}",
                render::size(projection.content.len() as i64),
                path.display()
            ));
            return Ok(lines.join("\n"));
        }

        let questions = self.api.questions(&slug).await.unwrap_or_default();
        if !questions.is_empty() {
            lines.push(format!(
                "{}, plan_answers for detail",
                render::question_summary(&questions)
            ));
        }
        lines.push(String::new());
        lines.push(projection.content);
        Ok(lines.join("\n"))
    }

    #[tool(description = "Document metadata, its revision index, and its questions.")]
    async fn plan_info(
        &self,
        Parameters(request): Parameters<DocumentRequest>,
    ) -> Result<String, ErrorData> {
        let (slug, _) = self
            .resolve_document(&request.document, None)
            .map_err(tool_error)?;
        let document = self.api.info(&slug).await.map_err(tool_error)?;
        Ok(render::info(&document))
    }

    #[tool(
        description = "What the reader decided about a document's questions. Call this after a push that asked any, and prefer it to plan_read or plan_info, which both carry the whole document as well."
    )]
    async fn plan_answers(
        &self,
        Parameters(request): Parameters<DocumentRequest>,
    ) -> Result<String, ErrorData> {
        let (slug, _) = self
            .resolve_document(&request.document, None)
            .map_err(tool_error)?;
        let questions = self.api.questions(&slug).await.map_err(tool_error)?;
        if questions.is_empty() {
            return Ok(format!("{slug} asks no questions"));
        }
        Ok(format!("{slug}  {}", render::questions(&questions)))
    }

    #[tool(
        description = "Documents newest first, one row each. Pass project to catch up on the project you are working in, which is what this is normally for; list across every project only when the user asks for that."
    )]
    async fn plan_list(
        &self,
        Parameters(request): Parameters<ListRequest>,
    ) -> Result<String, ErrorData> {
        let documents = self
            .api
            .list(request.project.as_deref(), Some(request.limit.unwrap_or(20)))
            .await
            .map_err(tool_error)?;
        Ok(render::documents(
            &documents,
            self.docs_url.as_str(),
            render::now(),
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::from_env().map_err(std::io::Error::other)?;
    let server = PlanServer {
        api: Api::new(config.base_url, config.token).map_err(std::io::Error::other)?,
        docs_url: config.docs_url,
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
        let docs_url = reqwest::Url::parse("https://plan.env.md/").expect("valid URL");
        let api = crate::api::Api::new(docs_url.clone(), "token".to_string()).expect("client");
        let server = PlanServer { api, docs_url };
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

    #[test]
    fn url_resolution_accepts_the_canonical_directory_form() {
        let docs_url = reqwest::Url::parse("https://plan.env.md/").expect("valid URL");
        let api = crate::api::Api::new(docs_url.clone(), "token".to_string()).expect("client");
        let server = PlanServer { api, docs_url };
        assert_eq!(
            server
                .resolve_document("https://plan.env.md/id/plan/", None)
                .expect("document"),
            ("plan".to_string(), None)
        );
        assert_eq!(
            server
                .resolve_document("https://plan.env.md/id/plan/rev/2/", None)
                .expect("document"),
            ("plan".to_string(), Some(2))
        );
    }
}
