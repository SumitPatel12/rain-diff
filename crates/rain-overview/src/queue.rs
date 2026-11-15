//! The "Needs you" queue: what to look at next, most important first, done items sunk to the end.

use std::collections::{BTreeMap, BTreeSet};

use rain_core::FileTag;

use crate::tags::{GROUP_TAGS, tag_of};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueKind {
    File { repo_path: String, tag: FileTag },
    Thread { id: String, repo_path: String },
    Group { tag: FileTag, files: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueItem {
    pub kind: QueueKind,
    pub needs_you: bool,
}

/// A thread whose last word is not the user's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitingThread {
    pub id: String,
    pub repo_path: String,
}

/// Order: unviewed must-review files, each followed by the threads waiting on it; every other
/// waiting thread; unviewed review, uncovered, then skim files; one row per non-empty group
/// (formatting-only, generated, ignored) with unviewed files; then viewed files and fully viewed groups.
/// A viewed file with waiting threads stays in place so its threads keep their anchor.
pub fn queue(
    files_in_reading_order: &[String],
    tags: &BTreeMap<String, FileTag>,
    is_viewed: impl Fn(&str) -> bool,
    threads_waiting: &[WaitingThread],
) -> Vec<QueueItem> {
    let mut by_file: BTreeMap<&str, Vec<&WaitingThread>> = BTreeMap::new();
    for t in threads_waiting {
        by_file.entry(t.repo_path.as_str()).or_default().push(t);
    }
    let thread = |t: &WaitingThread| QueueItem {
        kind: QueueKind::Thread {
            id: t.id.clone(),
            repo_path: t.repo_path.clone(),
        },
        needs_you: true,
    };
    let file = |f: &str, tag, needs_you| QueueItem {
        kind: QueueKind::File {
            repo_path: f.to_owned(),
            tag,
        },
        needs_you,
    };

    let mut out = Vec::new();
    let mut threads_placed: BTreeSet<&str> = BTreeSet::new();
    let mut viewed = Vec::new();

    let reading = |tag: FileTag| files_in_reading_order.iter().filter(move |f| tag_of(tags, f) == tag);
    for f in reading(FileTag::MustReview) {
        let waiting = by_file.get(f.as_str());
        if is_viewed(f) && waiting.is_none() {
            viewed.push(file(f, FileTag::MustReview, false));
            continue;
        }
        out.push(file(f, FileTag::MustReview, !is_viewed(f)));
        for t in waiting.into_iter().flatten() {
            out.push(thread(t));
            threads_placed.insert(&t.id);
        }
    }
    for t in threads_waiting.iter().filter(|t| !threads_placed.contains(t.id.as_str())) {
        out.push(thread(t));
    }
    for tag in [FileTag::Review, FileTag::Uncovered, FileTag::Skim] {
        for f in reading(tag) {
            if is_viewed(f) {
                viewed.push(file(f, tag, false));
            } else {
                out.push(file(f, tag, true));
            }
        }
    }
    let mut done_groups = Vec::new();
    for tag in GROUP_TAGS {
        let files: Vec<String> = reading(tag).cloned().collect();
        if files.is_empty() {
            continue;
        }
        let needs_you = files.iter().any(|f| !is_viewed(f));
        let row = QueueItem {
            kind: QueueKind::Group { tag, files },
            needs_you,
        };
        if needs_you { out.push(row) } else { done_groups.push(row) }
    }
    out.extend(viewed);
    out.extend(done_groups);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> (Vec<String>, BTreeMap<String, FileTag>) {
        let list = [
            ("a/must1.cs", FileTag::MustReview),
            ("a/review1.cs", FileTag::Review),
            ("a/must2.cs", FileTag::MustReview),
            ("a/skim.cs", FileTag::Skim),
            ("a/fmt.cs", FileTag::FormattingOnly),
            ("a/gen.cs", FileTag::Generated),
            ("a/new.cs", FileTag::Uncovered),
            ("a/review2.cs", FileTag::Review),
        ];
        (
            list.iter().map(|(f, _)| (*f).to_owned()).collect(),
            list.iter().map(|(f, t)| ((*f).to_owned(), *t)).collect(),
        )
    }

    fn label(i: &QueueItem) -> String {
        let mark = if i.needs_you { "" } else { " (done)" };
        match &i.kind {
            QueueKind::File { repo_path, .. } => format!("{repo_path}{mark}"),
            QueueKind::Thread { id, .. } => format!("#{id}"),
            QueueKind::Group { tag, files } => format!("[{} {}]{mark}", tag.label(), files.len()),
        }
    }

    #[test]
    fn orders_must_then_threads_then_review_skim_groups() {
        let (f, t) = files();
        let waiting = [
            WaitingThread {
                id: "t1".into(),
                repo_path: "a/must2.cs".into(),
            },
            WaitingThread {
                id: "t2".into(),
                repo_path: "a/skim.cs".into(),
            },
            WaitingThread {
                id: "t3".into(),
                repo_path: "gone/file.cs".into(),
            },
        ];
        let q = queue(&f, &t, |p| p == "a/review1.cs" || p == "a/gen.cs", &waiting);
        assert_eq!(
            q.iter().map(label).collect::<Vec<_>>(),
            [
                "a/must1.cs",
                "a/must2.cs",
                "#t1",
                "#t2",
                "#t3",
                "a/review2.cs",
                "a/new.cs",
                "a/skim.cs",
                "[Formatting only 1]",
                "a/review1.cs (done)",
                "[Generated 1] (done)",
            ]
        );
    }

    #[test]
    fn viewed_must_file_keeps_its_waiting_thread() {
        let (f, t) = files();
        let waiting = [WaitingThread {
            id: "t1".into(),
            repo_path: "a/must1.cs".into(),
        }];
        let q = queue(&f, &t, |p| p == "a/must1.cs" || p == "a/must2.cs", &waiting);
        assert_eq!(label(&q[0]), "a/must1.cs (done)");
        assert_eq!(label(&q[1]), "#t1");
        assert!(q.iter().rev().take(2).any(|i| label(i) == "a/must2.cs (done)"));
    }

    #[test]
    fn everything_viewed_needs_nothing() {
        let (f, t) = files();
        let q = queue(&f, &t, |_| true, &[]);
        assert!(q.iter().all(|i| !i.needs_you));
        assert_eq!(q.len(), 8);
    }
}
