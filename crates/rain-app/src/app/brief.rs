//! The Brief behind the Overview tab: the facts from triage, cites that jump into the diff, the
//! check against the loaded diff, and comments or questions on its sections, on Map nodes and on
//! notes (artifact threads).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use rain_comments::ops::{self, ThreadExtra};
use rain_core::brief::BriefDoc;
use rain_core::command::Command;
use rain_core::comments::{BRIEF_PATH, MAP_PATH, note_path};
use rain_core::{Anchor, Event, EventKind, FileTag, Side, Target, TargetKind, Thread, ThreadKind, ThreadState};
use rain_diffview::LineRef;
use rain_overview::brief::{BriefCheck, CheckInput, Step};
use rain_triage::{Facts, GroupCount, Triage};
use rune::{TextEdit, Tx};

use super::App;
use super::layout::Overlay;
use super::overview::{PeekState, ReaderTab};

/// The composer's two modes: a review comment, or a question the agent answers inline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComposeMode {
    #[default]
    Comment,
    Question,
}

/// An open composer on an artifact: what it targets and the title it shows (`Brief › How 2`).
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactDraft {
    pub target: Target,
    pub title: String,
    pub mode: ComposeMode,
}

#[derive(Default)]
pub struct BriefState {
    /// The section `c` and `?` act on, while the reader's cursor and selection stay where they
    /// were when it was focused.
    pub focus: Option<String>,
    focus_at: Option<ReaderSpot>,
    /// The floating Verify checklist folded to its pill.
    pub verify_shut: bool,
    pub show_not_covered: bool,
    pub composer: Option<ArtifactDraft>,
    pub edit: TextEdit,
    pub facts: Facts,
    pub groups: Vec<GroupCount>,
    /// Files a brief need not cite: moves, formatting-only, generated, lockfiles, huge, binary.
    pub noise: BTreeSet<String>,
    /// `None` until the first load picks one.
    pub tab: Option<ReaderTab>,
    pub peek: PeekState,
    /// `(section id, selected text)` of the last text selection in an Overview section body.
    pub selection: Option<(String, String)>,
    /// The section a right-click opened its menu on, and where, in window points.
    pub menu: Option<(String, (f32, f32))>,
    /// The section under the pointer, which shows its Comment / Ask.
    pub hover_section: Option<String>,
    /// The next pending jump puts the reader back after a reload: it keeps the tab.
    pub(crate) restore_tab: bool,
    /// Lines each file shows, from the last load.
    shown: Arc<Shown>,
    /// [`App::brief_check`] as of a mirror version; dropped by each load.
    check: RefCell<Option<(u64, Option<BriefCheck>)>>,
}

/// The reader's cursor, line selection and text selection, to tell whether the user moved on.
type ReaderSpot = (
    Option<(usize, LineRef)>,
    Option<rain_diffview::LineRange>,
    Option<rain_diffview::SpanSelection>,
);

/// Lines a file shows per side, sorted, for the cite check.
type Shown = Vec<(String, Vec<u32>, Vec<u32>)>;

fn shows(shown: &Shown, key: &str, side: Side, (a, b): (u32, u32)) -> bool {
    shown.iter().find(|(k, ..)| k == key).is_some_and(|(_, old, new)| {
        let v = if side == Side::Old { old } else { new };
        let i = v.partition_point(|&n| n < a);
        v.get(i).is_some_and(|&n| n <= b.max(a))
    })
}

impl App {
    pub fn brief_doc(&self) -> Option<&BriefDoc> {
        self.mirror.brief.as_ref()
    }

    /// Keeps the facts and groups of a load, and installs the check `GET /api/brief` reports.
    pub(crate) fn install_brief_facts(&mut self, t: &Triage) {
        self.brief.facts = t.facts.clone();
        self.brief.groups = t.groups.clone();
        self.brief.noise = t
            .items
            .iter()
            .filter(|i| !i.group.is_change() && i.group != rain_triage::Group::Tests)
            .map(|i| i.key.clone())
            .collect();
        let shown: Arc<Shown> = Arc::new(self.shown_lines());
        self.brief.shown = shown.clone();
        self.brief.check.replace(None);
        let keys = Arc::new(self.keys.clone());
        let noise = Arc::new(self.brief.noise.clone());
        let targets: Arc<Vec<(String, String)>> = Arc::new(self.brief_targets());
        self.shared.set_brief_check(Box::new(move |doc| {
            let shown_fn = |k: &str, side: Side, l: (u32, u32)| shows(&shown, k, side, l);
            let c = rain_overview::brief::check(
                doc,
                &CheckInput {
                    keys: &keys,
                    noise: &noise,
                    targets: &targets,
                    shown: Some(&shown_fn),
                },
            );
            serde_json::to_value(c).unwrap_or_default()
        }));
    }

    fn shown_lines(&self) -> Shown {
        self.diff
            .files()
            .iter()
            .zip(&self.keys)
            .map(|(f, k)| {
                let mut old: Vec<u32> = f.lines.iter().map(|l| l.old).filter(|&n| n > 0).collect();
                let mut new: Vec<u32> = f.lines.iter().map(|l| l.new).filter(|&n| n > 0).collect();
                // A parsed diff's numbers already ascend per side; sort only what doesn't.
                for v in [&mut old, &mut new] {
                    if !v.is_sorted() {
                        v.sort_unstable();
                    }
                }
                (k.clone(), old, new)
            })
            .collect()
    }

    /// `(repo id, loaded target sha)` per repo, as the session records them.
    fn brief_targets(&self) -> Vec<(String, String)> {
        self.mirror.repos.iter().map(|r| (r.id.clone(), r.target.clone())).collect()
    }

    /// The brief against the diff on screen; `None` without a brief. Computed again only when
    /// the session or the load changed: the Overview draws it on every commit.
    pub fn brief_check(&self) -> Option<BriefCheck> {
        if let Some((v, c)) = &*self.brief.check.borrow()
            && *v == self.mirror.version
        {
            return c.clone();
        }
        let c = self.compute_brief_check();
        self.brief.check.replace(Some((self.mirror.version, c.clone())));
        c
    }

    fn compute_brief_check(&self) -> Option<BriefCheck> {
        let doc = self.brief_doc()?;
        let shown = &self.brief.shown;
        let shown_fn = |k: &str, side: Side, l: (u32, u32)| shows(shown, k, side, l);
        let targets = self.brief_targets();
        Some(rain_overview::brief::check(
            doc,
            &CheckInput {
                keys: &self.keys,
                noise: &self.brief.noise,
                targets: &targets,
                shown: Some(&shown_fn),
            },
        ))
    }

    /// The tour: the brief's own steps, else one per `how[]` cite, each resolved to a key.
    pub fn tour_steps(&self) -> Vec<Step> {
        self.brief_doc()
            .map(|d| rain_overview::brief::tour(d, &self.keys))
            .unwrap_or_default()
    }

    /// Files the brief's `notCovered` globs leave out of its narrative.
    pub fn not_covered_files(&self) -> Vec<String> {
        let Some(n) = self.brief_doc().and_then(|d| d.not_covered.as_ref()) else {
            return Vec::new();
        };
        let set = rain_overview::glob_set(n.paths.iter().map(String::as_str));
        self.keys.iter().filter(|k| set.is_match(k.as_str())).cloned().collect()
    }

    /// Jumps to a cite or a code span: its file, at its first line when it has one.
    pub fn goto_cite(&mut self, path: &str, lines: Option<(u32, u32)>, side: Side, tx: &mut Tx) {
        let Some(g) = rain_overview::brief::resolve(path, &self.keys).and_then(|k| self.index_of(k)) else {
            self.status = format!("{path} is not in this diff");
            return;
        };
        match lines {
            Some((a, _)) => self.goto_line(g, LineRef::new(side, a), tx),
            None => self.goto_file(g, tx),
        }
        if self.status.is_empty() || !self.status.starts_with("Line ") {
            self.status = format!("From the brief: {}", self.keys[g]);
        }
    }

    fn reader_spot(&self) -> ReaderSpot {
        let a = self.active();
        (a.cursor(), a.selection(), a.span_selection().cloned())
    }

    pub fn focus_section(&mut self, id: &str) {
        self.brief.focus = Some(id.to_owned());
        self.brief.focus_at = Some(self.reader_spot());
        if let Some(d) = self.brief_doc() {
            let title = rain_overview::brief::section_title(d, id);
            self.status = crate::keys::with_hints(&title, &[(Command::Comment, "comment"), (Command::Ask, "question")]);
        }
    }

    /// `c`/`?` on an artifact: the focused Brief section, else the Map's selected node. False
    /// when neither has the focus, so the key goes to the diff.
    pub fn artifact_key(&mut self, mode: ComposeMode) -> bool {
        if self.layout.is_open(Overlay::Map)
            && let Some(n) = self.diagram.selected.clone()
        {
            self.comment_on_node(&n, mode);
            return true;
        }
        let brief_visible = self.reader_tab() == ReaderTab::Overview;
        let still = self.brief.focus_at.as_ref() == Some(&self.reader_spot());
        match self.brief.focus.clone() {
            Some(id) if still && brief_visible && self.brief_doc().is_some_and(|d| d.section(&id).is_some()) => {
                self.compose_on_section_selection(&id, mode);
                true
            }
            _ => {
                self.brief.focus = None;
                false
            }
        }
    }

    /// Opens the composer on Brief section `id`; `quote` is selected text in it.
    pub fn comment_on_section(&mut self, id: &str, quote: Option<String>, mode: ComposeMode) {
        let Some(doc) = self.brief_doc() else { return };
        let Some(sec) = doc.section(id) else { return };
        let title = rain_overview::brief::section_title(doc, id);
        let quote = quote.filter(|q| !q.trim().is_empty()).unwrap_or_else(|| sec.title.to_owned());
        self.brief.focus = Some(id.to_owned());
        self.brief.focus_at = Some(self.reader_spot());
        self.open_artifact(
            Target {
                kind: TargetKind::Brief,
                section: Some(id.to_owned()),
                quote: Some(quote),
                ..Target::default()
            },
            title,
            mode,
        );
    }

    /// Opens the composer on Map node `node` of the view on screen.
    pub fn comment_on_node(&mut self, node: &str, mode: ComposeMode) {
        let label = self
            .overview
            .as_ref()
            .and_then(|l| l.document.graph.nodes.iter().find(|n| n.id == node))
            .map_or_else(|| node.to_owned(), |n| n.label.clone());
        let view = self
            .overview
            .as_ref()
            .and_then(|l| l.targets.get(self.diagram.target))
            .and_then(|t| t.view.clone());
        self.open_artifact(
            Target {
                kind: TargetKind::Node,
                node: Some(node.to_owned()),
                view,
                quote: Some(label.clone()),
                ..Target::default()
            },
            format!("Map › {label}"),
            mode,
        );
    }

    /// Opens the composer on note `id` (stored or the overview's).
    pub fn comment_on_note(&mut self, id: &str, mode: ComposeMode) {
        let Some(n) = self.note(id) else { return };
        let title = match n.title.as_deref().filter(|t| !t.trim().is_empty()) {
            Some(t) => format!("Note › {t}"),
            None if n.kind == rain_core::NoteKind::Story => "Note › Story".into(),
            None => format!("Note › {}", n.key()),
        };
        self.open_artifact(
            Target {
                kind: TargetKind::Note,
                note: Some(id.to_owned()),
                ..Target::default()
            },
            title,
            mode,
        );
    }

    fn open_artifact(&mut self, target: Target, title: String, mode: ComposeMode) {
        self.brief.composer = Some(ArtifactDraft { target, title, mode });
        self.brief.edit = TextEdit::default();
        self.brief.edit.focused = true;
        self.status = match mode {
            ComposeMode::Comment => "Comment · Ctrl+Enter sends · Esc cancels".into(),
            ComposeMode::Question => "Question · Claude answers here · Ctrl+Enter sends".into(),
        };
    }

    pub fn set_artifact_mode(&mut self, mode: ComposeMode) {
        if let Some(c) = &mut self.brief.composer {
            c.mode = mode;
            self.brief.edit.focused = true;
        }
    }

    pub fn cancel_artifact(&mut self) -> bool {
        let open = self.brief.composer.take().is_some();
        self.brief.edit = TextEdit::default();
        open
    }

    /// Posts the composer's text on its artifact: a comment, or a question (also when the text
    /// mentions `@claude`) whose answer is queued and announced to the agent.
    pub fn submit_artifact(&mut self, body: String) {
        let Some(c) = self.brief.composer.take() else { return };
        self.brief.edit = TextEdit::default();
        let question = c.mode == ComposeMode::Question || body.contains("@claude");
        let file_path = match c.target.kind {
            TargetKind::Brief => BRIEF_PATH.to_owned(),
            TargetKind::Node => MAP_PATH.to_owned(),
            TargetKind::Note => note_path(c.target.note.as_deref().unwrap_or_default()),
            _ => return,
        };
        let target = c.target.clone();
        let id = self.mutate(|s| {
            let snapshot = ops::artifact_snapshot(s, &target);
            ops::add_thread_with(
                s,
                "",
                &file_path,
                Anchor::Line { side: Side::New, line: 1 },
                &body,
                None,
                snapshot,
                ThreadExtra {
                    kind: Some(if question { ThreadKind::Question } else { ThreadKind::Comment }),
                    target: Some(target.clone()),
                    ..ThreadExtra::default()
                },
            )
        });
        if question {
            self.shared.push_event(Event {
                at: rain_comments::clock::now_iso(),
                kind: EventKind::Ask,
                thread: Some(id.clone()),
                data: serde_json::json!({ "question": body, "target": target, "title": c.title }),
                ..Event::default()
            });
        }
        self.focused_thread = Some(id.clone());
        self.expanded.insert(id);
        self.status = if question {
            format!("Question on {} sent · waiting for an answer", c.title)
        } else {
            format!("Comment on {} saved", c.title)
        };
    }

    /// Threads on Brief section `id`, oldest first; resolved and dismissed ones left out.
    pub fn section_threads(&self, id: &str) -> Vec<&Thread> {
        self.artifact_threads(|t| t.kind == TargetKind::Brief && t.section.as_deref() == Some(id))
    }

    /// Threads on Map node `node`.
    pub fn node_threads(&self, node: &str) -> Vec<&Thread> {
        self.artifact_threads(|t| t.kind == TargetKind::Node && t.node.as_deref() == Some(node))
    }

    /// Threads on note `id`.
    pub fn note_threads(&self, id: &str) -> Vec<&Thread> {
        self.artifact_threads(|t| t.kind == TargetKind::Note && t.note.as_deref() == Some(id))
    }

    fn artifact_threads(&self, pick: impl Fn(&Target) -> bool) -> Vec<&Thread> {
        self.mirror
            .threads
            .iter()
            .filter(|t| matches!(t.state, ThreadState::Open | ThreadState::Addressed))
            .filter(|t| t.target.as_ref().is_some_and(&pick))
            .collect()
    }

    /// Where an artifact thread sits, as its card and readback name it.
    pub fn artifact_title(&self, t: &Thread) -> String {
        let Some(target) = &t.target else { return t.file_path.clone() };
        match target.kind {
            TargetKind::Brief => {
                let id = target.section.as_deref().unwrap_or_default();
                match self.brief_doc().filter(|d| d.section(id).is_some()) {
                    Some(d) => rain_overview::brief::section_title(d, id),
                    None => format!("Brief › {id} (removed)"),
                }
            }
            TargetKind::Node => {
                let id = target.node.as_deref().unwrap_or_default();
                let label = self
                    .overview
                    .as_ref()
                    .and_then(|l| l.document.graph.nodes.iter().find(|n| n.id == id))
                    .map_or_else(|| id.to_owned(), |n| n.label.clone());
                format!("Map › {label}")
            }
            TargetKind::Note => match target.note.as_deref().and_then(|id| self.note(id)) {
                Some(n) => match n.title.as_deref().filter(|x| !x.trim().is_empty()) {
                    Some(x) => format!("Note › {x}"),
                    None => format!("Note › {}", if n.key().is_empty() { "Story".to_owned() } else { n.key() }),
                },
                None => "Note (removed)".into(),
            },
            TargetKind::Pr => "Review (whole change)".into(),
            _ => t.file_path.clone(),
        }
    }

    /// Brief threads whose section is gone: shown at the end of the card with their snapshot.
    pub fn orphaned_brief_threads(&self) -> Vec<&Thread> {
        self.artifact_threads(|t| t.kind == TargetKind::Brief)
            .into_iter()
            .filter(|t| {
                t.orphan.is_some()
                    || self.brief_doc().is_none_or(|d| {
                        t.target
                            .as_ref()
                            .and_then(|x| x.section.as_deref())
                            .is_none_or(|id| d.section(id).is_none())
                    })
            })
            .collect()
    }

    /// "Refresh brief": asks the agent, through the event stream, for a brief against the diff on
    /// screen.
    pub fn refresh_brief(&mut self, _tx: &mut Tx) {
        let shas: Vec<serde_json::Value> = self
            .brief_targets()
            .into_iter()
            .map(|(repo, sha)| serde_json::json!({ "repo": repo, "sha": sha }))
            .collect();
        self.shared.push_event(Event {
            at: rain_comments::clock::now_iso(),
            kind: EventKind::Brief,
            data: serde_json::json!({ "request": "refresh", "loaded": shas }),
            ..Event::default()
        });
        self.shared.notify_waiters();
        self.status = "Asked the agent for a brief against the loaded commits".into();
    }

    /// Whether file `key` is a real change (not a group a reviewer accepts unread).
    pub fn is_noise(&self, key: &str) -> bool {
        self.brief.noise.contains(key)
            || matches!(
                self.tags.get(key),
                Some(FileTag::Generated | FileTag::FormattingOnly | FileTag::Ignored)
            )
    }
}

/// The brief the `brief` and `map` scenes show, written for the story fixture.
const SCENE_BRIEF: &str = include_str!("../../../../fixtures/brief/story.json");

impl App {
    /// Scenes: the story brief on the Overview with a comment on `how.2` and an answered
    /// question on `how.1`, `how.2` focused, with a Question composer under it when asked.
    pub(crate) fn scene_brief(&mut self, composer: bool) {
        let Ok(mut doc) = serde_json::from_str::<BriefDoc>(SCENE_BRIEF) else {
            return;
        };
        doc.assign_ids();
        self.mutate(|s| {
            s.brief = Some(doc);
            s.bump_brief();
        });
        self.comment_on_section("how.2", Some("same unit of work".into()), ComposeMode::Comment);
        self.submit_artifact("Is the payment saved in this unit of work too, or only the voucher?".into());
        self.comment_on_section("how.1", None, ComposeMode::Question);
        self.submit_artifact("Where is a negative amount rejected?".into());
        if let Some(q) = self.mirror.threads.last().map(|t| t.id.clone()) {
            self.mutate(|s| {
                ops::reply(
                    s,
                    &q,
                    "In `VouchersController.cs:23`: amounts ≤ 0 return 400 before the service is called.",
                    Some("claude"),
                )
            });
        }
        self.focus_section("how.2");
        self.comment_on_section("how.2", None, ComposeMode::Question);
        if !composer {
            self.cancel_artifact();
        }
        self.set_reader_tab(ReaderTab::Overview);
        self.status.clear();
    }

    /// Scene: the Map open on the story overview, a node selected with a thread, one of its files
    /// viewed.
    pub(crate) fn scene_map(&mut self) {
        self.scene_brief(false);
        if let Some(g) = self.index_of("server/src/Services/VoucherService.cs") {
            self.set_viewed(g, true);
        }
        self.toggle_map();
        self.diagram.selected = Some("voucher-service".into());
        self.comment_on_node("voucher-service", ComposeMode::Comment);
        self.submit_artifact("Could this record the voucher asynchronously?".into());
        self.status.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_lines_answer_by_range_and_side() {
        let shown: Shown = vec![("a.cs".into(), vec![3, 4], vec![10, 11, 12])];
        assert!(shows(&shown, "a.cs", Side::New, (12, 20)));
        assert!(shows(&shown, "a.cs", Side::New, (1, 10)));
        assert!(!shows(&shown, "a.cs", Side::New, (13, 20)));
        assert!(shows(&shown, "a.cs", Side::Old, (4, 4)));
        assert!(!shows(&shown, "b.cs", Side::New, (10, 10)));
    }
}
