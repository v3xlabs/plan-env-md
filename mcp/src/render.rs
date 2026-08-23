use crate::api::{
    Answer, AnsweredQuestion, DocumentInfo, DocumentSummary, ProjectSummary, PushedDocument,
};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// plan.env.md stores a timestamp as `YYYY-MM-DD HH:MM:SS` in UTC.
fn fields(stamp: &str) -> Option<[i64; 6]> {
    let cut = |from: usize, to: usize| -> Option<i64> { stamp.get(from..to)?.parse().ok() };
    Some([
        cut(0, 4)?,
        cut(5, 7)?,
        cut(8, 10)?,
        cut(11, 13)?,
        cut(14, 16)?,
        cut(17, 19)?,
    ])
}

/// Days since 1970-01-01, by Howard Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn epoch(stamp: &str) -> Option<i64> {
    let [year, month, day, hour, minute, second] = fields(stamp)?;
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// How long ago, which is what a list is scanned for. A stamp this cannot read
/// is passed through rather than hidden.
fn age(stamp: &str, now: i64) -> String {
    let Some(then) = epoch(stamp) else {
        return stamp.to_string();
    };
    let seconds = (now - then).max(0);
    match seconds {
        ..60 => "now".to_string(),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        86_400..604_800 => format!("{}d", seconds / 86_400),
        _ => format!("{}w", seconds / 604_800),
    }
}

/// When exactly, which is what a revision index is read for.
fn moment(stamp: &str) -> String {
    let Some([_, month, day, hour, minute, _]) = fields(stamp) else {
        return stamp.to_string();
    };
    let name = usize::try_from(month - 1)
        .ok()
        .and_then(|index| MONTHS.get(index))
        .copied()
        .unwrap_or("?");
    format!("{day} {name} {hour:02}:{minute:02}")
}

pub fn size(bytes: i64) -> String {
    if bytes < 1_024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

pub fn push(document: &PushedDocument, asked: Option<usize>) -> String {
    let files = document
        .files
        .iter()
        .map(|file| format!("{} {}", file.path, size(file.size_bytes)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines = vec![
        format!(
            "pushed rev {}  {}  {files}",
            document.revision,
            size(document.size_bytes)
        ),
        document.url.clone(),
    ];
    if let Some(count) = asked.filter(|count| *count > 0) {
        lines.push(format!(
            "{} for the reader. plan_answers reads the decisions.",
            plural(count, "question")
        ));
    }
    lines.join("\n")
}

pub fn projects(projects: &[ProjectSummary], now: i64) -> String {
    if projects.is_empty() {
        return "no projects".to_string();
    }
    let names = projects
        .iter()
        .map(|project| {
            if project.aliases.is_empty() {
                project.slug.clone()
            } else {
                format!("{} ({})", project.slug, project.aliases.join(", "))
            }
        })
        .collect::<Vec<_>>();
    let head = plural(projects.len(), "project");
    let width = names
        .iter()
        .map(String::len)
        .chain([head.len()])
        .max()
        .unwrap_or_default();
    let mut lines = vec![format!("{head:<width$}  docs  last push  icon")];
    for (project, name) in projects.iter().zip(&names) {
        let pushed = project
            .last_pushed_at
            .as_deref()
            .map(|stamp| age(stamp, now))
            .unwrap_or_else(|| "never".to_string());
        let icon = match (project.has_favicon_light, project.has_favicon_dark) {
            (true, true) => "light+dark",
            (true, false) => "light",
            (false, true) => "dark",
            (false, false) => "none",
        };
        lines.push(format!(
            "{name:<width$}  {:>4}  {pushed:<9}  {icon}",
            project.document_count
        ));
    }
    lines.join("\n")
}

/// One row per document. A list filtered to one project drops the project
/// column rather than repeating the same word down the page.
pub fn documents(documents: &[DocumentSummary], docs_url: &str, now: i64) -> String {
    if documents.is_empty() {
        return "no documents".to_string();
    }
    let first = documents[0].project.as_deref();
    let only = documents
        .iter()
        .all(|document| document.project.as_deref() == first)
        .then_some(first)
        .flatten();
    let width = match only {
        Some(_) => 0,
        None => documents
            .iter()
            .filter_map(|document| document.project.as_deref())
            .map(str::len)
            .chain(["project".len()])
            .max()
            .unwrap_or_default(),
    };

    let count = plural(documents.len(), "document");
    let scope = match only {
        Some(project) => format!("{count} in {project}"),
        None => count,
    };
    let mut lines = vec![
        format!("{scope}, newest first. Open any at {docs_url}<ref>"),
        match only {
            Some(_) => "age    rev  Q    ref / title".to_string(),
            None => format!("age    rev  Q    {:<width$}  ref / title", "project"),
        },
    ];
    for document in documents {
        let answers = if document.questions_total == 0 {
            "-".to_string()
        } else {
            format!(
                "{}/{}",
                document.questions_answered, document.questions_total
            )
        };
        let project = match only {
            Some(_) => String::new(),
            None => format!("{:<width$}  ", document.project.as_deref().unwrap_or("-")),
        };
        let row = format!(
            "{:<6} r{:<3} {answers:<4} {project}{}/{}  {}{}",
            age(&document.last_pushed_at, now),
            document.latest_revision,
            document.id,
            document.slug,
            document.title.as_deref().unwrap_or_default(),
            if document.published { "  [public]" } else { "" },
        );
        lines.push(row.trim_end().to_string());
    }
    lines.join("\n")
}

pub fn info(document: &DocumentInfo) -> String {
    let mut lines = vec![
        match &document.title {
            Some(title) => format!("{}  \"{title}\"", document.slug),
            None => document.slug.clone(),
        },
        document.url.clone(),
    ];

    let mut facts = Vec::new();
    if let Some(project) = &document.project {
        facts.push(format!("project {project}"));
    }
    if !document.tags.is_empty() {
        facts.push(format!("tags {}", document.tags.join(", ")));
    }
    facts.push(
        if document.published {
            "published"
        } else {
            "unpublished"
        }
        .to_string(),
    );
    facts.push(format!("created {}", moment(&document.created_at)));
    lines.push(facts.join("  "));

    if let Some(latest) = document
        .revisions
        .iter()
        .map(|revision| revision.revision)
        .max()
    {
        lines.push(String::new());
        lines.push(format!(
            "{}, latest {latest}",
            plural(document.revisions.len(), "revision")
        ));
        for revision in document.revisions.iter().rev() {
            lines.push(format!(
                "  {:>2}  {}  {}  {}",
                revision.revision,
                moment(&revision.created_at),
                size(revision.size_bytes),
                plural(revision.files.len(), "file"),
            ));
        }
    }

    if !document.questions.is_empty() {
        lines.push(String::new());
        lines.push(questions(&document.questions));
    }
    lines.join("\n")
}

pub fn question_summary(questions: &[AnsweredQuestion]) -> String {
    let answered = questions
        .iter()
        .filter(|question| question.answer.is_some())
        .count();
    format!("{answered} of {} questions answered", questions.len())
}

/// An answered question drops the options the reader rejected. An open one
/// keeps their values, because the model may still have to chase the reader.
pub fn questions(questions: &[AnsweredQuestion]) -> String {
    let mut lines = vec![question_summary(questions)];
    for question in questions {
        match &question.answer {
            Some(answer) => {
                lines.push(format!("{}  {}", question.key, question.prompt));
                lines.push(format!(
                    "  {}  {}",
                    choice(question, answer),
                    moment(&answer.answered_at)
                ));
                if let Some(notes) = &answer.notes {
                    lines.push(format!("  note: \"{notes}\""));
                }
            }
            None => {
                lines.push(format!("{}  {}  [open]", question.key, question.prompt));
                lines.push(format!(
                    "  {}",
                    question
                        .options
                        .iter()
                        .map(|option| option.value.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ));
            }
        }
    }
    lines.join("\n")
}

fn choice(question: &AnsweredQuestion, answer: &Answer) -> String {
    let picked = answer
        .selected
        .iter()
        .map(|value| {
            // `other` is the reader writing their own answer, and the text they
            // wrote is the whole content of the decision
            if value == "other" {
                return format!("other: \"{}\"", answer.other_text.as_deref().unwrap_or(""));
            }
            match question
                .options
                .iter()
                .find(|option| &option.value == value)
            {
                Some(option) => format!("{value}  ({})", option.label),
                None => value.clone(),
            }
        })
        .collect::<Vec<_>>();
    if picked.is_empty() {
        return "nothing selected".to_string();
    }
    picked.join(", ")
}

#[cfg(test)]
mod tests {
    use super::{age, documents, moment, projects, questions, size};
    use crate::api::{Answer, AnsweredOption, AnsweredQuestion, DocumentSummary, ProjectSummary};

    fn at(stamp: &str) -> i64 {
        super::epoch(stamp).expect("a readable stamp")
    }

    #[test]
    fn ages_are_relative_and_moments_are_absolute() {
        let now = at("2026-08-23 20:00:00");
        assert_eq!(age("2026-08-23 19:07:50", now), "52m");
        assert_eq!(age("2026-08-23 12:00:00", now), "8h");
        assert_eq!(age("2026-08-20 20:00:00", now), "3d");
        assert_eq!(age("2026-08-01 20:00:00", now), "3w");
        assert_eq!(moment("2026-08-21 16:54:19"), "21 Aug 16:54");
        assert_eq!(size(27_823), "27.2 KB");
    }

    #[test]
    fn an_unreadable_stamp_is_passed_through() {
        assert_eq!(age("soon", 0), "soon");
        assert_eq!(moment("soon"), "soon");
    }

    fn document(slug: &str, project: &str, answered: i64, total: i64) -> DocumentSummary {
        DocumentSummary {
            id: "KPQgKFWuIX".to_string(),
            slug: slug.to_string(),
            title: Some("Dashboard: list and grid".to_string()),
            project: Some(project.to_string()),
            tags: vec!["mockup".to_string()],
            published: false,
            revision_count: 1,
            latest_revision: 1,
            questions_total: total,
            questions_answered: answered,
            last_pushed_at: "2026-08-23 15:25:43".to_string(),
            created_at: "2026-08-23 15:25:43".to_string(),
            updated_at: "2026-08-23 15:25:43".to_string(),
            url: "https://plan.env.md/KPQgKFWuIX/plan-env-md-dashboard-grid-views".to_string(),
        }
    }

    #[test]
    fn a_list_of_one_project_drops_the_project_column() {
        let now = at("2026-08-23 20:00:00");
        let rendered = documents(
            &[document("a-plan", "plan-env-md", 3, 3), document("b-plan", "plan-env-md", 0, 0)],
            "https://plan.env.md/",
            now,
        );
        assert!(rendered.starts_with("2 documents in plan-env-md, newest first."));
        assert!(!rendered.contains("  plan-env-md  "));
        assert!(rendered.contains("KPQgKFWuIX/a-plan  Dashboard: list and grid"));
        // no questions reads as nothing to chase, which "0/0" does not
        assert!(rendered.lines().last().expect("a row").contains(" -    "));
    }

    #[test]
    fn a_list_of_several_projects_keeps_the_column() {
        let now = at("2026-08-23 20:00:00");
        let rendered = documents(
            &[document("a-plan", "plan-env-md", 3, 3), document("b-plan", "gitgui", 1, 2)],
            "https://plan.env.md/",
            now,
        );
        assert!(rendered.starts_with("2 documents, newest first."));
        assert!(rendered.contains("gitgui"));
        assert!(rendered.contains("1/2"));
    }

    #[test]
    fn a_project_shows_its_aliases_and_whether_it_has_an_icon() {
        let rendered = projects(
            &[ProjectSummary {
                slug: "open-lavatory".to_string(),
                aliases: vec!["openlv".to_string()],
                document_count: 16,
                last_pushed_at: Some("2026-08-23 12:00:00".to_string()),
                has_favicon_light: true,
                has_favicon_dark: false,
            }],
            at("2026-08-23 20:00:00"),
        );
        assert!(rendered.contains("open-lavatory (openlv)"));
        assert!(rendered.contains("16"));
        assert!(rendered.ends_with("8h         light"));
    }

    fn question(key: &str, answer: Option<Answer>) -> AnsweredQuestion {
        AnsweredQuestion {
            key: key.to_string(),
            prompt: "Which grid?".to_string(),
            anchor: None,
            options: vec![
                AnsweredOption {
                    value: "a".to_string(),
                    label: "A, contact sheet".to_string(),
                    detail: Some("A tradeoff nobody needs once the choice is made".to_string()),
                },
                AnsweredOption {
                    value: "b".to_string(),
                    label: "B, two columns".to_string(),
                    detail: None,
                },
            ],
            answer,
        }
    }

    #[test]
    fn an_answered_question_drops_the_options_the_reader_rejected() {
        let rendered = questions(&[question(
            "variation",
            Some(Answer {
                selected: vec!["a".to_string()],
                other_text: None,
                notes: None,
                answered_at: "2026-08-23 16:09:12".to_string(),
            }),
        )]);
        assert!(rendered.starts_with("1 of 1 questions answered"));
        assert!(rendered.contains("a  (A, contact sheet)  23 Aug 16:09"));
        assert!(!rendered.contains("B, two columns"));
        assert!(!rendered.contains("A tradeoff nobody needs"));
    }

    #[test]
    fn free_text_and_notes_survive() {
        let rendered = questions(&[question(
            "variation",
            Some(Answer {
                selected: vec!["other".to_string()],
                other_text: Some("i like all of them, do em all".to_string()),
                notes: Some("query one project by default".to_string()),
                answered_at: "2026-08-23 16:09:12".to_string(),
            }),
        )]);
        assert!(rendered.contains("other: \"i like all of them, do em all\""));
        assert!(rendered.contains("note: \"query one project by default\""));
    }

    #[test]
    fn an_open_question_keeps_its_option_values() {
        let rendered = questions(&[question("variation", None)]);
        assert!(rendered.starts_with("0 of 1 questions answered"));
        assert!(rendered.contains("Which grid?  [open]"));
        assert!(rendered.contains("a | b"));
    }
}
