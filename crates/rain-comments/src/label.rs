//! The conventional-comment prefix Claude's review comments start with
//! (`F3 issue (blocking): …`), the glyph per thread kind, and the one-line summary a collapsed
//! card shows.

use rain_core::ThreadKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Issue,
    Suggestion,
    Question,
    Nitpick,
    Praise,
    Note,
    Todo,
    Chore,
}

impl Kind {
    fn parse(word: &str) -> Option<Kind> {
        Some(match word.to_ascii_lowercase().as_str() {
            "issue" | "bug" | "problem" => Kind::Issue,
            "suggestion" | "suggest" => Kind::Suggestion,
            "question" => Kind::Question,
            "nitpick" | "nit" => Kind::Nitpick,
            "praise" => Kind::Praise,
            "note" | "thought" | "info" => Kind::Note,
            "todo" => Kind::Todo,
            "chore" => Kind::Chore,
            _ => return None,
        })
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Kind::Issue => "●",
            Kind::Suggestion => "◆",
            Kind::Question => "?",
            Kind::Nitpick => "·",
            Kind::Praise => "★",
            Kind::Note => "○",
            Kind::Todo | Kind::Chore => "□",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Issue => "issue",
            Kind::Suggestion => "suggestion",
            Kind::Question => "question",
            Kind::Nitpick => "nit",
            Kind::Praise => "praise",
            Kind::Note => "note",
            Kind::Todo => "todo",
            Kind::Chore => "chore",
        }
    }
}

/// The card glyph of a thread kind, matching [`Kind::glyph`] for the prefixes.
pub fn kind_glyph(k: ThreadKind) -> &'static str {
    match k {
        ThreadKind::Comment => "○",
        ThreadKind::Issue => "●",
        ThreadKind::Question => "?",
        ThreadKind::Nit => "·",
        ThreadKind::Suggestion => "◆",
        ThreadKind::Praise => "★",
    }
}

/// A parsed prefix: `F3 issue (blocking): rest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label<'a> {
    /// The finding number (`F3`), when present.
    pub finding: Option<&'a str>,
    pub kind: Kind,
    /// `(blocking)` → true, `(non-blocking)` → false, absent → None.
    pub blocking: Option<bool>,
    /// The body after the prefix.
    pub rest: &'a str,
}

/// The prefix of `body`, when its first line starts with a known kind followed by `:`.
pub fn parse(body: &str) -> Option<Label<'_>> {
    let s = body.trim_start();
    let (finding, s) = match s.split_once(char::is_whitespace) {
        Some((f, rest)) if f.len() > 1 && f.starts_with(['F', 'f']) && f[1..].bytes().all(|b| b.is_ascii_digit()) => {
            (Some(f), rest.trim_start())
        }
        _ => (None, s),
    };
    let word_end = s.find(|c: char| !(c.is_ascii_alphabetic() || c == '-')).unwrap_or(s.len());
    let kind = Kind::parse(&s[..word_end])?;
    let mut s = s[word_end..].trim_start();
    let mut blocking = None;
    if let Some(inner) = s.strip_prefix('(') {
        let close = inner.find(')')?;
        let decor = inner[..close].to_ascii_lowercase();
        blocking = if decor.contains("non-blocking") || decor.contains("nonblocking") {
            Some(false)
        } else if decor.contains("blocking") {
            Some(true)
        } else {
            None
        };
        s = inner[close + 1..].trim_start();
    }
    let rest = s.strip_prefix(':')?.trim_start();
    Some(Label {
        finding,
        kind,
        blocking,
        rest,
    })
}

/// The first sentence of `text`'s first non-empty line as plain text (Markdown's emphasis and
/// code markers dropped), cut to `max` chars with an ellipsis.
pub fn first_sentence(text: &str, max: usize) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.strip_prefix("```suggestion").map_or(line, |_| "suggested change");
    let plain = line.replace("**", "").replace(['`', '*'], "");
    let line = plain.as_str();
    let mut end = line.len();
    for (i, _) in line.match_indices(['.', '!', '?']) {
        if line[i + 1..].starts_with(' ') || i + 1 == line.len() {
            end = i + 1;
            break;
        }
    }
    let s = &line[..end];
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_prefixes() {
        let l = parse("F3 issue (blocking): The voucher is lost. More text").unwrap();
        assert_eq!(l.finding, Some("F3"));
        assert_eq!(l.kind, Kind::Issue);
        assert_eq!(l.blocking, Some(true));
        assert_eq!(l.rest, "The voucher is lost. More text");
        let l = parse("nit (non-blocking): spacing").unwrap();
        assert_eq!((l.kind, l.blocking, l.finding), (Kind::Nitpick, Some(false), None));
        assert_eq!(parse("question: why?").unwrap().kind, Kind::Question);
        assert!(parse("Why was this removed?").is_none());
        assert!(parse("issue without colon").is_none());
    }

    #[test]
    fn first_sentence_cuts() {
        assert_eq!(first_sentence("The voucher is lost. More text", 80), "The voucher is lost.");
        assert_eq!(first_sentence("\n\nv1.2 ships now", 80), "v1.2 ships now");
        assert_eq!(first_sentence("abcdefghij", 5), "abcd…");
        assert_eq!(first_sentence("```suggestion\nx\n```", 80), "suggested change");
        assert_eq!(
            first_sentence("**`ask` events can be skipped** for good. More", 80),
            "ask events can be skipped for good."
        );
    }

    #[test]
    fn every_thread_kind_has_the_prefix_glyph() {
        assert_eq!(kind_glyph(ThreadKind::Issue), Kind::Issue.glyph());
        assert_eq!(kind_glyph(ThreadKind::Question), Kind::Question.glyph());
        assert_eq!(kind_glyph(ThreadKind::Suggestion), Kind::Suggestion.glyph());
        assert_eq!(kind_glyph(ThreadKind::Praise), Kind::Praise.glyph());
        assert_eq!(kind_glyph(ThreadKind::Nit), Kind::Nitpick.glyph());
    }
}
