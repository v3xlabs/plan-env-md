use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Api {
    base_url: Url,
    client: Client,
}

#[derive(Deserialize, Serialize)]
pub struct PushedDocument {
    pub id: String,
    pub slug: String,
    pub revision: i64,
    pub size_bytes: i64,
    pub url: String,
    /// What the revision holds, so a caller can check that every asset it
    /// linked from the entry document arrived at the path the link uses
    pub files: Vec<File>,
}

/// One stored file, served at `<document url>/<path>`.
#[derive(Deserialize, Serialize)]
pub struct File {
    pub path: String,
    pub size_bytes: i64,
    pub content_type: String,
}

#[derive(Deserialize, Serialize)]
pub struct Revision {
    pub revision: i64,
    pub size_bytes: i64,
    pub created_at: String,
    pub files: Vec<File>,
}

#[derive(Deserialize, Serialize)]
pub struct DocumentInfo {
    pub id: String,
    pub slug: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub published: bool,
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
    pub revisions: Vec<Revision>,
    /// What the latest revision asks, and what the owner answered
    pub questions: Vec<AnsweredQuestion>,
}

/// A question as it comes back, carrying the reader's decision or `null`.
///
/// The question fields are flattened into the same object by the service, so
/// they are captured loosely here: this type exists to make `answer` explicit,
/// not to restate a shape the service already owns.
#[derive(Deserialize, Serialize)]
pub struct AnsweredQuestion {
    pub key: String,
    pub prompt: String,
    pub anchor: Option<String>,
    pub options: Vec<AnsweredOption>,
    /// `None` until the reader decides.
    pub answer: Option<Answer>,
}

#[derive(Deserialize, Serialize)]
pub struct AnsweredOption {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Answer {
    /// Option values the reader picked, or `other` when they wrote their own.
    pub selected: Vec<String>,
    /// What they wrote, when `selected` holds `other`.
    pub other_text: Option<String>,
    pub notes: Option<String>,
    pub answered_at: String,
}

#[derive(Deserialize, Serialize)]
pub struct DocumentSummary {
    pub id: String,
    pub slug: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub published: bool,
    pub revision_count: i64,
    pub latest_revision: i64,
    pub questions_total: i64,
    pub questions_answered: i64,
    pub last_pushed_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
}

#[derive(Deserialize, Serialize)]
pub struct ProjectSummary {
    pub slug: String,
    pub aliases: Vec<String>,
    pub document_count: i64,
    pub last_pushed_at: Option<String>,
    pub has_favicon_light: bool,
    pub has_favicon_dark: bool,
}

/// One file of a revision, read from disk by this server rather than passed
/// through the model: the agent already wrote them, and base64 in a tool
/// argument is bloat with no upside.
pub struct FilePart {
    pub path: String,
    pub bytes: Vec<u8>,
}

impl Api {
    pub fn new(base_url: Url, token: String) -> Result<Self, String> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut headers = reqwest::header::HeaderMap::new();
        let authorization = format!("Bearer {token}")
            .parse()
            .map_err(|_| "invalid plan.env.md credential".to_string())?;
        headers.insert(reqwest::header::AUTHORIZATION, authorization);
        let client = Client::builder()
            .https_only(base_url.scheme() == "https")
            .no_proxy()
            .default_headers(headers)
            .build()
            .map_err(|error| format!("cannot create the plan.env.md HTTP client: {error}"))?;
        Ok(Self { base_url, client })
    }

    fn endpoint(&self, path: &str) -> Result<Url, String> {
        self.base_url
            .join(path)
            .map_err(|_| "cannot construct a plan.env.md API URL".to_string())
    }

    async fn checked(&self, response: reqwest::Response) -> Result<reqwest::Response, String> {
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let body = body.chars().take(1_024).collect::<String>();
        let message = match status {
            StatusCode::UNAUTHORIZED => "authentication failed".to_string(),
            StatusCode::NOT_FOUND => "document not found".to_string(),
            _ if body.is_empty() => format!("plan.env.md returned HTTP {status}"),
            _ => format!("plan.env.md returned HTTP {status}: {body}"),
        };
        Err(message)
    }

    /// Metadata rides in a JSON `meta` part and each file in a part named after
    /// its path, which is the one shape the server accepts for anything richer
    /// than a bare document.
    pub async fn push(
        &self,
        slug: &str,
        meta: serde_json::Value,
        files: Vec<FilePart>,
    ) -> Result<PushedDocument, String> {
        let mut form = reqwest::multipart::Form::new().text("meta", meta.to_string());
        for file in files {
            let part = reqwest::multipart::Part::bytes(file.bytes).file_name(file.path.clone());
            form = form.part(file.path, part);
        }
        let response = self
            .client
            .put(self.endpoint(&format!("api/docs/{slug}"))?)
            .multipart(form)
            .send()
            .await
            .map_err(|_| "cannot reach plan.env.md".to_string())?;
        self.checked(response)
            .await?
            .json()
            .await
            .map_err(|_| "plan.env.md returned an invalid upload response".to_string())
    }

    pub async fn info(&self, slug: &str) -> Result<DocumentInfo, String> {
        let response = self
            .client
            .get(self.endpoint(&format!("api/docs/{slug}"))?)
            .send()
            .await
            .map_err(|_| "cannot reach plan.env.md".to_string())?;
        self.checked(response)
            .await?
            .json()
            .await
            .map_err(|_| "plan.env.md returned invalid document metadata".to_string())
    }

    pub async fn list(
        &self,
        project: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<DocumentSummary>, String> {
        let mut url = self.endpoint("api/docs")?;
        // query_pairs_mut leaves a bare "?" behind even when nothing is
        // appended, so it is only taken when there is something to add
        if project.is_some() || limit.is_some() {
            let mut query = url.query_pairs_mut();
            if let Some(project) = project {
                query.append_pair("project", project);
            }
            if let Some(limit) = limit {
                query.append_pair("limit", &limit.to_string());
            }
        }
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| "cannot reach plan.env.md".to_string())?;
        self.checked(response)
            .await?
            .json()
            .await
            .map_err(|_| "plan.env.md returned an invalid document list".to_string())
    }

    pub async fn projects(&self) -> Result<Vec<ProjectSummary>, String> {
        let response = self
            .client
            .get(self.endpoint("api/projects")?)
            .send()
            .await
            .map_err(|_| "cannot reach plan.env.md".to_string())?;
        self.checked(response)
            .await?
            .json()
            .await
            .map_err(|_| "plan.env.md returned an invalid project list".to_string())
    }

    pub async fn set_favicon(
        &self,
        project: &str,
        scheme: &str,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        let mut url = self.endpoint(&format!("api/projects/{project}/favicon"))?;
        url.query_pairs_mut().append_pair("scheme", scheme);
        let response = self
            .client
            .put(url)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(bytes)
            .send()
            .await
            .map_err(|_| "cannot reach plan.env.md".to_string())?;
        self.checked(response).await.map(|_| ())
    }

    pub async fn add_alias(&self, project: &str, alias: &str) -> Result<(), String> {
        let response = self
            .client
            .put(self.endpoint(&format!("api/projects/{project}/aliases/{alias}"))?)
            .send()
            .await
            .map_err(|_| "cannot reach plan.env.md".to_string())?;
        self.checked(response).await.map(|_| ())
    }

    pub async fn raw(&self, slug: &str, revision: Option<i64>) -> Result<String, String> {
        let path = match revision {
            Some(revision) => format!("api/docs/{slug}/revisions/{revision}/raw"),
            None => format!("api/docs/{slug}/raw"),
        };
        let response = self
            .client
            .get(self.endpoint(&path)?)
            .send()
            .await
            .map_err(|_| "cannot reach plan.env.md".to_string())?;
        self.checked(response)
            .await?
            .text()
            .await
            .map_err(|_| "plan.env.md returned an invalid HTML document".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{Api, Url};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn list_sends_bearer_authentication() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("connection");
            let mut request = vec![0; 4_096];
            let count = stream.read(&mut request).await.expect("request");
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]").await.expect("response");
            String::from_utf8(request[..count].to_vec()).expect("UTF-8 request")
        });
        let base_url = Url::parse(&format!("http://{address}/")).expect("URL");
        let api = Api::new(base_url, "private-token".to_string()).expect("API");

        assert!(
            api.list(None, None)
                .await
                .expect("document list")
                .is_empty()
        );
        let request = server.await.expect("server task");
        assert!(request.starts_with("GET /api/docs HTTP/1.1\r\n"));
        assert!(request.contains("authorization: Bearer private-token\r\n"));
    }
}
