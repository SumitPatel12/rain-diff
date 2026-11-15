//! The prompt a question becomes: a fixed preamble, where the review stands, the parts of the
//! Brief that touch the question, the anchored excerpt, the thread so far and the question.
//! Pure and deterministic; it only ever holds what the caller passes in.

use rain_core::BriefDoc;

pub const PREAMBLE: &str = "You are answering a code-review question inside Rain, a diff viewer. \
Answer in 150 words or fewer. Cite code as path:line. Do not edit files. \
If the answer is not in the excerpt, say so and name what you would read next.";

/// The prompt's length cap in characters.
pub const MAX_CHARS: usize = 6000;
const QUESTION_CHARS: usize = 1500;
const EXCERPT_CHARS: usize = 2400;
const BRIEF_CHARS: usize = 1400;
const THREAD_CHARS: usize = 1200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoShas {
    pub repo: String,
    pub base: String,
    pub target: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Excerpt {
    #[default]
    None,
    /// Diff lines around the anchor; `snapshot` is the text as it was when the thread was posted.
    Code {
        path: String,
        snapshot: Option<String>,
        diff: Option<String>,
    },
    /// The path matched `ask.redact`.
    Withheld { path: String },
    /// A Brief section, Map node or note: its title and text.
    Artifact { title: String, text: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Msg {
    pub author: String,
    pub body: String,
}

#[derive(Clone, Debug, Default)]
pub struct PromptCtx<'a> {
    pub title: String,
    pub shas: Vec<RepoShas>,
    pub brief: Option<&'a BriefDoc>,
    /// The thread's file as `<repo>/<path>` (or the bare path), to pick the `how` sections citing it.
    pub file: Option<String>,
    /// The Brief section the question is on.
    pub section: Option<String>,
    /// `server/src/x.cs:L27-31`, `Brief › How 2`, …
    pub location: String,
    pub excerpt: Excerpt,
    /// The thread's messages before the question, oldest first.
    pub earlier: Vec<Msg>,
    pub question: String,
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    }
}

fn same_file(cite: &str, file: &str) -> bool {
    let (c, f) = (cite.replace('\\', "/"), file.replace('\\', "/"));
    c == f || c.ends_with(&format!("/{f}")) || f.ends_with(&format!("/{c}"))
}

fn brief_part(c: &PromptCtx) -> String {
    let Some(doc) = c.brief else { return String::new() };
    let mut out = String::new();
    if let Some(i) = &doc.intent {
        out.push_str(&format!("Intent: {}\n", i.text.trim()));
    }
    for h in &doc.how {
        let cites_file = c.file.as_deref().is_some_and(|f| h.cites.iter().any(|ct| same_file(&ct.path, f)));
        let is_section = c.section.as_deref() == Some(h.id.as_str());
        if cites_file || is_section {
            out.push_str(&format!("[{}] {}: {}\n", h.id, h.title, h.text.trim()));
        }
    }
    if let Some(id) = c.section.as_deref()
        && !doc.how.iter().any(|h| h.id == id)
        && let Some(s) = doc.section(id)
    {
        out.push_str(&format!("[{}] {}: {}\n", s.id, s.title, s.text.trim()));
    }
    clip(&out, BRIEF_CHARS)
}

fn excerpt_part(e: &Excerpt) -> String {
    match e {
        Excerpt::None => String::new(),
        Excerpt::Withheld { path } => {
            format!(
                "The code of {path} is withheld: its path matches the reviewer's redact list. Answer without it or say what you would need.\n"
            )
        }
        Excerpt::Artifact { title, text } => clip(&format!("{title}:\n{}\n", text.trim()), EXCERPT_CHARS),
        Excerpt::Code { path, snapshot, diff } => {
            let mut s = String::new();
            if let Some(snap) = snapshot.as_deref().filter(|t| !t.trim().is_empty()) {
                s.push_str(&format!(
                    "Anchored code when the question was posted ({path}):\n```\n{}\n```\n",
                    snap.trim_end()
                ));
            }
            if let Some(d) = diff.as_deref().filter(|t| !t.trim().is_empty()) {
                s.push_str(&format!("The diff around it (old new ±text):\n```\n{}```\n", d));
            }
            clip(&s, EXCERPT_CHARS)
        }
    }
}

fn thread_part(earlier: &[Msg]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0;
    for m in earlier.iter().rev() {
        let who = if m.author.trim().is_empty() {
            "reviewer"
        } else {
            m.author.as_str()
        };
        let line = format!("{who}: {}\n", clip(m.body.trim(), 600));
        used += line.chars().count();
        if used > THREAD_CHARS && !lines.is_empty() {
            break;
        }
        lines.push(line);
    }
    lines.reverse();
    lines.concat()
}

/// The prompt for `c`, at most [`MAX_CHARS`] characters.
pub fn build(c: &PromptCtx) -> String {
    let mut out = format!("{PREAMBLE}\n\n");
    if !c.title.trim().is_empty() {
        out.push_str(&format!("Review: {}\n", c.title.trim()));
    }
    for r in &c.shas {
        let repo = if r.repo.is_empty() { "repo" } else { r.repo.as_str() };
        out.push_str(&format!("{repo}: {} → {}\n", r.base, r.target));
    }
    if !c.location.is_empty() {
        out.push_str(&format!("The question is on: {}\n", c.location));
    }
    for (title, body) in [
        ("Brief", brief_part(c)),
        ("Excerpt", excerpt_part(&c.excerpt)),
        ("Earlier in this thread", thread_part(&c.earlier)),
    ] {
        if !body.is_empty() {
            out.push_str(&format!("\n## {title}\n{body}"));
        }
    }
    let question = format!("\n## Question\n{}\n", clip(c.question.trim(), QUESTION_CHARS));
    let mut head = clip(&out, MAX_CHARS.saturating_sub(question.chars().count() + 1));
    head.push_str(&question);
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../fixtures/brief/sample.json");

    fn ctx(doc: &BriefDoc) -> PromptCtx<'_> {
        let how2 = &doc.how[1];
        PromptCtx {
            title: "Vouchers at checkout".into(),
            shas: vec![RepoShas {
                repo: "server".into(),
                base: "1111111".into(),
                target: "a1b2c3d".into(),
            }],
            brief: Some(doc),
            file: Some(how2.cites[0].path.clone()),
            section: None,
            location: format!("{}:L27-31", how2.cites[0].path),
            excerpt: Excerpt::Code {
                path: how2.cites[0].path.clone(),
                snapshot: Some("await _uow.SaveAsync();".into()),
                diff: Some("   27    27 +await _uow.SaveAsync();\n".into()),
            },
            earlier: vec![
                Msg {
                    author: String::new(),
                    body: "Is this inside the unit of work?".into(),
                },
                Msg {
                    author: "claude".into(),
                    body: "Yes, see line 27.".into(),
                },
            ],
            question: "Then why does RefreshTotal run after it?".into(),
        }
    }

    #[test]
    fn holds_question_excerpt_intent_touching_how_and_the_thread() {
        let doc: BriefDoc = serde_json::from_str(SAMPLE).unwrap();
        let c = ctx(&doc);
        let p = build(&c);
        assert!(p.starts_with(PREAMBLE));
        assert!(p.contains("Then why does RefreshTotal run after it?"));
        assert!(p.contains("await _uow.SaveAsync();"));
        assert!(p.contains(doc.intent.as_ref().unwrap().text.trim()));
        assert!(p.contains(&format!("[{}] {}", doc.how[1].id, doc.how[1].title)));
        let untouched = doc
            .how
            .iter()
            .find(|h| !h.cites.iter().any(|ct| same_file(&ct.path, c.file.as_deref().unwrap())));
        if let Some(h) = untouched {
            assert!(
                !p.contains(&format!("[{}]", h.id)),
                "a how section that doesn't cite the file stays out"
            );
        }
        assert!(p.contains("reviewer: Is this inside the unit of work?"));
        assert!(p.contains("claude: Yes, see line 27."));
        assert!(p.contains("server: 1111111 → a1b2c3d"));
        assert!(p.chars().count() <= MAX_CHARS);
        assert_eq!(build(&c), p, "deterministic");
    }

    #[test]
    fn never_holds_settings_paths_or_environment_values() {
        let doc: BriefDoc = serde_json::from_str(SAMPLE).unwrap();
        let p = build(&ctx(&doc));
        assert!(!p.contains("settings.json"));
        assert!(!p.to_lowercase().contains("appdata"));
        for (k, v) in std::env::vars() {
            if v.len() >= 6 && !"Vouchers at checkout".contains(&v) {
                assert!(!p.contains(&v), "env {k} leaked");
            }
        }
    }

    #[test]
    fn a_redacted_path_is_withheld_with_a_note() {
        let c = PromptCtx {
            excerpt: Excerpt::Withheld {
                path: "server/appsettings.json".into(),
            },
            question: "What changed here?".into(),
            ..PromptCtx::default()
        };
        let p = build(&c);
        assert!(p.contains("server/appsettings.json is withheld"));
        assert!(!p.contains("```"));
    }

    #[test]
    fn a_brief_question_carries_its_section_and_long_input_is_capped() {
        let doc: BriefDoc = serde_json::from_str(SAMPLE).unwrap();
        let verify = doc.verify.as_ref().map(|v| v.id.clone());
        let c = PromptCtx {
            brief: Some(&doc),
            section: verify.clone().or(Some(doc.how[0].id.clone())),
            excerpt: Excerpt::Code {
                path: "x.cs".into(),
                snapshot: Some("x".repeat(20_000)),
                diff: None,
            },
            earlier: (0..50)
                .map(|i| Msg {
                    author: "claude".into(),
                    body: format!("message {i} {}", "y".repeat(500)),
                })
                .collect(),
            question: "q".repeat(10_000),
            ..PromptCtx::default()
        };
        let p = build(&c);
        assert!(p.chars().count() <= MAX_CHARS + 1);
        let id = c.section.unwrap();
        assert!(p.contains(&format!("[{id}]")));
        assert!(p.contains("message 49"), "the latest messages are kept");
        assert!(!p.contains("message 0 "));
        assert!(p.contains("## Question"));
    }
}
