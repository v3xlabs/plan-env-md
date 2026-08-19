//! What a revision's file set is allowed to contain.
//!
//! Every rule here rejects the whole push rather than dropping one file: a
//! revision that silently lost its stylesheet is worse than one that never
//! landed.

pub const ENTRY_PATH: &str = "index.html";
/// The entry document keeps the cap it always had, so a plan stays readable.
pub const MAX_ENTRY_BYTES: usize = 512 * 1024;
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_REVISION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 64;
const MAX_PATH: usize = 128;
const MAX_SEGMENTS: usize = 4;

/// Reserved by the routes a document occupies. Reserving them at upload means
/// the router never has to disambiguate, and a reserved path 404s naturally
/// because no row can exist there.
const RESERVED_ROOT: &str = "share";
const RESERVED_PREFIX: &str = "rev/";

pub struct UploadedFile {
    pub path: String,
    pub content: Vec<u8>,
    pub content_type: &'static str,
}

pub fn validate_path(path: &str) -> Result<(), String> {
    if !(1..=MAX_PATH).contains(&path.len()) {
        return Err(format!("path {path:?} must be 1 to {MAX_PATH} characters"));
    }
    if !path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
    {
        return Err(format!(
            "path {path:?} may only contain letters, digits, dot, underscore, hyphen and slash"
        ));
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(format!("path {path:?} must not start or end with a slash"));
    }

    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > MAX_SEGMENTS {
        return Err(format!(
            "path {path:?} may be at most {MAX_SEGMENTS} segments deep"
        ));
    }
    for segment in &segments {
        if segment.is_empty() || *segment == "." || *segment == ".." {
            return Err(format!("path {path:?} has an empty or relative segment"));
        }
    }

    if path == RESERVED_ROOT || path.starts_with(RESERVED_PREFIX) {
        return Err(format!(
            "path {path:?} is reserved; a document's own URLs live at {RESERVED_ROOT} and {RESERVED_PREFIX}*"
        ));
    }
    if content_type_for(path).is_none() {
        return Err(format!("path {path:?} has no accepted file extension"));
    }
    Ok(())
}

/// The whole set, once every part has been read.
pub fn validate_set(files: &[UploadedFile]) -> Result<(), String> {
    if files.len() > MAX_FILES {
        return Err(format!("a revision may hold at most {MAX_FILES} files"));
    }
    if !files.iter().any(|file| file.path == ENTRY_PATH) {
        return Err(format!("a revision needs an {ENTRY_PATH}"));
    }

    let mut seen = std::collections::HashSet::new();
    let mut total = 0usize;
    for file in files {
        if !seen.insert(file.path.as_str()) {
            return Err(format!("path {:?} appears twice", file.path));
        }
        let cap = if file.path == ENTRY_PATH {
            MAX_ENTRY_BYTES
        } else {
            MAX_FILE_BYTES
        };
        if file.content.len() > cap {
            return Err(format!(
                "{} is {} KB, over the {} KB limit",
                file.path,
                file.content.len() / 1024,
                cap / 1024
            ));
        }
        total += file.content.len();
    }
    if total > MAX_REVISION_BYTES {
        return Err(format!(
            "the revision totals {} KB, over the {} KB limit",
            total / 1024,
            MAX_REVISION_BYTES / 1024
        ));
    }
    Ok(())
}

/// Decided by the server from the extension, never taken from the part header.
///
/// Deliberately separate from `content_type_of` in `static_files`: that one
/// serves the compiled-in SPA bundle and must never fail, so it falls back to
/// `application/octet-stream`. This one must reject what it does not recognise.
/// Same data, opposite contract.
pub fn content_type_for(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" | "diff" | "patch" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        _ => return None,
    })
}
