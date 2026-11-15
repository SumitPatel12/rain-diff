//! Composer modes (Comment and Question), its type picker, `+ suggestion`, the long editor,
//! comments on the Brief, Map nodes and notes, and the events a send or a re-ask raises.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::ops::Range;

use rain_comments::ComposerMode;
use rain_comments::clock::now_iso;
use rain_comments::ops;
use rain_comments::ui::suggestion_block;
use rain_core::command::Command;
use rain_core::comments::{BRIEF_PATH, MAP_PATH, NOTE_PATH_PREFIX, PR_PATH};
use rain_core::{Anchor, Answer, AnswerStatus, Draft, Event, EventKind, Target, TargetKind, Thread, ThreadKind};
use rain_diffview::{LineRange, LineRef, SpanSelection};
use rune::{TextEdit, Tx};
use serde_json::{Value, json};

use super::{App, Overlay};

/// Where the long editor first opens, below the toolbar.
const LONG_AT: (f32, f32) = (220.0, 40.0);

/// A composer on the Brief, a Map node, a note or the whole change: the title it shows and the
/// thread it will start.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactComposer {
    pub title: String,
    pub file_path: String,
    pub target: Target,
}

#[derive(Clone, Debug, Default)]
pub struct ComposerUi {
    pub mode: ComposerMode,
    pub kind: ThreadKind,
    pub blocking: bool,
    /// The kind dropdown's list is open.
    pub kind_menu: bool,
    /// The long editor (`⤢`) is open in its floating window.
    pub long: bool,
    pub long_at: Option<(f32, f32)>,
    drag_from: Option<(f32, f32)>,
    /// Open on an artifact rather than a line.
    pub artifact: Option<ArtifactComposer>,
    /// The thread whose dismiss reasons are showing.
    pub dismissing: Option<String>,
    pub hovered: Option<String>,
    /// Files whose re-anchor strip is folded.
    pub folded_strips: BTreeSet<usize>,
    /// The long editor drawn as a compact card beside the selection it was opened from.
    pub card: bool,
    pub sel: SelectionUi,
}

/// The mouse paths onto a selection in the diff: its toolbar and the right-click menu.
#[derive(Clone, Debug, Default)]
pub struct SelectionUi {
    /// The toolbar shows while the diff has the span or lines it was raised for.
    pub toolbar: bool,
    /// Where a right-click on the diff opened the menu, in window points.
    pub menu: Option<(f32, f32)>,
    /// A text selection a scene asks for: the active diff's file, the line and its byte range.
    pub pending: Option<(usize, LineRef, Range<usize>)>,
    /// What a scene does with that selection once the toolbar has been placed.
    pub later: Option<Later>,
    /// Where the toolbar last put a card opened from it, and the middle of the selection's
    /// last box, in window points.
    pub seen: Cell<Option<[f32; 4]>>,
}

/// A scene's next step on its selection: Comment or Ask from the toolbar, or a right-click.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Later {
    Compose(ComposerMode),
    Menu,
}

impl ComposerUi {
    pub fn long_position(&self) -> (f32, f32) {
        self.long_at.unwrap_or(LONG_AT)
    }
}

/// The title of an artifact thread's card and composer: `Brief › How 2`, `Map › node voucher-service`,
/// `Note k7p2qa`, `Review (whole change)`.
pub(crate) fn artifact_title(file_path: &str, target: Option<&Target>) -> String {
    let kind = target.map_or_else(|| ops::target_from_path(file_path).map_or(TargetKind::Line, |t| t.kind), |t| t.kind);
    match kind {
        TargetKind::Brief => match target.and_then(|t| t.section.as_deref()) {
            Some(s) => format!("Brief › {}", section_title(s)),
            None => "Brief".to_owned(),
        },
        TargetKind::Node => match target.and_then(|t| t.node.as_deref()) {
            Some(n) => format!("Map › node {n}"),
            None => "Map".to_owned(),
        },
        TargetKind::Note => {
            let id = target
                .and_then(|t| t.note.clone())
                .or_else(|| file_path.strip_prefix(NOTE_PATH_PREFIX).map(str::to_owned))
                .unwrap_or_default();
            format!("Note {id}")
        }
        TargetKind::Pr => "Review (whole change)".to_owned(),
        TargetKind::File => format!("{file_path} (file)"),
        TargetKind::Line => file_path.to_owned(),
    }
}

/// `how.2` → `How 2`, `intent` → `Intent`, `not-covered` → `Not covered`.
fn section_title(id: &str) -> String {
    let words = id.replace(['.', '-'], " ");
    let mut c = words.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}

impl App {
    /// Annotation ids of unsent drafts: `draft:<index into the session's drafts>`.
    pub const DRAFT_PREFIX: &'static str = "draft:";

    /// Where a thread sits, for its card: the artifact's title or the thread's path.
    pub fn thread_title(t: &Thread) -> String {
        artifact_title(&t.file_path, t.target.as_ref())
    }

    /// Scene setups: `composer` (Comment mode, an issue), `composer-question`, `thread-states`
    /// and `orphan`.
    pub fn composer_scene(&mut self, name: &str) {
        scene(self, name);
    }

    /// `?` away from a thread: the composer in Question mode on the selection or cursor line.
    pub fn open_question(&mut self, tx: &mut Tx) {
        self.comment_in(ComposerMode::Question, tx);
    }

    /// Opens the composer on an artifact: the Brief section, Map node or note `target` names.
    /// The brief and diagram views call it when `c` or `?` is pressed on them.
    pub fn comment_on_artifact(&mut self, target: Target, mode: ComposerMode) {
        let file_path = match target.kind {
            TargetKind::Brief => BRIEF_PATH.to_owned(),
            TargetKind::Node => MAP_PATH.to_owned(),
            TargetKind::Pr => PR_PATH.to_owned(),
            TargetKind::Note => rain_core::comments::note_path(target.note.as_deref().unwrap_or_default()),
            TargetKind::File | TargetKind::Line => {
                self.status = "File and line comments open on the diff".into();
                return;
            }
        };
        if self.composer.is_some() {
            self.cancel_composer();
        }
        let title = artifact_title(&file_path, Some(&target));
        let draft = self
            .mirror
            .drafts
            .iter()
            .find(|d| d.file_path == file_path && d.target.as_ref() == Some(&target))
            .cloned();
        self.composer_ui.mode = mode;
        self.composer_ui.kind = ThreadKind::Comment;
        self.composer_ui.blocking = false;
        self.composer_edit = TextEdit::default();
        if let Some(d) = draft {
            self.restore_draft_kind(d.kind);
            let _ = self.composer_edit.paste(&d.text);
        }
        self.composer_edit.focused = true;
        self.composer_ui.artifact = Some(ArtifactComposer { title, file_path, target });
        self.open_overlay(Overlay::Composer);
    }

    /// Ctrl+1 / Ctrl+2 and the tabs.
    pub fn set_composer_mode(&mut self, m: ComposerMode) {
        self.composer_ui.mode = m;
    }

    pub fn set_composer_kind(&mut self, k: ThreadKind) {
        self.composer_ui.kind = k;
        self.composer_ui.kind_menu = false;
        if k != ThreadKind::Issue {
            self.composer_ui.blocking = false;
        }
    }

    pub fn toggle_composer_blocking(&mut self) {
        self.composer_ui.blocking = !self.composer_ui.blocking;
    }

    /// `+ suggestion`: a suggestion block holding the selected text, else the anchored lines.
    pub fn insert_suggestion(&mut self) {
        let code = match &self.composer {
            Some(c) if !c.preview.is_empty() => c.preview.clone(),
            Some(c) => self
                .diff
                .files()
                .get(c.file)
                .and_then(|f| ops::anchor_text(f, &c.anchor))
                .unwrap_or_default(),
            None => String::new(),
        };
        let block = suggestion_block(&code);
        let sep = if self.composer_edit.text.is_empty() || self.composer_edit.text.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        let _ = self.composer_edit.paste(&format!("{sep}{block}\n"));
        self.composer_edit.focused = true;
    }

    /// `⤢`: the floating long editor, and back to the inline box; the text stays.
    pub fn toggle_long_editor(&mut self) {
        self.composer_ui.long = !self.composer_ui.long;
        self.composer_ui.card = false;
        if self.composer_ui.long {
            self.open_overlay(Overlay::Composer);
        } else if self.composer_ui.artifact.is_none() {
            self.close_overlay(Overlay::Composer);
        }
        self.composer_edit.focused = true;
    }

    /// Moves the long editor by its title bar; it never goes above the reader's top.
    pub fn drag_long_editor(&mut self, phase: rune::Phase, dx: f32, dy: f32) {
        let at = self.composer_ui.long_position();
        match phase {
            rune::Phase::Down => self.composer_ui.drag_from = Some(at),
            rune::Phase::Move | rune::Phase::Up => {
                let from = self.composer_ui.drag_from.unwrap_or(at);
                self.composer_ui.long_at = Some(((from.0 + dx).max(0.0), (from.1 + dy).max(0.0)));
                if phase == rune::Phase::Up {
                    self.composer_ui.drag_from = None;
                }
            }
        }
    }

    pub(crate) fn restore_draft_kind(&mut self, kind: Option<ThreadKind>) {
        match kind {
            Some(ThreadKind::Question) => self.composer_ui.mode = ComposerMode::Question,
            Some(k) => {
                self.composer_ui.mode = ComposerMode::Comment;
                self.composer_ui.kind = k;
            }
            None => {}
        }
    }

    /// The kind a send from the composer stores, and the blocking flag for an issue.
    pub(crate) fn composer_kind(&self, body: &str) -> (ThreadKind, Option<bool>) {
        match rain_comments::ui::effective_mode(self.composer_ui.mode, body) {
            ComposerMode::Question => (ThreadKind::Question, None),
            ComposerMode::Comment => {
                let k = self.composer_ui.kind;
                (k, (k == ThreadKind::Issue).then_some(self.composer_ui.blocking))
            }
        }
    }

    /// Keeps the unsent text of an artifact composer; empty text drops it.
    pub(crate) fn save_artifact_draft(&mut self, a: &ArtifactComposer, text: &str, kind: ThreadKind) {
        let (path, target) = (a.file_path.clone(), a.target.clone());
        let text = text.to_owned();
        self.mutate(|s| {
            let at = s
                .drafts
                .iter()
                .position(|d| d.file_path == path && d.target.as_ref() == Some(&target));
            match (at, text.is_empty()) {
                (Some(i), true) => {
                    s.drafts.remove(i);
                }
                (Some(i), false) => {
                    s.drafts[i].text = text.clone();
                    s.drafts[i].kind = Some(kind);
                }
                (None, true) => return,
                (None, false) => s.drafts.push(Draft {
                    file_path: path.clone(),
                    text: text.clone(),
                    kind: Some(kind),
                    target: Some(target.clone()),
                    ..Draft::default()
                }),
            }
            s.version += 1;
        });
    }

    pub(crate) fn take_artifact_draft(&mut self, a: &ArtifactComposer) {
        let (path, target) = (a.file_path.clone(), a.target.clone());
        self.mutate(|s| {
            let before = s.drafts.len();
            s.drafts.retain(|d| !(d.file_path == path && d.target.as_ref() == Some(&target)));
            if s.drafts.len() != before {
                s.version += 1;
            }
        });
    }

    /// Stamps the composer's kind on the code draft at `anchor`.
    pub(crate) fn mark_draft_kind(&mut self, repo: &str, path: &str, anchor: rain_core::Anchor, kind: ThreadKind) {
        let (repo, path) = (repo.to_owned(), path.to_owned());
        self.mutate(|s| {
            if let Some(d) = s
                .drafts
                .iter_mut()
                .find(|d| d.repo == repo && d.file_path == path && d.anchor == anchor)
                && d.kind != Some(kind)
            {
                d.kind = Some(kind);
                s.version += 1;
            }
        });
    }

    /// Records a change for `/api/events` readers; the window is the author.
    pub(crate) fn push_ui_event(&self, kind: EventKind, thread: &str, data: Value) {
        self.shared.push_event(Event {
            seq: 0,
            at: now_iso(),
            kind,
            author: None,
            thread: Some(thread.to_owned()),
            data,
        });
        self.shared.notify_waiters();
    }

    /// Tells the answerers that `id` wants an answer: a question queues again; any thread gets
    /// an `ask` event carrying the question and where it sits.
    pub(crate) fn ask_agent(&mut self, id: &str, question: &str) {
        let Some(t) = ops::thread(&self.mirror, id).cloned() else { return };
        if t.kind() == ThreadKind::Question {
            let tid = id.to_owned();
            self.mutate(|s| {
                if let Some(t) = s.threads.iter_mut().find(|t| t.id == tid) {
                    t.answer = Some(Answer::default());
                    s.bump_comments();
                }
            });
        }
        let excerpt = t.code_snapshot.as_ref().map(|c| c.content.clone());
        self.push_ui_event(
            EventKind::Ask,
            id,
            json!({
                "threadId": id,
                "question": question,
                "filePath": t.file_path,
                "repo": t.repo,
                "anchor": t.anchor,
                "target": t.target,
                "excerpt": excerpt,
                "title": App::thread_title(&t),
            }),
        );
        self.status = "Asked Claude: waiting for an answer".into();
    }

    pub(crate) fn cancel_answer(&mut self, id: &str) {
        let tid = id.to_owned();
        let now = now_iso();
        self.mutate(|s| {
            if let Some(a) = s.threads.iter_mut().find(|t| t.id == tid).and_then(|t| t.answer.as_mut())
                && a.status.pending()
            {
                a.status = AnswerStatus::Cancelled;
                a.ended_at = Some(now.clone());
                s.bump_comments();
            }
        });
        self.push_ui_event(EventKind::Answer, id, json!({"status": "cancelled"}));
        self.status = "Answer cancelled".into();
    }

    /// Re-anchors an orphan to the line selection or cursor, quoting where it was in a reply.
    pub(crate) fn attach_orphan(&mut self, id: &str) {
        let a = self.active();
        let Some(sel) = a.selection().or_else(|| a.comment_at_cursor()) else {
            self.status = "Put the cursor on the line it belongs to (j/k), then Attach here".into();
            return;
        };
        let g = self.global(sel.file);
        let Some(f) = self.diff.files().get(g).cloned() else { return };
        let anchor = sel.anchor();
        let repo = self.repo_tag(g).to_owned();
        let snapshot = ops::snapshot_at(&f, &anchor, &f.path);
        let Some(old) = ops::thread(&self.mirror, id).cloned() else {
            return;
        };
        let was = old.orphan.as_ref().map(|o| &o.snapshot).or(old.code_snapshot.as_ref());
        let note = match was {
            Some(w) => format!(
                "Attached here by hand. Before, at {}:\n```\n{}\n```",
                rain_comments::anchor_label(&rain_core::paths::join(&old.repo, &old.file_path), &old.anchor),
                w.content
            ),
            None => "Attached here by hand.".to_owned(),
        };
        let tid = id.to_owned();
        self.mutate(|s| {
            if let Some(t) = s.threads.iter_mut().find(|t| t.id == tid) {
                if t.original.is_none() {
                    t.original = Some(t.anchor);
                }
                t.repo = repo.clone();
                t.file_path = f.path.clone();
                t.anchor = anchor;
                t.code_snapshot = snapshot.clone();
                t.commit = None;
                t.set_anchor_state(rain_core::AnchorState::Placed, None);
                s.bump_comments();
            }
            ops::reply(s, &tid, &note, None);
        });
        self.status = "Attached; the old code is quoted in the thread".into();
    }
}

/// The mouse's way to what `c`, `?`, `Ctrl+C` and the file keys do on a selection in the diff:
/// its toolbar, the card the composer floats in, and the right-click menu.
impl App {
    /// A drag over the code ended with text selected, or that selection cleared.
    pub fn span_selected(&mut self, span: Option<&SpanSelection>) {
        self.composer_ui.sel.toolbar = span.is_some();
        if let Some(sp) = span {
            self.status = format!(
                "Selected {} char(s): c to comment, ? to ask about the selection",
                sp.text.chars().count()
            );
        }
    }

    /// A click, shift-click or drag on the line numbers ended.
    pub fn lines_selected(&mut self, sel: Option<LineRange>) {
        self.composer_ui.sel.toolbar = sel.is_some();
    }

    /// Whether the toolbar has a selection to act on and nothing covers it.
    pub fn selection_toolbar_shown(&self) -> bool {
        let a = self.active();
        self.composer_ui.sel.toolbar
            && self.phase == super::Phase::Ready
            && self.composer.is_none()
            && self.composer_ui.artifact.is_none()
            && (a.span_selection().is_some() || a.selection().is_some())
    }

    /// Esc: the toolbar goes first, the selection stays.
    pub(crate) fn hide_selection_toolbar(&mut self) -> bool {
        if self.hide_overview_toolbar() {
            return true;
        }
        let shown = self.selection_toolbar_shown();
        self.composer_ui.sel.toolbar = false;
        self.composer_ui.sel.menu = None;
        shown
    }

    /// Comment or Ask on the selection, as `c` / `?` open it, floated as a card at `at` (window
    /// points) beside the selection; the diff doesn't scroll.
    pub fn compose_on_selection(&mut self, mode: ComposerMode, at: (f32, f32)) {
        self.comment_in(mode, &mut Tx::default());
        self.float_composer(at);
    }

    fn float_composer(&mut self, at: (f32, f32)) {
        if self.composer.is_none() {
            return;
        }
        self.composer_ui.long = true;
        self.composer_ui.card = true;
        self.composer_ui.long_at = Some(at);
        self.open_overlay(Overlay::Composer);
    }

    /// Copy: the selected text, else the selected lines, else the cursor's line.
    pub fn copy_selection(&mut self, tx: &mut Tx) {
        let a = self.active();
        let text = match a.span_selection() {
            Some(sp) => Some(sp.text.clone()),
            None => a.selected_text().or_else(|| {
                let (local, at) = a.cursor()?;
                ops::anchor_text(
                    a.files().get(local)?,
                    &Anchor::Line {
                        side: at.side,
                        line: at.line,
                    },
                )
            }),
        };
        match text {
            Some(t) => {
                let n = t.lines().count().max(1);
                tx.copy(t);
                self.status = if n == 1 {
                    "Copied 1 line".into()
                } else {
                    format!("Copied {n} lines")
                };
            }
            None => self.status = "Nothing to copy: select text or lines, or put the cursor on a line".into(),
        }
    }

    /// A right-click on the diff at window point `at`.
    pub fn open_diff_menu(&mut self, at: (f32, f32)) {
        self.composer_ui.sel.menu = Some(at);
    }

    pub fn close_diff_menu(&mut self) {
        self.composer_ui.sel.menu = None;
    }

    /// A menu item: the cursor moves to the right-clicked `line` (local file, line), then `cmd`
    /// runs as its key runs it. Comment and Ask float their composer at the click.
    pub fn diff_menu_run(&mut self, line: Option<(usize, LineRef)>, cmd: Command, tx: &mut Tx) {
        let at = self.composer_ui.sel.menu.take();
        if let Some(l) = line {
            self.active_mut().set_cursor(Some(l));
        }
        match (cmd, at) {
            (Command::Comment | Command::Ask, Some(at)) => {
                self.run(cmd, &mut Tx::default());
                self.float_composer(at);
            }
            _ => {
                self.run(cmd, tx);
            }
        }
    }

    /// The menu's Copy: the selection, else the right-clicked line.
    pub fn diff_menu_copy(&mut self, line: Option<(usize, LineRef)>, tx: &mut Tx) {
        self.composer_ui.sel.menu = None;
        if let Some(l) = line {
            self.active_mut().set_cursor(Some(l));
        }
        self.copy_selection(tx);
    }

    /// Selects text a scene asked for once the diff has been laid out: bytes `range` of `line` in
    /// the active diff's file `local`.
    pub fn select_later(&mut self, local: usize, line: LineRef, range: Range<usize>) {
        self.composer_ui.sel.pending = Some((local, line, range));
    }

    /// Scenes: text selected on an added line of the scene file, then `later` done with it.
    pub fn selection_scene(&mut self, later: Option<Later>) {
        let g = (0..self.keys.len()).find(|&g| self.diff.files()[g].additions > 3).unwrap_or(0);
        let Some(f) = self.diff.files().get(g) else { return };
        let long: Vec<&rain_core::LineRec> = f
            .lines
            .iter()
            .filter(|l| l.kind == rain_core::LineKind::Added && f.text[l.start as usize..l.end as usize].trim().len() >= 16)
            .collect();
        let (Some(top), Some(l)) = (long.first().map(|l| l.new), long.get(2).or(long.first())) else {
            return;
        };
        let code = &f.text[l.start as usize..l.end as usize];
        let start = code.len() - code.trim_start().len();
        let (n, range) = (l.new, start..code.trim_end().len());
        let Some(local) = self.local(g) else { return };
        self.goto_later(format!("{}:{top}", self.keys[g]));
        self.select_later(local, LineRef::new(rain_core::Side::New, n), range);
        self.composer_ui.sel.later = later;
    }

    pub(crate) fn selection_poll(&mut self, tx: &mut Tx) -> bool {
        if self.diff_width == f32::MAX || self.pending_goto.is_some() {
            return false;
        }
        if let (Some(later), Some([x, y, mx, my])) = (self.composer_ui.sel.later, self.composer_ui.sel.seen.get())
            && self.composer_ui.sel.pending.is_none()
            && self.selection_toolbar_shown()
        {
            self.composer_ui.sel.later = None;
            match later {
                Later::Compose(mode) => self.compose_on_selection(mode, (x, y)),
                Later::Menu => self.open_diff_menu((mx, my)),
            }
            return true;
        }
        let Some((local, line, range)) = self.composer_ui.sel.pending.take() else {
            return false;
        };
        let a = self.active();
        let Some(row) = a.row_of_line(local, line).and_then(|r| a.row_key(r as usize)) else {
            return false;
        };
        let cell = match (a.layout(local), line.side) {
            (rain_diffview::Layout::Split, rain_core::Side::Old) => "/old",
            (rain_diffview::Layout::Split, rain_core::Side::New) => "/new",
            _ => "",
        };
        tx.select_text(format!("{row}{cell}/code/text"), range);
        true
    }
}

fn scene(s: &mut App, name: &str) {
    match name {
        "composer" | "composer-question" => {
            let g = (0..s.keys.len()).find(|&g| s.diff.files()[g].additions > 3).unwrap_or(0);
            let Some(f) = s.diff.files().get(g) else { return };
            let news: Vec<u32> = f.lines.iter().filter(|l| l.new != 0).map(|l| l.new).collect();
            let Some(&n) = news.get(2).or(news.first()) else { return };
            let at = rain_diffview::LineRef::new(rain_core::Side::New, n);
            let Some(local) = s.local(g) else { return };
            s.active_mut().set_cursor(Some((local, at)));
            let mut tx = Tx::default();
            if name == "composer" {
                s.comment(&mut tx);
                s.set_composer_kind(ThreadKind::Issue);
                s.composer_ui.blocking = true;
                let _ = s
                    .composer_edit
                    .paste("The voucher amount is never checked against the account total.");
            } else {
                s.open_question(&mut tx);
                let _ = s.composer_edit.paste("Why isn't RefreshTotal inside the same unit of work?");
            }
            s.goto_later(format!("{}:{n}", s.keys[g]));
        }
        "thread-states" | "orphan" => {
            crate::seed_threads(s);
            seed_states(s, name == "orphan");
        }
        _ => {}
    }
}

/// A question being answered, one answered, an addressed thread and (for `orphan`) an orphan.
fn seed_states(s: &mut App, orphan: bool) {
    let Some(first) = s.mirror.threads.first().cloned() else { return };
    let (repo, path, anchor) = (first.repo.clone(), first.file_path.clone(), first.anchor);
    let started = rain_comments::clock::iso_from_millis(rain_comments::ui::iso_millis(&now_iso()).unwrap_or(0) - 12_000);
    let ids: Vec<String> = s.mirror.threads.iter().map(|t| t.id.clone()).collect();
    s.mutate(|ss| {
        if let Some(id) = ids.get(1) {
            ops::set_state(ss, id, rain_core::ThreadState::Addressed, Some("d4e5f6a1b2"), None);
        }
        let question = ops::ThreadExtra {
            kind: Some(ThreadKind::Question),
            ..ops::ThreadExtra::default()
        };
        let q = ops::add_thread_with(
            ss,
            &repo,
            &path,
            anchor,
            "Why is the voucher stored before the account total is refreshed?",
            None,
            None,
            question.clone(),
        );
        if let Some(t) = ss.threads.iter_mut().find(|t| t.id == q) {
            t.answer = Some(Answer {
                status: AnswerStatus::Answering,
                by: Some("launcher".into()),
                started_at: Some(started.clone()),
                ..Answer::default()
            });
        }
        let done = ops::add_thread_with(
            ss,
            &repo,
            &path,
            anchor,
            "Does the API reject a negative voucher?",
            None,
            None,
            question,
        );
        ops::reply(
            ss,
            &done,
            "Yes: `RecordVoucher` returns 400 when `dto.Amount <= 0` (VouchersController.cs:23).",
            Some("claude"),
        );
        if orphan {
            let o = ops::add_thread(
                ss,
                &repo,
                &path,
                anchor,
                "N2 note: The total used to be summed here; keep the rounding.",
                Some("claude"),
                None,
            );
            if let Some(t) = ss.threads.iter_mut().find(|t| t.id == o) {
                let snap = rain_core::CodeSnapshot {
                    content: "var total = calc.Sum(payments);\nreturn Math.Round(total, 2);".into(),
                    path: Some(rain_core::paths::join(&repo, &path)),
                    lines: Some((50, 51)),
                    side: Some(rain_core::Side::New),
                    ..rain_core::CodeSnapshot::default()
                };
                t.code_snapshot = Some(snap.clone());
                t.set_anchor_state(rain_core::AnchorState::Orphaned, None);
                t.orphan = Some(rain_core::Orphan {
                    reason: rain_core::OrphanReason::ContentRewritten,
                    last_seen_sha: Some("a1b2c3d4e5".into()),
                    snapshot: snap,
                    at: now_iso(),
                });
            }
        }
    });
    let all: Vec<String> = s.mirror.threads.iter().map(|t| t.id.clone()).collect();
    s.expanded.extend(all.iter().skip(2).cloned());
    s.focused_thread = all.get(1).cloned();
    s.rebuild_annotations();
    if let Some(g) = s.file_of_thread(&first) {
        s.goto_later(s.keys[g].clone());
    }
}
