//! The reading place: kept across a reload by file key, line text and scroll offset (not by
//! index), saved into the session as it changes (the autosave debounces the write), restored on
//! open, and published for `GET /api/state`.

use std::path::{Path, PathBuf};

use rain_core::session::{LayoutMode, UiState};
use rain_core::{LineKind, ReadingPlace, Side};
use rain_diffview::{Align, DiffMode, LineRef};
use rune::Tx;

use super::App;

/// A place plus what finds it again in a changed diff.
#[derive(Clone, Debug, PartialEq)]
pub struct Captured {
    pub place: ReadingPlace,
    /// The text of the line the place is on, as it read.
    pub text: Option<String>,
    /// How far below the viewport's top that line sat, in points.
    pub dy: Option<f32>,
}

/// What the place is compared by between frames: file, line, side and whole points of scroll.
type PlaceKey = (String, Option<u32>, Side, i64);

impl App {
    /// Where the reader is: the cursor's line, else the first line of the hunk at the top.
    pub fn capture_place(&self) -> Option<Captured> {
        let (g, l, at) = self.place_line()?;
        let a = self.active();
        let scroll = a.scroll_offset();
        let dy = at.and_then(|at| a.row_of_line(l, at)).map(|r| a.row_y(r as usize) - scroll);
        Some(Captured {
            place: ReadingPlace {
                file: self.keys[g].clone(),
                line: at.map(|at| at.line),
                side: at.map_or(Side::New, |at| at.side),
                scroll,
                reader: self.mirror.ui.reader,
                filter: Some(self.tree.filter.text.clone()).filter(|t| !t.is_empty()),
                chapter: None,
            },
            text: at.and_then(|at| a.side_line(l, at.side, at.line)).map(str::to_owned),
            dy,
        })
    }

    /// The current file (full-list and active index) and the place's line in it.
    fn place_line(&self) -> Option<(usize, usize, Option<LineRef>)> {
        let g = self.current()?;
        let a = self.active();
        let l = self.local(g)?;
        let at = match a.cursor().filter(|(cl, _)| *cl == l) {
            Some((_, at)) => Some(at),
            None => a.visible_hunk().filter(|(f, _)| *f == l).and_then(|(f, h)| {
                let hunk = a.files()[f].hunks.get(h)?;
                Some(if hunk.new_count == 0 {
                    LineRef::new(Side::Old, hunk.old_start)
                } else {
                    LineRef::new(Side::New, hunk.new_start)
                })
            }),
        };
        Some((g, l, at))
    }

    /// Puts the reader back at `c`: the same file, the line holding the same text nearest the old
    /// number, at about the same height in the viewport.
    pub fn restore_place(&mut self, c: &Captured, tx: &mut Tx) {
        // A file the lens or filter now hides stays hidden: the reader keeps what it shows.
        let Some(g) = self.index_of(&c.place.file).filter(|&g| self.local(g).is_some()) else {
            return;
        };
        let tab = self.brief.tab.replace(super::ReaderTab::Changes);
        let Some(line) = c.place.line else {
            self.goto_file(g, tx);
            self.brief.tab = tab;
            return;
        };
        let side = c.place.side;
        let n = find_line(&self.diff.files()[g], side, line, c.text.as_deref());
        let at = LineRef::new(side, n);
        self.goto_line(g, at, tx);
        self.brief.tab = tab;
        let Some(l) = self.local(g) else { return };
        let h = self.active().viewport_height();
        let align = match c.dy {
            Some(dy) if h > 0.0 && dy > h * 0.42 => Some(Align::Center),
            Some(dy) if h > 0.0 && dy > h * 0.15 => Some(Align::Third),
            _ => None,
        };
        if let Some(align) = align
            && self.active().cursor() == Some((l, at))
        {
            self.active_mut().scroll_to_range(l, side, (n, n), align, tx);
        }
    }

    /// Writes the place into the session when it moved (no version bump: nothing waits on it)
    /// and publishes the reader's state.
    pub(crate) fn save_place(&mut self) {
        // Compared before capturing, which clones the key, the filter and the line's text.
        let Some((g, _, at)) = self.place_line() else { return };
        let (line, side) = (at.map(|at| at.line), at.map_or(Side::New, |at| at.side));
        let scroll = self.active().scroll_offset() as i64;
        if self
            .reloader
            .saved_place
            .as_ref()
            .is_some_and(|k| k.0 == self.keys[g] && (k.1, k.2, k.3) == (line, side, scroll))
        {
            return;
        }
        let Some(c) = self.capture_place() else { return };
        let key: PlaceKey = (c.place.file.clone(), c.place.line, c.place.side, c.place.scroll as i64);
        self.reloader.saved_place = Some(key);
        self.shared.set_ui_state(self.ui_state(&c));
        if self.reloader.pending_place.is_some() {
            return;
        }
        let line_key = (c.place.file.clone(), c.place.line, c.place.side);
        self.shared.lock().place = Some(c.place.clone());
        self.mirror.place = Some(c.place);
        // A scroll alone rides along with the next save instead of rewriting the file per pixel.
        if self.reloader.touched_place.as_ref() == Some(&line_key) {
            self.reloader.place_unsaved = true;
            return;
        }
        self.reloader.touched_place = Some(line_key);
        self.reloader.place_unsaved = false;
        if let Some(a) = &self.autosave {
            a.touch();
        }
    }

    /// Saves a scroll offset [`App::save_place`] left for the next save, before the window closes.
    pub(crate) fn flush_place(&mut self) {
        if std::mem::take(&mut self.reloader.place_unsaved)
            && let Some(a) = &self.autosave
        {
            a.touch();
            a.flush();
        }
    }

    fn ui_state(&self, c: &Captured) -> UiState {
        UiState {
            file: Some(c.place.file.clone()),
            line: c.place.line,
            side: c.place.side,
            selection: self.active().selection().map(|r| r.anchor().lines()),
            mode: match (self.settings.layout, self.effective_mode()) {
                (crate::settings::ViewLayout::Auto, _) => LayoutMode::Auto,
                (_, DiffMode::Split) => LayoutMode::Split,
                (_, DiffMode::Unified) => LayoutMode::Unified,
            },
            filter: c.place.filter.clone(),
            zoomed: self.layout.is_zoomed(),
            reader: c.place.reader,
        }
    }

    /// The most recently written session file in `dir` (`rain open --last`): its id and path.
    pub fn last_session(dir: &Path) -> Option<(String, PathBuf)> {
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .max_by_key(|(t, _)| *t)
            .and_then(|(_, p)| Some((p.file_stem()?.to_string_lossy().into_owned(), p)))
    }
}

/// The line on `side` whose text is `text`, nearest `line`; `line` itself when none matches.
fn find_line(f: &rain_core::FileDiff, side: Side, line: u32, text: Option<&str>) -> u32 {
    let Some(text) = text else { return line };
    let num = |r: &rain_core::LineRec| if side == Side::Old { r.old } else { r.new };
    let skip = if side == Side::Old { LineKind::Added } else { LineKind::Removed };
    f.lines
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind != skip && num(r) != 0)
        .filter(|(i, _)| f.line_text(*i as u32) == text)
        .map(|(_, r)| num(r))
        .min_by_key(|n| n.abs_diff(line))
        .unwrap_or(line)
}
