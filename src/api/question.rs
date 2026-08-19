use poem_openapi::Object;
use serde::{Deserialize, Serialize};

pub const OTHER: &str = "other";
const MAX_QUESTIONS: usize = 32;
const MAX_OPTIONS: usize = 12;
const MIN_OPTIONS: usize = 2;
pub const MAX_TEXT: usize = 2000;

#[derive(Object, Serialize, Deserialize, Clone)]
pub struct Question {
    /// Stable across revisions; an answer is keyed by this, not by revision.
    pub key: String,
    pub prompt: String,
    pub detail: Option<String>,
    /// An element id in the document; the widget places the card after it.
    pub anchor: Option<String>,
    #[serde(default)]
    pub multiple: bool,
    pub options: Vec<QuestionOption>,
}

#[derive(Object, Serialize, Deserialize, Clone)]
pub struct QuestionOption {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Object, Serialize, Deserialize, Clone)]
pub struct Answer {
    pub selected: Vec<String>,
    pub other_text: Option<String>,
    pub notes: Option<String>,
    pub answered_at: String,
}

/// A question with whatever the owner decided about it. One shape for the
/// widget, the SPA and the MCP tool.
#[derive(Object, Serialize, Clone)]
pub struct AnsweredQuestion {
    #[oai(flatten)]
    #[serde(flatten)]
    pub question: Question,
    pub answer: Option<Answer>,
}

pub fn validate_all(questions: &[Question]) -> Result<(), String> {
    if questions.len() > MAX_QUESTIONS {
        return Err(format!("at most {MAX_QUESTIONS} questions per revision"));
    }
    let mut seen = std::collections::HashSet::new();
    for question in questions {
        validate(question)?;
        if !seen.insert(question.key.as_str()) {
            return Err(format!("duplicate question key {}", question.key));
        }
    }
    Ok(())
}

fn validate(question: &Question) -> Result<(), String> {
    if !is_key(&question.key) {
        return Err(format!(
            "question key {} must match [A-Za-z0-9_-]{{1,64}}",
            question.key
        ));
    }
    if question.prompt.trim().is_empty() {
        return Err(format!("question {} needs a prompt", question.key));
    }
    if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&question.options.len()) {
        return Err(format!(
            "question {} needs between {MIN_OPTIONS} and {MAX_OPTIONS} options",
            question.key
        ));
    }
    if let Some(anchor) = &question.anchor
        && !is_key(anchor)
    {
        return Err(format!(
            "question {} anchor must match [A-Za-z0-9_-]{{1,64}}",
            question.key
        ));
    }

    let mut seen = std::collections::HashSet::new();
    for option in &question.options {
        if !is_key(&option.value) {
            return Err(format!(
                "option {} of question {} must match [A-Za-z0-9_-]{{1,64}}",
                option.value, question.key
            ));
        }
        // the widget always offers a written answer under this value, so a
        // declared option may not claim it
        if option.value == OTHER {
            return Err(format!(
                "question {} may not declare an option named {OTHER}; the widget always offers one",
                question.key
            ));
        }
        if option.label.trim().is_empty() {
            return Err(format!(
                "option {} of question {} needs a label",
                option.value, question.key
            ));
        }
        if !seen.insert(option.value.as_str()) {
            return Err(format!(
                "duplicate option {} in question {}",
                option.value, question.key
            ));
        }
    }
    Ok(())
}

/// Check a submitted answer against the question that was asked.
pub fn check_answer(
    question: &Question,
    selected: &[String],
    other_text: Option<&str>,
) -> Result<(), String> {
    if selected.is_empty() {
        return Err("select at least one option".to_string());
    }
    if !question.multiple && selected.len() > 1 {
        return Err("this question takes a single answer".to_string());
    }

    let mut seen = std::collections::HashSet::new();
    for value in selected {
        if !seen.insert(value.as_str()) {
            return Err(format!("option {value} selected twice"));
        }
        if value != OTHER && !question.options.iter().any(|o| &o.value == value) {
            return Err(format!("option {value} was not offered"));
        }
    }

    let wrote_other = selected.iter().any(|value| value == OTHER);
    let has_text = other_text.is_some_and(|text| !text.trim().is_empty());
    match (wrote_other, has_text) {
        (true, false) => Err("a written answer needs text".to_string()),
        (false, true) => Err("text was given without selecting a written answer".to_string()),
        _ => Ok(()),
    }
}

fn is_key(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
