//! The overview document: load and validate it (difit + PR Lens rules), list what it can draw,
//! split each repo's diff by coverage into review tags, and derive chapters and the "Needs you" queue.
//! Pure model code: no UI, no git.

pub mod brief;
pub mod chapters;
pub mod coverage;
pub mod diagrams;
pub mod paths;
pub mod queue;
pub mod schema;
pub mod tags;
pub mod targets;
pub mod validate;

pub use chapters::{Chapter, ChapterItem, Chapters, MAX_CHAPTERS, chapters, chapters_of_brief};
pub use coverage::{coverage, glob_set, matches_glob, parse_formatting_only, self_repo};
pub use queue::{QueueItem, QueueKind, WaitingThread, queue};
pub use tags::{Budget, Rated, budget, notes_for, rated, reading_order, tag_of, tags};
pub use targets::{ScopedGraph, default_target, resolve_scope, scope_for, tab_names, targets};
pub use validate::{EDGE_KINDS, Loaded, NODE_KINDS, load, load_value};

#[cfg(test)]
pub(crate) mod test_support {
    use rain_core::OverviewDocument;
    use serde_json::Value;

    pub const SAMPLE: &str = include_str!("../../../fixtures/overview/sample-overview.json");
    pub const SAMPLE_R1: &str = include_str!("../../../fixtures/overview/sample-overview-r1.json");
    pub const SAMPLE_SCHEMA: &str = include_str!("../../../fixtures/overview/sample-overview-schema.json");

    pub fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    pub fn sample() -> OverviewDocument {
        crate::load_value(json(SAMPLE)).unwrap().document
    }

    pub fn schema_sample() -> OverviewDocument {
        crate::load_value(json(SAMPLE_SCHEMA)).unwrap().document
    }
}

#[cfg(test)]
mod perf {
    use std::time::Instant;

    use crate::test_support::{SAMPLE_SCHEMA, json, sample};

    #[test]
    #[ignore = "release-only timing"]
    fn validate_and_derive_for_302_files() {
        let value = json(SAMPLE_SCHEMA);
        let t = Instant::now();
        for _ in 0..100 {
            crate::load_value(value.clone()).unwrap();
        }
        println!("overview load_value: {:.3} ms", t.elapsed().as_secs_f64() * 10.0);

        let doc = sample();
        let mut paths: Vec<String> = (0..300).map(|i| format!("src/Gen/File{i}.cs")).collect();
        paths.extend(["src/Api/VouchersController.cs".into(), "src/Services/VoucherService.cs".into()]);
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let t = Instant::now();
        let cov = crate::coverage(&doc, Some("server"), &refs, &refs[..100], &refs[100..150]);
        let tags = crate::tags(&doc, &[cov]);
        let files: Vec<String> = paths.iter().map(|p| format!("server/{p}")).collect();
        let order = crate::reading_order(&doc, &files, &tags);
        let chapters = crate::chapters(&doc, &order, &tags);
        let queue = crate::queue(&order, &tags, |_| false, &[]);
        println!(
            "coverage+tags+order+chapters+queue for 302 files: {:.3} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!((order.len(), chapters.other.len() + 2, queue.len()), (302, 302, 2 + 150 + 2));
    }
}
