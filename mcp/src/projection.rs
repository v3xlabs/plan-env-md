use rmcp::schemars;
use scraper::{Html, Selector};
use serde::Serialize;

#[derive(
    Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum View {
    Html,
    #[default]
    Text,
    Outline,
    A11y,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            View::Html => "html",
            View::Text => "text",
            View::Outline => "outline",
            View::A11y => "a11y",
        }
    }
}

#[derive(Serialize)]
pub struct Projection {
    pub view: View,
    pub content: String,
}

fn selector(query: &str) -> Selector {
    Selector::parse(query).expect("static CSS selector")
}

fn text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn project(html: &str, view: View) -> Projection {
    let content = match view {
        View::Html => html.to_string(),
        View::Text => text_view(html),
        View::Outline => outline_view(html),
        View::A11y => a11y_view(html),
    };
    Projection { view, content }
}

fn text_view(source: &str) -> String {
    let html = Html::parse_document(source);
    html.select(&selector(
        "h1, h2, h3, h4, h5, h6, p, li, blockquote, pre, th, td",
    ))
    .filter_map(|element| {
        let value = text(element);
        (!value.is_empty()).then_some(value)
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn outline_view(source: &str) -> String {
    let html = Html::parse_document(source);
    let title = html
        .select(&selector("title"))
        .next()
        .map(text)
        .unwrap_or_default();
    let headings = html
        .select(&selector("h1, h2, h3, h4, h5, h6"))
        .filter_map(|element| {
            let value = text(element);
            if value.is_empty() {
                return None;
            }
            let level = element.value().name().strip_prefix('h')?;
            Some(match element.value().attr("id") {
                Some(id) => format!("H{level} [{id}] {value}"),
                None => format!("H{level} {value}"),
            })
        })
        .collect::<Vec<_>>();

    // A plan page links mostly to itself: a contents rail and cross references,
    // which the heading list above already carries. A link that leaves the page
    // is a fact the headings cannot carry, so those are named and the rest are
    // counted.
    let mut in_page = 0;
    let mut external: Vec<&str> = Vec::new();
    for element in html.select(&selector("a[href]")) {
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        if href.starts_with('#') {
            in_page += 1;
        } else if !external.contains(&href) {
            external.push(href);
        }
    }
    let links = std::iter::once(format!(
        "LINKS {in_page} in-page, {} external",
        external.len()
    ))
    .chain(external.into_iter().map(|href| format!("LINK {href}")))
    .collect::<Vec<_>>();
    let tables = html.select(&selector("table")).count();
    [
        format!("TITLE {title}"),
        headings.join("\n"),
        links.join("\n"),
        format!("TABLES {tables}"),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

fn a11y_view(source: &str) -> String {
    let html = Html::parse_document(source);
    let title = html
        .select(&selector("title"))
        .next()
        .map(text)
        .unwrap_or_default();
    let language = html
        .select(&selector("html"))
        .next()
        .and_then(|element| element.value().attr("lang"))
        .unwrap_or("missing");
    let landmarks = html.select(&selector("main, nav, header, footer, aside, [role=main], [role=navigation], [role=banner], [role=contentinfo], [role=complementary]")).count();
    let headings = html
        .select(&selector("h1, h2, h3, h4, h5, h6"))
        .filter_map(|element| element.value().name().strip_prefix('h'))
        .collect::<Vec<_>>();
    let skipped = headings
        .windows(2)
        .any(|pair| pair[1].parse::<u8>().unwrap_or(0) > pair[0].parse::<u8>().unwrap_or(0) + 1);
    let images = html.select(&selector("img")).collect::<Vec<_>>();
    let missing_alt = images
        .iter()
        .filter(|image| image.value().attr("alt").is_none())
        .count();
    let links_without_text = html
        .select(&selector("a[href]"))
        .filter(|link| text(*link).is_empty() && link.value().attr("aria-label").is_none())
        .count();
    let controls = html
        .select(&selector("button, input, select, textarea"))
        .count();
    format!(
        "TITLE: {title}\nLANG: {language}\nLANDMARKS: {landmarks}\nHEADINGS: {}\nHEADING_LEVEL_SKIP: {skipped}\nIMAGES: {}\nIMAGES_MISSING_ALT: {missing_alt}\nLINKS_WITHOUT_TEXT: {links_without_text}\nCONTROLS: {controls}",
        headings.join(", "),
        images.len()
    )
}

#[cfg(test)]
mod tests {
    use super::{View, project};

    const PAGE: &str = r##"<html lang="en"><head><title>Plan</title></head><body><main>
        <h1 id="top">Title</h1><h2>Unanchored</h2>
        <nav><a href="#top">Contents</a><a href="#top">Top again</a><a href="#other">Other</a></nav>
        <p><a href="https://esm.sh/shiki@3">shiki</a> and <a href="https://esm.sh/shiki@3">again</a></p>
        </main></body></html>"##;

    #[test]
    fn the_outline_counts_in_page_links_and_names_the_rest() {
        let outline = project(PAGE, View::Outline).content;
        assert!(outline.contains("LINKS 3 in-page, 1 external"));
        assert!(outline.contains("LINK https://esm.sh/shiki@3"));
        // the contents rail restates the heading list, so its targets are noise
        assert!(!outline.contains("#top"));
    }

    #[test]
    fn a_heading_without_an_id_prints_no_empty_bracket() {
        let outline = project(PAGE, View::Outline).content;
        assert!(outline.contains("H1 [top] Title"));
        assert!(outline.contains("H2 Unanchored"));
        assert!(!outline.contains("[-]"));
    }
}
