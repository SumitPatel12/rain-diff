//! Threads: the composer, replies, dispositions, card actions, copying, and the annotations
//! that place threads, notes, drafts and the composer on their lines.

use rain_comments::ui::mentions_claude;
use rain_comments::{CardAction, ComposerMode, ops};
use rain_core::diff::Side;
use rain_core::{Anchor, AnchorState, Disposition, EventKind, Thread, ThreadKind};
use rain_diffview::{Annotation, LineRange, LineRef};
use rune::{TextEdit, Tx};
use serde_json::json;

use super::conversation::{ToastAction, UNDO_DELETE_FOR};
use super::{App, ComposerState, Overlay};

/// What Ask puts in an empty reply box.
const ASK_SEED: &str = "@claude ";

impl App {
    pub fn rebuild_annotations(&mut self) {
        let mut all: Vec<(usize, Anchor, String)> = Vec::new();
        self.unplaced.clear();
        for t in &self.mirror.threads {
            let Some(f) = self.file_of_thread(t) else { continue };
            if self.tour_hides(&t.id) {
                continue;
            }
            let shown = self.diff.files().get(f).is_some_and(|file| {
                let (first, last) = t.anchor.lines();
                [first, last].iter().all(|&n| ops::line_index(file, t.anchor.side(), n).is_some())
            });
            if t.outdated || !shown {
                self.unplaced.entry(f).or_default().push(t.id.clone());
            } else {
                all.push((f, t.anchor, t.id.clone()));
            }
        }
        all.extend(self.note_annotations());
        all.extend(self.draft_annotations());
        if let Some(c) = &self.composer {
            all.push((c.file, c.anchor, "composer".into()));
        }
        let main: Vec<Annotation> = all.iter().map(|(f, a, id)| Annotation::new(*f, *a, id.clone())).collect();
        self.diff.set_annotations(main);
        for v in self.subsets_mut() {
            let local: Vec<Annotation> = all
                .iter()
                .filter_map(|(f, a, id)| v.files.iter().position(|g| g == f).map(|l| Annotation::new(l, *a, id.clone())))
                .collect();
            v.diff.set_annotations(local);
        }
    }

    /// Unsent drafts on lines the diff shows, except the one the open composer is editing.
    fn draft_annotations(&self) -> Vec<(usize, Anchor, String)> {
        let open = self.composer.as_ref().map(|c| (c.file, c.anchor));
        self.mirror
            .drafts
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.file_path.starts_with('@'))
            .filter_map(|(i, d)| {
                let g = self.index_of(&rain_core::paths::join(&d.repo, &d.file_path)).or_else(|| {
                    (self.repos.len() == 1)
                        .then(|| self.index_of(&rain_core::paths::join(&self.repos[0].id, &d.file_path)))
                        .flatten()
                })?;
                let file = self.diff.files().get(g)?;
                ops::line_index(file, d.anchor.side(), d.anchor.end_line())?;
                (open != Some((g, d.anchor))).then(|| (g, d.anchor, format!("{}{i}", App::DRAFT_PREFIX)))
            })
            .collect()
    }

    /// `c`: opens the composer in Comment mode for what is selected.
    pub fn comment(&mut self, tx: &mut Tx) {
        self.comment_in(ComposerMode::Comment, tx);
    }

    /// Opens the composer in `mode` for what is selected: a text span, else the line selection,
    /// else the cursor's line.
    pub(crate) fn comment_in(&mut self, mode: ComposerMode, tx: &mut Tx) {
        let a = self.active();
        let pick = match a.span_selection() {
            Some(sp) => Some((sp.file, sp.anchor(), sp.text.clone())),
            None => a
                .selection()
                .or_else(|| a.comment_at_cursor())
                .map(|sel| (sel.file, sel.anchor(), String::new())),
        };
        match pick {
            Some((local, anchor, preview)) => {
                let g = self.global(local);
                self.reset_composer_ui(mode);
                self.open_composer(g, anchor, preview, None, tx);
            }
            None => {
                let key = if mode == ComposerMode::Question { "?" } else { "c" };
                self.status = format!("Put the cursor on a line (j/k), select lines or text, then press {key}");
            }
        }
    }

    fn reset_composer_ui(&mut self, mode: ComposerMode) {
        self.composer_ui.mode = mode;
        self.composer_ui.kind = ThreadKind::Comment;
        self.composer_ui.blocking = false;
        self.composer_ui.long = false;
        self.composer_ui.artifact = None;
        self.close_overlay(Overlay::Composer);
    }

    pub fn comment_on(&mut self, lines: LineRange, tx: &mut Tx) {
        let g = self.global(lines.file);
        self.reset_composer_ui(ComposerMode::Comment);
        self.open_composer(g, lines.anchor(), String::new(), None, tx);
    }

    /// The gutter's `?`: the composer in Question mode on `lines`.
    pub fn ask_on(&mut self, lines: LineRange, tx: &mut Tx) {
        let g = self.global(lines.file);
        self.reset_composer_ui(ComposerMode::Question);
        self.open_composer(g, lines.anchor(), String::new(), None, tx);
    }

    /// Whether thread `t` sits on a diff line, where folding sends it to the gutter.
    pub fn folds_to_gutter(&self, t: &Thread) -> bool {
        !t.is_artifact() && t.placement() != AnchorState::Orphaned && self.file_of_thread(t).is_some()
    }

    /// Folds `id` (a thread, or `note:<id>`) into the gutter, or unfolds it.
    pub fn toggle_fold(&mut self, id: &str) {
        if !self.folded.remove(id) {
            self.folded.insert(id.to_owned());
        }
        self.drop_hidden_reply();
    }

    /// A reply box whose card is gone, folded or collapsed lets go of the keyboard, as Esc does.
    pub(crate) fn drop_hidden_reply(&mut self) {
        let Some(id) = &self.reply_to else { return };
        let shown = ops::thread(&self.mirror, id)
            .is_some_and(|t| !self.folded.contains(id) && (self.folds_to_gutter(t) || self.expanded.contains(id)));
        if !shown {
            self.keep_reply_draft();
        }
    }

    pub fn is_folded(&self, id: &str) -> bool {
        self.folded.contains(id)
    }

    /// A gutter mark's press: unfolds `id` and focuses its thread.
    pub fn unfold(&mut self, id: &str) {
        self.folded.remove(id);
        if ops::thread(&self.mirror, id).is_some() {
            self.focused_thread = Some(id.to_owned());
            self.expanded.insert(id.to_owned());
        }
    }

    pub fn open_composer(&mut self, file: usize, anchor: Anchor, preview: String, edit_of: Option<(String, String)>, tx: &mut Tx) {
        let key = &self.keys[file];
        let draft = self
            .mirror
            .drafts
            .iter()
            .find(|d| d.anchor == anchor && rain_core::paths::join(&d.repo, &d.file_path) == *key)
            .map(|d| (d.text.clone(), d.kind));
        self.composer = Some(ComposerState {
            file,
            anchor,
            preview,
            edit_of,
        });
        self.composer_edit = TextEdit::default();
        if let Some((d, kind)) = draft {
            self.restore_draft_kind(kind);
            let _ = self.composer_edit.paste(&d);
        }
        self.composer_edit.focused = true;
        self.rebuild_annotations();
        if let Some(l) = self.local(file) {
            let at = LineRef::new(anchor.side(), anchor.end_line());
            self.active_mut().scroll_to_line(l, at, tx);
        }
    }

    pub fn open_edit(&mut self, thread_id: &str, message_id: &str, tx: &mut Tx) {
        let Some(t) = ops::thread(&self.mirror, thread_id).cloned() else {
            return;
        };
        let Some(m) = t.messages.iter().find(|m| m.id == message_id) else {
            return;
        };
        if m.author.as_deref().is_some_and(|a| !a.trim().is_empty()) {
            self.status = "Only your own messages can be edited".into();
            return;
        }
        let body = m.body.clone();
        let Some(g) = self.file_of_thread(&t) else {
            self.status = "Edit artifact comments from the Conversation".into();
            return;
        };
        self.reset_composer_ui(ComposerMode::Comment);
        self.open_composer(g, t.anchor, String::new(), Some((thread_id.to_owned(), message_id.to_owned())), tx);
        self.composer_edit = TextEdit::default();
        let _ = self.composer_edit.paste(&body);
        self.composer_edit.focused = true;
    }

    /// Closes the composer (inline, long or on an artifact), keeping unsent text as a draft.
    pub fn cancel_composer(&mut self) {
        let text = self.composer_edit.text.trim().to_owned();
        let (kind, _) = self.composer_kind(&text);
        if let Some(a) = self.composer_ui.artifact.take() {
            self.save_artifact_draft(&a, &text, kind);
        }
        if let Some(c) = self.composer.take()
            && !text.is_empty()
            && c.edit_of.is_none()
        {
            let repo = self.repo_tag(c.file).to_owned();
            let path = self.diff.files()[c.file].path.clone();
            self.mutate(|s| ops::set_draft(s, &repo, &path, c.anchor, &text));
            self.mark_draft_kind(&repo, &path, c.anchor, kind);
        }
        self.composer_ui.long = false;
        self.close_overlay(Overlay::Composer);
        self.composer_edit = TextEdit::default();
        self.rebuild_annotations();
    }

    /// Sends the composer: a thread with its kind, target and snapshot; a Question also queues
    /// an answer and raises an `ask` event.
    pub fn submit_composer(&mut self, body: String) {
        let (kind, blocking) = self.composer_kind(&body);
        let extra = ops::ThreadExtra {
            kind: Some(kind),
            blocking,
            target: None,
        };
        let id = if let Some(a) = self.composer_ui.artifact.take() {
            let snapshot = ops::artifact_snapshot(&self.mirror, &a.target);
            self.take_artifact_draft(&a);
            let extra = ops::ThreadExtra {
                target: Some(a.target.clone()),
                ..extra
            };
            self.mutate(|s| ops::add_thread_with(s, "", &a.file_path, Anchor::default(), &body, None, snapshot, extra))
        } else {
            let Some(c) = self.composer.take() else { return };
            let repo = self.repo_tag(c.file).to_owned();
            let f = self.diff.files()[c.file].clone();
            let path = f.path.clone();
            let snapshot = ops::snapshot_at(&f, &c.anchor, &path).map(|mut s| {
                s.language = rain_diffview::language(&path).map(str::to_owned);
                s
            });
            if let Some((t, m)) = &c.edit_of {
                self.mutate(|s| ops::edit_message(s, t, m, &body));
                self.after_send(t.clone(), "Message saved");
                return;
            }
            self.mutate(|s| {
                ops::take_draft(s, &repo, &path, c.anchor);
                let id = ops::add_thread_with(s, &repo, &path, c.anchor, &body, None, snapshot, extra);
                ops::stamp_commit(s, &id);
                id
            })
        };
        if kind == ThreadKind::Question {
            self.ask_agent(&id, &body);
        } else {
            self.push_ui_event(EventKind::Thread, &id, json!({"kind": kind.label()}));
        }
        let msg = if kind == ThreadKind::Question {
            "Question sent: waiting for an answer"
        } else {
            "Comment saved"
        };
        self.after_send(id, msg);
    }

    fn after_send(&mut self, id: String, msg: &str) {
        self.focused_thread = Some(id);
        self.composer_edit = TextEdit::default();
        self.composer_ui.long = false;
        self.close_overlay(Overlay::Composer);
        self.active_mut().set_selection(None);
        self.active_mut().set_span_selection(None);
        self.rebuild_annotations();
        self.status = msg.into();
    }

    /// The thread keys act on: the focused card, else one anchored on the cursor's line.
    pub fn target_thread(&self) -> Option<String> {
        if let Some(t) = &self.focused_thread
            && ops::thread(&self.mirror, t).is_some()
        {
            return Some(t.clone());
        }
        let (local, at) = self.active().cursor()?;
        let g = self.global(local);
        self.mirror
            .threads
            .iter()
            .find(|t| {
                self.file_of_thread(t) == Some(g) && t.anchor.side() == at.side && {
                    let (a, b) = t.anchor.lines();
                    a <= at.line && at.line <= b
                }
            })
            .map(|t| t.id.clone())
    }

    /// `a x ? d` from the window: `x` asks for a reason, `a` confirms a thread that reads as
    /// addressed.
    pub fn dispose(&mut self, d: Disposition) {
        let Some(id) = self.target_thread() else {
            self.status = "No thread here: focus a comment or put the cursor on its line".into();
            return;
        };
        let Some(t) = ops::thread(&self.mirror, &id) else { return };
        let confirm = d == Disposition::Agree && rain_comments::ui::confirmable(t);
        if !confirm && !rain_comments::ui::takes_dispositions(t) {
            self.status = "Your own thread: reply, resolve or delete it".into();
            return;
        }
        let action = match d {
            _ if confirm => CardAction::Confirm,
            Disposition::Dismiss => CardAction::OpenDismiss,
            d => CardAction::Disposition(d),
        };
        let mut tx = Tx::default();
        self.card_action(&id, action, &mut tx);
    }

    pub fn open_reply(&mut self, tx: &mut Tx) {
        match self.target_thread() {
            Some(id) => {
                self.expanded.insert(id.clone());
                self.start_reply(&id, "");
                self.focused_thread = Some(id.clone());
                self.rebuild_annotations();
                self.goto_thread(&id, tx);
            }
            None => self.status = "No thread here to reply to".into(),
        }
    }

    /// Opens `id`'s reply box with the draft it was left with, else `seed`; Ask's `seed` leads a
    /// draft that doesn't mention Claude. A reply open on another thread keeps its text.
    fn start_reply(&mut self, id: &str, seed: &str) {
        self.keep_reply_draft();
        let text = match self.reply_drafts.remove(id) {
            Some(d) if !seed.is_empty() && !mentions_claude(&d) => format!("{seed}{d}"),
            Some(d) => d,
            None => seed.to_owned(),
        };
        self.reply_to = Some(id.to_owned());
        if !text.is_empty() {
            let _ = self.reply_edit.paste(&text);
        }
        self.reply_edit.focused = true;
    }

    /// Closes the reply box, keeping its text for when that thread's reply opens again.
    pub(crate) fn keep_reply_draft(&mut self) {
        if let Some(id) = self.reply_to.take() {
            let text = std::mem::take(&mut self.reply_edit.text);
            if text.trim().is_empty() || text.trim() == ASK_SEED.trim() {
                self.reply_drafts.remove(&id);
            } else {
                self.reply_drafts.insert(id, text);
                self.status = "Reply kept as a draft: Reply on that thread brings it back".into();
            }
        }
        self.reply_edit = TextEdit::default();
    }

    /// Send on an empty composer: says why nothing went.
    pub fn nothing_to_send(&mut self) {
        let what = if self.composer.as_ref().is_some_and(|c| c.edit_of.is_some()) {
            "the message"
        } else if self.composer_ui.mode == ComposerMode::Question {
            "a question"
        } else {
            "a comment"
        };
        self.status = format!("Nothing to send: type {what} first");
    }

    /// Undo on a deleted thread: puts it back where it was, focused. A question whose answer
    /// the delete cut off is asked again.
    pub fn restore_thread(&mut self, t: Thread, at: usize) {
        let id = t.id.clone();
        let asked = t
            .answer
            .as_ref()
            .is_some_and(|a| a.status.pending())
            .then(|| super::ask::last_question(&t));
        if self.mutate(|s| ops::restore_thread(s, t, at)) {
            self.push_ui_event(EventKind::State, &id, json!({"state": "restored"}));
            if let Some(q) = asked {
                self.ask_agent(&id, &q);
            }
            self.focused_thread = Some(id);
            self.rebuild_annotations();
            self.status = "Thread restored".into();
        }
    }

    /// `u`: undoes the latest of a delete whose Undo still shows and the last accept.
    pub fn undo_latest(&mut self, tx: &mut Tx) {
        let delete = self
            .conversation
            .toasts
            .iter()
            .filter_map(|t| match &t.action {
                Some(ToastAction::Restore(_, _, when)) => Some((when.clone(), t.key.clone())),
                _ => None,
            })
            .max();
        let accept = self.mirror.accepted.iter().rev().find(|a| !a.undone).map(|a| a.at.clone());
        match delete {
            Some((when, key)) if accept.is_none_or(|a| a <= when) => self.toast_action(&key, tx),
            _ => self.undo_accept(tx),
        }
    }

    pub fn card_action(&mut self, id: &str, action: CardAction, tx: &mut Tx) {
        if let CardAction::Hover(inside) = action {
            if inside {
                self.composer_ui.hovered = Some(id.to_owned());
            } else if self.composer_ui.hovered.as_deref() == Some(id) {
                self.composer_ui.hovered = None;
            }
            return;
        }
        self.focused_thread = Some(id.to_owned());
        if !matches!(action, CardAction::OpenDismiss | CardAction::Dismiss(_)) {
            self.composer_ui.dismissing = None;
        }
        match action {
            CardAction::Hover(_) => {}
            CardAction::Toggle if ops::thread(&self.mirror, id).is_some_and(|t| self.folds_to_gutter(t)) => {
                self.toggle_fold(id);
            }
            CardAction::Toggle => {
                if !self.expanded.remove(id) {
                    self.expanded.insert(id.to_owned());
                }
                self.drop_hidden_reply();
            }
            CardAction::AskReply => {
                self.folded.remove(id);
                self.expanded.insert(id.to_owned());
                self.start_reply(id, ASK_SEED);
            }
            CardAction::OpenReply => {
                self.folded.remove(id);
                self.expanded.insert(id.to_owned());
                self.start_reply(id, "");
            }
            CardAction::CancelReply => self.keep_reply_draft(),
            CardAction::EmptyReply => self.status = "Nothing to send: type a reply first".into(),
            CardAction::Reply(body) => {
                let saved = self.mutate(|s| ops::reply(s, id, &body, None)).is_some();
                self.reply_to = None;
                self.reply_edit = TextEdit::default();
                if !saved {
                    self.reply_drafts.insert(id.to_owned(), body);
                    self.status = "Reply not sent: the thread is gone".into();
                } else {
                    self.reply_drafts.remove(id);
                    self.status = "Reply saved".into();
                    let question = ops::thread(&self.mirror, id).is_some_and(|t| t.kind() == ThreadKind::Question);
                    if question || mentions_claude(&body) {
                        self.ask_agent(id, &body);
                    } else {
                        self.push_ui_event(EventKind::Reply, id, json!({}));
                    }
                }
            }
            CardAction::Disposition(Disposition::Dismiss) => {
                let on = ops::thread(&self.mirror, id).is_some_and(|t| t.disposition == Some(Disposition::Dismiss));
                if on {
                    self.mutate(|s| {
                        ops::set_disposition(s, id, None);
                        ops::reopen(s, id);
                    });
                    self.status = "Dismissal undone".into();
                } else {
                    self.composer_ui.dismissing = Some(id.to_owned());
                }
            }
            CardAction::Disposition(d) => {
                self.mutate(|s| ops::toggle_disposition(s, id, d));
                let now = ops::thread(&self.mirror, id).and_then(|t| t.disposition);
                self.status = match now {
                    Some(d) => format!("Thread marked {}", d.label()),
                    None => "Disposition cleared".into(),
                };
                self.push_ui_event(EventKind::Disposition, id, json!({"disposition": now}));
                if now == Some(Disposition::Ask) {
                    self.expanded.insert(id.to_owned());
                    self.start_reply(id, ASK_SEED);
                }
            }
            CardAction::OpenDismiss => {
                let on = ops::thread(&self.mirror, id).is_some_and(|t| t.disposition == Some(Disposition::Dismiss));
                if on {
                    return self.card_action(id, CardAction::Disposition(Disposition::Dismiss), tx);
                }
                self.composer_ui.dismissing = Some(id.to_owned());
                self.status = "Dismiss because: pick a reason".into();
            }
            CardAction::Dismiss(reason) => {
                self.mutate(|s| {
                    ops::set_disposition(s, id, Some(Disposition::Dismiss));
                    ops::dismiss(s, id, reason);
                });
                self.composer_ui.dismissing = None;
                self.push_ui_event(EventKind::State, id, json!({"state": "dismissed", "reason": reason}));
                self.status = format!("Dismissed: {}", reason.label());
            }
            CardAction::Confirm => {
                self.mutate(|s| ops::confirm_addressed(s, id));
                self.push_ui_event(EventKind::State, id, json!({"state": "addressed", "confirmed": true}));
                self.status = "Confirmed addressed".into();
            }
            CardAction::Reopen => {
                self.mutate(|s| {
                    ops::reopen(s, id);
                    ops::set_disposition(s, id, None);
                });
                self.push_ui_event(EventKind::State, id, json!({"state": "open"}));
                self.status = "Thread reopened".into();
            }
            CardAction::Resolve => {
                self.mutate(|s| ops::resolve(s, id));
                self.push_ui_event(EventKind::State, id, json!({"state": "resolved"}));
                self.status = "Thread resolved".into();
            }
            CardAction::CloseOrphan => {
                self.mutate(|s| ops::resolve(s, id));
                self.push_ui_event(EventKind::State, id, json!({"state": "resolved", "orphan": true}));
                self.status = "Orphan closed; its last-seen code stays with the thread".into();
            }
            CardAction::Attach => self.attach_orphan(id),
            CardAction::Retry => {
                let q = ops::thread(&self.mirror, id)
                    .and_then(|t| {
                        t.messages
                            .iter()
                            .rev()
                            .find(|m| m.author.as_deref().is_none_or(|a| a.trim().is_empty()))
                    })
                    .map(|m| m.body.clone())
                    .unwrap_or_default();
                self.ask_agent(id, &q);
            }
            CardAction::CancelAnswer => self.cancel_answer(id),
            CardAction::EditMessage(m) => self.open_edit(id, &m, tx),
            CardAction::Delete => {
                let mine = ops::thread(&self.mirror, id)
                    .is_some_and(|t| t.messages.iter().all(|m| m.author.as_deref().is_none_or(|a| a.trim().is_empty())));
                if mine {
                    let gone = self.mutate(|s| {
                        let at = s.threads.iter().position(|t| t.id == id)?;
                        let t = s.threads[at].clone();
                        ops::delete_thread(s, id);
                        Some((t, at))
                    });
                    self.focused_thread = None;
                    self.status = "Thread deleted".into();
                    if let Some((t, at)) = gone {
                        self.push_ui_event(EventKind::State, id, json!({"state": "deleted"}));
                        self.push_toast(
                            format!("deleted-{id}"),
                            "Thread deleted".into(),
                            Some(ToastAction::Restore(Box::new(t), at, rain_comments::clock::now_iso())),
                        );
                        tx.wake_after(UNDO_DELETE_FOR + std::time::Duration::from_millis(20));
                    }
                } else {
                    self.status = "Threads with the agent's messages are resolved or dismissed, not deleted".into();
                }
            }
        }
        self.rebuild_annotations();
    }

    pub fn copy_all(&mut self, tx: &mut Tx) {
        let text = rain_comments::comments_output(&self.mirror, None);
        let n = self.mirror.threads.len();
        tx.copy(text);
        self.status = format!("Copied {n} thread(s) for Claude");
    }
}

/// The side an overview reference's `revision` points at.
pub(crate) fn revision_side(r: Option<rain_core::overview::GraphRevision>) -> Side {
    if r == Some(rain_core::overview::GraphRevision::Base) {
        Side::Old
    } else {
        Side::New
    }
}
