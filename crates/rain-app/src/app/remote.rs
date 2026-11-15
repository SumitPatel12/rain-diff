//! Requests from the HTTP server for the open window (go to a file, line or thread, spotlight a
//! range, reload), then the Conversation's per-poll checks.

use rain_comments::anchor_label;
use rain_core::{Side, UiRequest};
use rain_diffview::{Align, LineRef};
use rune::Tx;

use super::App;

#[cfg(test)]
thread_local! {
    pub(crate) static APPLY_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl App {
    /// Runs at the end of every ready [`App::poll`]; true when it changed anything.
    pub(crate) fn apply_requests(&mut self, tx: &mut Tx) -> bool {
        #[cfg(test)]
        APPLY_CALLS.with(|c| c.set(c.get() + 1));
        let requests = self.shared.take_requests();
        let mut changed = !requests.is_empty();
        for r in requests {
            match r {
                UiRequest::Goto { path, line, side } => self.remote_goto(&path, line.map(|n| LineRef::new(side, n)), tx),
                UiRequest::GotoThread(id) => self.remote_goto_thread(&id, tx),
                UiRequest::Highlight {
                    path,
                    lines,
                    side,
                    caption,
                    step,
                } => {
                    if !self.tour_highlight(&path, lines, side, caption.clone(), step, tx) {
                        self.remote_highlight(&path, lines, side, caption, step, tx);
                    }
                }
                UiRequest::Reload => self.reload(),
                UiRequest::Focus => self.status = "Claude asks you to look at this window".into(),
                UiRequest::Close => tx.close_window(),
            }
        }
        changed |= self.conversation_tick(tx);
        changed |= self.tour_tick(tx);
        changed
    }

    /// Spotlights a range the agent names and scrolls it into the upper third; moving the
    /// cursor ends the spotlight.
    fn remote_highlight(
        &mut self,
        path: &str,
        lines: (u32, u32),
        side: Side,
        caption: Option<String>,
        step: Option<(u32, u32)>,
        tx: &mut Tx,
    ) {
        let Some(g) = self.file_for_path(path) else {
            self.status = format!("Claude pointed at {path}, which is not in this diff");
            return;
        };
        let (a, b) = (lines.0.min(lines.1), lines.0.max(lines.1));
        self.status.clear();
        self.goto_line(g, LineRef::new(side, a), tx);
        if let Some(l) = self.local(g) {
            self.active_mut().scroll_to_range(l, side, (a, b), Align::Third, tx);
        }
        self.spotlight(g, side, (a, b), caption.clone(), step);
        let mut place = if a == b {
            format!("{}:{a}", self.keys[g])
        } else {
            format!("{}:{a}-{b}", self.keys[g])
        };
        if let Some((i, n)) = step {
            place = format!("{place} · step {i}/{n}");
        }
        if let Some(c) = caption.filter(|c| !c.trim().is_empty()) {
            place = format!("{place} · {}", c.trim());
        }
        self.status = claude_says(&place, &self.status);
    }

    /// A tree key, else the one file with that repo-relative path, else the one file whose path
    /// ends the given one; `None` when it is missing or ambiguous.
    fn file_for_path(&self, path: &str) -> Option<usize> {
        let path = rain_core::paths::normalize(path);
        if let Some(g) = self.index_of(&path) {
            return Some(g);
        }
        let only = |hit: &dyn Fn(&str) -> bool| {
            let mut it = self.diff.files().iter().enumerate().filter(|(_, f)| hit(&f.path)).map(|(g, _)| g);
            let first = it.next()?;
            it.next().is_none().then_some(first)
        };
        let tail = |p: &str| path.ends_with(&format!("/{p}"));
        if self.diff.files().iter().any(|f| f.path == path) {
            return only(&|p| p == path);
        }
        let (repo, _) = rain_core::paths::split_repo_path(&path);
        if self.repos.iter().any(|r| !r.id.is_empty() && r.id == repo) {
            return None;
        }
        only(&tail)
    }

    fn remote_goto(&mut self, path: &str, at: Option<LineRef>, tx: &mut Tx) {
        let Some(g) = self.file_for_path(path) else {
            self.status = format!("Claude pointed at {path}, which is not in this diff");
            return;
        };
        self.status.clear();
        match at {
            Some(at) => self.goto_line(g, at, tx),
            None => self.goto_file(g, tx),
        }
        let place = match at {
            Some(at) => format!("{}:{}", self.keys[g], at.line),
            None => self.keys[g].clone(),
        };
        self.status = claude_says(&place, &self.status);
    }

    fn remote_goto_thread(&mut self, id: &str, tx: &mut Tx) {
        let Some(t) = rain_comments::ops::thread(&self.mirror, id).cloned() else {
            self.status = format!("Claude pointed at thread {id}, which no longer exists");
            return;
        };
        self.status.clear();
        self.goto_thread(id, tx);
        let place = anchor_label(&rain_core::paths::join(&t.repo, &t.file_path), &t.anchor);
        self.status = claude_says(&place, &self.status);
    }
}

fn claude_says(place: &str, warning: &str) -> String {
    if warning.is_empty() {
        format!("Claude: look at {place}")
    } else {
        format!("Claude: look at {place}. {warning}")
    }
}
