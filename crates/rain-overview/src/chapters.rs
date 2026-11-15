//! Chapters: the overview's walkthrough steps (else its phases, else its risk levels) as at most
//! [`MAX_CHAPTERS`] reading units, each with the diff locations it covers; the rest is "Other changes".

use std::collections::{BTreeMap, BTreeSet};

use rain_core::FileTag;
use rain_core::overview::{
    CiteRef, GraphFileRef, GraphFlow, GraphRevision, GraphViewScope, OverviewDocument, OverviewGraph, RiskLevel, StepFocus, StepStage,
    WalkthroughStep,
};
use rain_core::paths::join;

use crate::tags::{importance, is_group, tag_of};
use crate::targets::{find_view, resolve_scope};

pub const MAX_CHAPTERS: usize = 7;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterItem {
    pub repo_path: String,
    /// 1-based inclusive line range on `revision`, when the overview names one.
    pub lines: Option<(u32, u32)>,
    pub revision: GraphRevision,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    /// Markdown.
    pub body: String,
    pub items: Vec<ChapterItem>,
    pub cites: Vec<CiteRef>,
    /// The most important tag among its items.
    pub tag: FileTag,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chapters {
    pub chapters: Vec<Chapter>,
    /// Diff files in no chapter, in `files` order: the "Other changes" chapter.
    pub other: Vec<String>,
    /// Files in `other` that still need reading (not formatting-only, generated or ignored).
    pub uncovered_count: usize,
}

impl Chapters {
    /// Indexes of the chapters that cover `repo_path`.
    pub fn chapters_of(&self, repo_path: &str) -> Vec<usize> {
        (0..self.chapters.len())
            .filter(|&i| self.chapters[i].items.iter().any(|it| it.repo_path == repo_path))
            .collect()
    }
}

/// The Brief's `how[]` sections as a "by narrative" grouping: each section's title with the diff
/// keys its cites name, in cite order; sections that name no diff file are left out.
pub fn chapters_of_brief(doc: &rain_core::brief::BriefDoc, keys: &[String]) -> Vec<(String, Vec<String>)> {
    doc.how
        .iter()
        .map(|h| (h.title.clone(), crate::brief::section_keys(&h.cites, keys)))
        .filter(|(_, k)| !k.is_empty())
        .collect()
}

struct Collector<'a> {
    in_diff: &'a BTreeSet<&'a str>,
    items: Vec<ChapterItem>,
}

impl Collector<'_> {
    fn path(&mut self, path: &str, lines: Option<(u32, u32)>, revision: GraphRevision) {
        let item = ChapterItem {
            repo_path: path.to_owned(),
            lines,
            revision,
        };
        if self.in_diff.contains(path) && !self.items.contains(&item) {
            self.items.push(item);
        }
    }
    fn files<'f>(&mut self, files: impl IntoIterator<Item = &'f GraphFileRef>) {
        for f in files {
            let lines = f.start_line.map(|s| (s, f.end_line.unwrap_or(s)));
            self.path(&f.path, lines, f.revision.unwrap_or_default());
        }
    }
    fn messages<'f>(&mut self, flows: impl IntoIterator<Item = &'f GraphFlow>, only: Option<&[String]>) {
        for f in flows {
            for m in f.messages.iter().filter(|m| only.is_none_or(|ids| ids.contains(&m.id))) {
                self.files(&m.files);
            }
        }
    }
}

fn stage_flows<'a>(g: &'a OverviewGraph, stage: Option<&StepStage>) -> Vec<&'a GraphFlow> {
    match stage {
        Some(StepStage::Flow { flow }) => g.flows.iter().filter(|f| &f.id == flow).collect(),
        Some(StepStage::View { view }) => match find_view(&g.views, view).map(|v| &v.scope) {
            Some(GraphViewScope::All) => g.flows.iter().collect(),
            Some(GraphViewScope::Selection { flows, .. }) => g.flows.iter().filter(|f| flows.contains(&f.id)).collect(),
            None => vec![],
        },
        None => vec![],
    }
}

fn node_files<'a>(g: &'a OverviewGraph, id: &'a str) -> impl Iterator<Item = &'a GraphFileRef> {
    g.nodes.iter().filter(move |n| n.id == id).flat_map(|n| n.files.iter())
}

fn table_files<'a>(g: &'a OverviewGraph, id: &'a str) -> impl Iterator<Item = &'a GraphFileRef> {
    g.data_model
        .iter()
        .flat_map(|m| m.tables.iter())
        .filter(move |t| t.id == id)
        .flat_map(|t| t.files.iter().chain(t.indexes.iter().flat_map(|x| x.files.iter())))
}

fn step_items(g: &OverviewGraph, step: &WalkthroughStep, c: &mut Collector) {
    let node_files = |id| node_files(g, id);
    let table_files = |id| table_files(g, id);
    match &step.focus {
        StepFocus::Selection {
            lanes,
            nodes,
            edges,
            messages,
            tables: ts,
        } => {
            for id in nodes {
                c.files(node_files(id));
            }
            for lane in lanes {
                c.files(g.nodes.iter().filter(|n| &n.lane == lane).flat_map(|n| n.files.iter()));
            }
            for id in edges {
                c.files(g.edges.iter().filter(|e| &e.id == id).flat_map(|e| e.files.iter()));
            }
            c.messages(stage_flows(g, step.stage.as_ref()), Some(messages));
            for id in ts {
                c.files(table_files(id));
            }
        }
        StepFocus::All => match &step.stage {
            Some(StepStage::View { view }) => {
                if let Some(v) = find_view(&g.views, view) {
                    let scoped = resolve_scope(g, &v.scope);
                    c.files(scoped.nodes.iter().flat_map(|n| n.files.iter()));
                    c.messages(scoped.flows.iter().copied(), None);
                    for t in scoped.tables {
                        c.files(table_files(&t.id));
                    }
                }
            }
            Some(StepStage::Flow { .. }) => c.messages(stage_flows(g, step.stage.as_ref()), None),
            None => {}
        },
    }
    for cite in step.detail.iter().flat_map(|d| d.cites.iter()) {
        match &cite.reference {
            CiteRef::Node { node } => c.files(node_files(node)),
            CiteRef::Message { flow, message } => c.messages(g.flows.iter().filter(|f| &f.id == flow), Some(std::slice::from_ref(message))),
            CiteRef::Flow { flow } => c.messages(g.flows.iter().filter(|f| &f.id == flow), None),
            CiteRef::File { path } => c.path(path, None, GraphRevision::Head),
            CiteRef::Table { table } | CiteRef::Column { table, .. } => c.files(table_files(table)),
            CiteRef::View { .. } => {}
        }
    }
}

fn chapter_tag(items: &[ChapterItem], tags: &BTreeMap<String, FileTag>) -> FileTag {
    items
        .iter()
        .map(|i| tag_of(tags, &i.repo_path))
        .min_by_key(|t| importance(*t))
        .unwrap_or(FileTag::Skim)
}

/// `files` are the diff's `<repoId>/<path>` keys, ideally in reading order.
pub fn chapters(doc: &OverviewDocument, files: &[String], tags: &BTreeMap<String, FileTag>) -> Chapters {
    let in_diff: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let g = &doc.graph;
    let mut chapters = Vec::new();
    let steps = g.walkthrough.as_ref().map(|w| w.steps.as_slice()).unwrap_or_default();
    if !steps.is_empty() {
        for step in steps.iter().take(MAX_CHAPTERS) {
            let mut c = Collector {
                in_diff: &in_diff,
                items: vec![],
            };
            step_items(g, step, &mut c);
            let detail = step.detail.as_ref().map(|d| format!("\n\n{}", d.text)).unwrap_or_default();
            chapters.push(Chapter {
                id: step.id.clone(),
                title: step.heading.clone(),
                body: format!("{}{detail}", step.body),
                tag: chapter_tag(&c.items, tags),
                items: c.items,
                cites: step
                    .detail
                    .iter()
                    .flat_map(|d| d.cites.iter().map(|c| c.reference.clone()))
                    .collect(),
            });
        }
    } else if !doc.phases.is_empty() {
        for phase in doc.phases.iter().take(MAX_CHAPTERS) {
            let mut c = Collector {
                in_diff: &in_diff,
                items: vec![],
            };
            for id in &phase.node_ids {
                c.files(g.nodes.iter().filter(|n| &n.id == id).flat_map(|n| n.files.iter()));
            }
            chapters.push(Chapter {
                id: phase.id.clone(),
                title: phase.title.clone(),
                body: format!("**Today:** {}\n\n**Proposed:** {}", phase.today, phase.proposed),
                tag: chapter_tag(&c.items, tags),
                items: c.items,
                cites: vec![],
            });
        }
    } else {
        chapters = risk_chapters(doc, files, tags);
    }

    let covered: BTreeSet<&str> = chapters.iter().flat_map(|c| c.items.iter().map(|i| i.repo_path.as_str())).collect();
    let other: Vec<String> = files.iter().filter(|f| !covered.contains(f.as_str())).cloned().collect();
    let uncovered_count = other.iter().filter(|f| !is_group(tag_of(tags, f))).count();
    Chapters {
        chapters,
        other,
        uncovered_count,
    }
}

/// No walkthrough and no phases: one chapter per tier of the reading order.
fn risk_chapters(doc: &OverviewDocument, files: &[String], tags: &BTreeMap<String, FileTag>) -> Vec<Chapter> {
    let why: BTreeMap<String, (&str, Option<RiskLevel>)> = doc
        .reading_order
        .iter()
        .flat_map(|o| o.review.iter().map(move |e| (join(&o.repo, &e.path), (e.why.as_str(), e.risk))))
        .collect();
    [
        (FileTag::MustReview, "must-review"),
        (FileTag::Review, "review"),
        (FileTag::Skim, "skim"),
    ]
    .into_iter()
    .filter_map(|(tag, id)| {
        let paths: Vec<&String> = files.iter().filter(|f| why.contains_key(*f) && tag_of(tags, f) == tag).collect();
        (!paths.is_empty()).then(|| Chapter {
            id: id.to_owned(),
            title: tag.label().to_owned(),
            body: paths
                .iter()
                .map(|p| format!("- `{p}`: {}", why[*p].0))
                .collect::<Vec<_>>()
                .join("\n"),
            items: paths
                .iter()
                .map(|p| ChapterItem {
                    repo_path: (*p).clone(),
                    lines: None,
                    revision: GraphRevision::Head,
                })
                .collect(),
            cites: vec![],
            tag,
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::sample;
    use rain_core::overview::Walkthrough;

    fn diff() -> Vec<String> {
        [
            "server/src/Api/VouchersController.cs",
            "server/src/Services/VoucherService.cs",
            "server/src/Services/AccountService.cs",
            "server/src/Data/Voucher.cs",
            "app/src/app/vouchers/voucher-form.component.ts",
            "app/src/app/vouchers/voucher.service.ts",
            "app/src/app/account/account-page.component.ts",
            "server/src/Unlisted.cs",
            "server/src/Migrations/X.Designer.cs",
        ]
        .map(String::from)
        .to_vec()
    }

    fn tags() -> BTreeMap<String, FileTag> {
        let mut t: BTreeMap<String, FileTag> = diff().into_iter().map(|f| (f, FileTag::Review)).collect();
        t.insert("server/src/Api/VouchersController.cs".into(), FileTag::MustReview);
        t.insert("server/src/Unlisted.cs".into(), FileTag::Uncovered);
        t.insert("server/src/Migrations/X.Designer.cs".into(), FileTag::Ignored);
        t
    }

    #[test]
    fn walkthrough_steps_become_chapters() {
        let ch = chapters(&sample(), &diff(), &tags());
        assert_eq!(ch.chapters.len(), 3);
        let first = &ch.chapters[0];
        assert_eq!(
            (first.id.as_str(), first.title.as_str()),
            ("new-endpoint", "Accounts now take a voucher")
        );
        assert!(first.body.ends_with("hands the amount to VoucherService."));
        assert_eq!(first.cites.len(), 3);
        assert_eq!(
            first.items.iter().map(|i| i.repo_path.as_str()).collect::<Vec<_>>(),
            [
                "server/src/Api/VouchersController.cs",
                "app/src/app/vouchers/voucher-form.component.ts",
                "server/src/Services/VoucherService.cs"
            ]
        );
        assert_eq!(first.items[0].lines, Some((12, 30)));
        assert_eq!(first.tag, FileTag::MustReview);
        let flow = &ch.chapters[1];
        assert_eq!(
            flow.items.iter().map(|i| (i.repo_path.as_str(), i.lines)).collect::<Vec<_>>(),
            [
                ("server/src/Api/VouchersController.cs", Some((22, 22))),
                ("server/src/Services/VoucherService.cs", Some((27, 27)))
            ]
        );
        assert_eq!(ch.chapters[2].items[0].repo_path, "server/src/Services/AccountService.cs");
        assert_eq!(
            ch.other,
            [
                "server/src/Data/Voucher.cs",
                "app/src/app/vouchers/voucher.service.ts",
                "app/src/app/account/account-page.component.ts",
                "server/src/Unlisted.cs",
                "server/src/Migrations/X.Designer.cs"
            ]
        );
        assert_eq!(ch.uncovered_count, 4);
        assert_eq!(ch.chapters_of("server/src/Api/VouchersController.cs"), [0, 1]);
    }

    #[test]
    fn caps_at_seven_and_overflows_into_other() {
        let mut doc = sample();
        let template = doc.graph.walkthrough.as_ref().unwrap().steps[2].clone();
        let mut steps: Vec<WalkthroughStep> = (0..7)
            .map(|i| WalkthroughStep {
                id: format!("s{i}"),
                ..template.clone()
            })
            .collect();
        let mut last = doc.graph.walkthrough.as_ref().unwrap().steps[0].clone();
        last.id = "late".into();
        steps.push(last);
        doc.graph.walkthrough = Some(Walkthrough { steps });
        let ch = chapters(&doc, &diff(), &tags());
        assert_eq!(ch.chapters.len(), MAX_CHAPTERS);
        assert!(ch.other.contains(&"server/src/Api/VouchersController.cs".to_owned()));
    }

    #[test]
    fn falls_back_to_phases_then_risk() {
        let mut doc = sample();
        doc.graph.walkthrough = None;
        let ch = chapters(&doc, &diff(), &tags());
        assert_eq!(ch.chapters.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["api", "ui"]);
        assert!(ch.chapters[0].body.starts_with("**Today:**"));
        assert_eq!(ch.chapters[1].items.len(), 3);
        doc.phases.clear();
        let ch = chapters(&doc, &diff(), &tags());
        assert_eq!(
            ch.chapters.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["Must review", "Review"]
        );
        assert_eq!(ch.chapters[0].items.len(), 1);
        assert!(ch.chapters[1].body.contains("- `server/src/Data/Voucher.cs`: new entity"));
        assert_eq!(
            ch.other,
            [
                "app/src/app/account/account-page.component.ts",
                "server/src/Unlisted.cs",
                "server/src/Migrations/X.Designer.cs"
            ]
        );
        assert_eq!(ch.uncovered_count, 2);
    }

    #[test]
    fn files_outside_the_diff_are_not_items() {
        let ch = chapters(&sample(), &["server/src/Unlisted.cs".to_owned()], &tags());
        assert!(ch.chapters.iter().all(|c| c.items.is_empty() && c.tag == FileTag::Skim));
        assert_eq!(ch.other, ["server/src/Unlisted.cs"]);
    }
}
