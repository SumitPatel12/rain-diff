//! What a diff view shows and where: the rows files, hunks and expanded ranges make, the line
//! selection, the text-span selection, the line cursor, annotations, the side text selection
//! copies, and the reading position.
//!
//! Sized for a million rows: a row is 16 bytes, keys are formatted on demand, and every lookup
//! from a line to its row is arithmetic or a binary search over the rows of one hunk.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use rain_core::comments::Anchor;
use rain_core::diff::{FileDiff, FileStatus, Hunk, LineKind, Side};
use rune::{Key, KeyEvent, Modifier, SelectedText, Tx};
use rune_kit::syntax::{Highlight, Highlighter, Paint};

use crate::model::{self, Block, LineIndex};
use crate::myers;

mod caret;
pub(crate) mod vim;

pub use caret::Caret;
use vim::{Pending, Want};
pub use vim::{Vim, VimKey, Visual};

/// How a diff lays out its two sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DiffMode {
    /// One column, removed lines above the added lines that replace them.
    #[default]
    Unified,
    /// Old on the left, new on the right, changed lines side by side.
    Split,
}

/// A line of one side of a file: what annotations, selections and the cursor point at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineRef {
    pub side: Side,
    /// 1-based.
    pub line: u32,
}

impl LineRef {
    pub fn new(side: Side, line: u32) -> LineRef {
        LineRef { side, line }
    }
}

/// Selected lines of one file, from where the selection started to where it ends (either way
/// round), as a click, shift-click or drag down the line numbers makes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineRange {
    pub file: usize,
    pub start: LineRef,
    pub end: LineRef,
}

impl LineRange {
    /// One line.
    pub fn line(file: usize, at: LineRef) -> LineRange {
        LineRange { file, start: at, end: at }
    }

    /// The range as a comment anchor on the side it started on.
    pub fn anchor(&self) -> Anchor {
        let side = self.start.side;
        let (a, b) = if self.end.side == side {
            (self.start.line.min(self.end.line), self.start.line.max(self.end.line))
        } else {
            (self.start.line, self.start.line)
        };
        if a == b {
            Anchor::Line { side, line: a }
        } else {
            Anchor::Range { side, start: a, end: b }
        }
    }
}

/// Text selected across the code of one side of one file, by line and 0-based char column
/// (`end_col` exclusive), with the text as copying takes it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SpanSelection {
    pub file: usize,
    pub side: Side,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub text: String,
}

impl SpanSelection {
    pub fn anchor(&self) -> Anchor {
        Anchor::Span {
            side: self.side,
            start_line: self.start_line,
            start_col: self.start_col,
            end_line: self.end_line,
            end_col: self.end_col,
        }
    }
}

/// A widget the app shows under the last line of its anchor, such as a review thread or a
/// composer. A range or span anchor also marks its lines in the gutter, and a span tints its
/// text. The app builds the widget from `id` (see [`DiffView::annotation`](super::DiffView::annotation)).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Annotation {
    pub file: usize,
    pub anchor: Anchor,
    pub id: String,
}

impl Annotation {
    pub fn new(file: usize, anchor: Anchor, id: impl Into<String>) -> Annotation {
        Annotation {
            file,
            anchor,
            id: id.into(),
        }
    }

    /// Under one line.
    pub fn line(file: usize, at: LineRef, id: impl Into<String>) -> Annotation {
        Annotation::new(
            file,
            Anchor::Line {
                side: at.side,
                line: at.line,
            },
            id,
        )
    }

    /// The line it sits under.
    pub fn at(&self) -> LineRef {
        LineRef::new(self.anchor.side(), self.anchor.end_line())
    }
}

/// Which way an unchanged range between hunks opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expand {
    /// Twenty lines up from the hunk below it.
    Up,
    /// Twenty lines down from the hunk above it.
    Down,
    /// All of it.
    All,
}

/// Lines one expand step reveals.
pub const EXPAND_STEP: u32 = 20;

/// How one file's lines are laid out as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    /// One column across the whole width: every file in unified mode, and in split mode a file
    /// with one side only (added, deleted, binary, rename-only, contents unavailable) or every
    /// file while the view is too narrow for two sides.
    Single,
    /// Old beside new.
    Split,
}

/// How a drawn file lays out its gutter: both numbers in one column, one side's number only
/// (a one-sided file in split mode), or two halves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Draw {
    Unified,
    One(Side),
    Split,
}

/// A range of lines the reader is pointed at (a tour step, an agent's highlight): the rest of the
/// diff dims and one outline sits around it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Spot {
    pub file: usize,
    pub side: Side,
    /// First and last line, inclusive.
    pub lines: (u32, u32),
    pub caption: Option<String>,
    /// Step `i` of `n`, 1-based.
    pub step: Option<(u32, u32)>,
}

/// Where [`DiffState::scroll_to_range`] puts a range's first line in the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Just below the sticky header.
    Top,
    /// A fifth of the way down: inside the upper third, with the lines before it in view.
    #[default]
    Third,
    Center,
}

/// Row heights in points. Lines are fixed unless they wrap or carry annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub line: f32,
    pub header: f32,
    pub gap: f32,
    pub note: f32,
}

impl Metrics {
    /// Compact: 20 point lines under a 38 point header.
    pub const COMPACT: Metrics = Metrics {
        line: 20.0,
        header: 38.0,
        gap: 28.0,
        note: 40.0,
    };
    /// Comfortable: 22 point lines under a 44 point header.
    pub const COMFORTABLE: Metrics = Metrics {
        line: 22.0,
        header: 44.0,
        gap: 28.0,
        note: 44.0,
    };
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics::COMPACT
    }
}

/// Word changes are skipped for lines longer than this (the cap `myers::words` applies too), and
/// for blocks pairing more lines.
const WORD_MAX_BYTES: usize = 1000;
const WORD_MAX_PAIRS: u32 = 128;

/// One row of the virtual list. `i`, `del` and `add` index [`FileDiff::lines`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// A file's header.
    File(u32),
    /// A file with no lines to show: binary, or only renamed or re-moded.
    Note(u32),
    /// The unchanged range before hunk `gap` (the last one: after the last hunk), with its
    /// expand buttons and the hunk's header.
    Gap { f: u32, gap: u32 },
    /// An unchanged line revealed from a range, by its old and new numbers.
    Ctx { f: u32, old: u32, new: u32 },
    /// One line. In split mode, one without a partner on the other side.
    Line { f: u32, i: u32 },
    /// Split mode: removed line `del` beside added line `add`.
    Pair { f: u32, del: u32, add: u32 },
}

impl Row {
    pub(crate) fn file(self) -> usize {
        match self {
            Row::File(f) | Row::Note(f) | Row::Gap { f, .. } | Row::Ctx { f, .. } | Row::Line { f, .. } | Row::Pair { f, .. } => f as usize,
        }
    }

    /// The smallest line index a hunk row holds: rows of a hunk ascend by it in both modes.
    fn min_line(self) -> u32 {
        match self {
            Row::Line { i, .. } => i,
            Row::Pair { del, .. } => del,
            _ => u32::MAX,
        }
    }

    fn has_line(self) -> bool {
        matches!(self, Row::Ctx { .. } | Row::Line { .. } | Row::Pair { .. })
    }
}

/// What of a file shows beyond its hunks, to carry to another diff over the same file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    /// Every unchanged range open, now or once the whole texts arrive.
    pub whole: bool,
    /// Lines revealed at the top and bottom of each unchanged range.
    pub revealed: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Default)]
struct FileUi {
    collapsed: bool,
    /// Every unchanged range open, now or once the whole texts arrive.
    whole: bool,
    /// Lines revealed at the top and bottom of each unchanged range (`hunks.len() + 1` of them).
    revealed: Vec<(u32, u32)>,
}

/// An unchanged range before a hunk: its first old and new line numbers and its length (`None`
/// when unknown: after the last hunk without the whole text).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Gap {
    pub(crate) old: u32,
    pub(crate) new: u32,
    pub(crate) len: Option<u32>,
}

/// Per file, what the rows are derived from: the line indexes of each side (ascending by line
/// number), change blocks, unchanged ranges and whole-text indexes.
#[derive(Debug, Default)]
struct FileIdx {
    side: [Vec<u32>; 2],
    blocks: Vec<Block>,
    gaps: Vec<Gap>,
    texts: [Option<LineIndex>; 2],
    digits: u32,
    /// Offset of this file's gaps in `gap_rows` and hunks in `hunk_line_rows`.
    gap_base: u32,
    hunk_base: u32,
}

/// Highlights of 128 lines of one side, by line number.
struct Chunk {
    /// Byte offset of each line in the chunk's text, and one past the last.
    starts: Vec<u32>,
    hl: Rc<[Highlight]>,
}

const CHUNK: u32 = 128;
const CHUNK_CAP: usize = 512;

type ChunkKey = (u32, Side, u32);

/// Highlighted chunks, oldest evicted first once [`CHUNK_CAP`] are held.
#[derive(Default)]
struct ChunkCache {
    map: HashMap<ChunkKey, Rc<Chunk>>,
    order: std::collections::VecDeque<ChunkKey>,
}

impl ChunkCache {
    fn get(&self, k: &ChunkKey) -> Option<&Rc<Chunk>> {
        self.map.get(k)
    }

    fn insert(&mut self, k: ChunkKey, c: Rc<Chunk>) {
        if self.map.insert(k, c).is_none() {
            self.order.push_back(k);
        }
        while self.map.len() > CHUNK_CAP {
            let Some(old) = self.order.pop_front() else { break };
            self.map.remove(&old);
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    fn retain(&mut self, keep: impl Fn(&ChunkKey) -> bool) {
        self.map.retain(|k, _| keep(k));
        self.order.retain(|k| keep(k));
    }
}
const NONE: u32 = u32::MAX;

/// Where a row sits, independent of mode and expansion: used to keep the reader's place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    File(usize),
    Line(usize, Side, u32),
}

/// A diff and everything about how it is shown: the app keeps one in its state and
/// [`DiffView`](super::DiffView) draws it.
pub struct DiffState {
    /// Shared so a second view over some of the files (a chapter) doesn't copy their text.
    files: Vec<Arc<FileDiff>>,
    idx: Vec<FileIdx>,
    ui: Vec<FileUi>,
    repo_ids: Vec<String>,
    mode: DiffMode,
    wrap: bool,
    annotations: Vec<Annotation>,
    by_line: HashMap<(u32, Side, u32), Vec<u32>>,
    /// Range and span annotations, by file.
    spans_by_file: HashMap<u32, Vec<u32>>,
    selection: Option<LineRange>,
    selecting: bool,
    selection_at_press: Option<LineRange>,
    /// The press was on the one selected line: it clears the selection unless it drags.
    unselect: bool,
    shift: bool,
    span: Option<SpanSelection>,
    cursor: Option<(usize, LineRef)>,
    /// The cursor's byte in its line, kept on a grapheme's start as the line is read.
    col: usize,
    /// The pointer over a line's code, for a click to put the cursor under it.
    pub(crate) pointer: [f32; 2],
    /// The column `j` and `k` aim for (Vim's curswant).
    want: Want,
    /// A count or the first key of `gg`, `yy`, `f{c}`.
    pending: Pending,
    /// The last `f` `t` (char, forward, till), for `;` and `,`.
    last_find: Option<(char, bool, bool)>,
    visual: Option<Visual>,
    /// Visual mode was started by Shift and a cursor key: a plain cursor key ends it.
    caret_visual: bool,
    /// The scroll viewport's key, as the view last drew it.
    scroll_key: RefCell<String>,
    /// The side text selection copies, which follows the pointer.
    copy_side: Side,
    hovered: Vec<(Side, String)>,
    /// The line under the pointer (its row key, and its side in split), whose `+` and `?` mount.
    hover_line: Option<(String, Option<Side>)>,
    highlighter: Highlighter,
    chunks: RefCell<ChunkCache>,
    /// The widest line of any file, in columns.
    columns: u32,
    pub(crate) rows: Vec<Row>,
    file_rows: Vec<u32>,
    /// Per file and gap, the first row of the unchanged range's rows (`NONE` when collapsed).
    gap_rows: Vec<u32>,
    /// Per file and hunk, the row of its first line.
    hunk_line_rows: Vec<u32>,
    /// Rows taller than a line (headers, notes, gap bars), ascending, each with the extra height
    /// of it and every tall row before it.
    tall_rows: Vec<(u32, f32)>,
    /// The rows hunk navigation stops at, ascending.
    hunk_rows: Vec<u32>,
    /// Bumped whenever the rows change, so built rows rebuild.
    pub(crate) rev: u64,
    /// Changes with the rows' keys, unique across states so a swapped-in state never matches.
    pub(crate) keys_rev: u64,
    /// Files were replaced since the last rebuild: the keys change even if the rows don't.
    new_files: bool,
    visible_rows: Range<usize>,
    view_w: std::cell::Cell<f32>,
    visible: usize,
    /// The visible file last reported to the app.
    reported: Option<usize>,
    nav: Option<u32>,
    /// An expansion asked for before the file's whole texts arrived.
    pending_expand: Option<(usize, usize, Expand)>,
    /// Split draws unified while fewer columns than this fit each side (0: never).
    min_split: u32,
    /// Columns of code one split side has at the view's width, once the view reports it.
    side_cols: Option<u32>,
    spot: Option<Spot>,
    /// Find's query (lowercased when ASCII) and its current hit `(file, line index)`, marked in
    /// the code.
    find: Option<(String, Option<(usize, u32)>)>,
    /// The spot's first and last rows, kept with the rows.
    spot_rows: Option<(u32, u32)>,
    /// Per file and hunk (by `hunk_base`), its stable id.
    hunk_ids: Vec<u64>,
    /// The viewport's vertical offset, height and horizontal offset, as the view last drew them.
    scroll: Rc<std::cell::Cell<[f32; 3]>>,
    /// The spot outline's shift with the scroll, set every scrolled frame without a commit.
    pub(crate) spot_shift: rune::Live<[f32; 2]>,
    m: Metrics,
}

impl std::fmt::Debug for DiffState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffState")
            .field("files", &self.files.len())
            .field("rows", &self.rows.len())
            .field("mode", &self.mode)
            .finish()
    }
}

impl Default for DiffState {
    fn default() -> DiffState {
        DiffState::new(Vec::<Arc<FileDiff>>::new())
    }
}

impl DiffState {
    /// Over owned files, or over `Arc`s shared with another view.
    pub fn new(files: Vec<impl Into<Arc<FileDiff>>>) -> DiffState {
        let mut s = DiffState {
            files: Vec::new(),
            idx: Vec::new(),
            ui: Vec::new(),
            repo_ids: Vec::new(),
            mode: DiffMode::Unified,
            wrap: false,
            annotations: Vec::new(),
            by_line: HashMap::new(),
            spans_by_file: HashMap::new(),
            selection: None,
            selecting: false,
            selection_at_press: None,
            unselect: false,
            shift: false,
            span: None,
            cursor: None,
            col: 0,
            pointer: [0.0; 2],
            want: Want::Here,
            pending: Pending::default(),
            last_find: None,
            visual: None,
            caret_visual: false,
            scroll_key: RefCell::new(String::new()),
            copy_side: Side::New,
            hovered: Vec::new(),
            hover_line: None,
            highlighter: Highlighter::builtin(),
            chunks: RefCell::new(ChunkCache::default()),
            columns: 0,
            rows: Vec::new(),
            file_rows: Vec::new(),
            gap_rows: Vec::new(),
            hunk_line_rows: Vec::new(),
            tall_rows: Vec::new(),
            hunk_rows: Vec::new(),
            rev: 0,
            keys_rev: 0,
            new_files: false,
            visible_rows: 0..0,
            view_w: std::cell::Cell::new(0.0),
            visible: 0,
            reported: None,
            nav: None,
            pending_expand: None,
            min_split: 0,
            side_cols: None,
            spot: None,
            find: None,
            spot_rows: None,
            hunk_ids: Vec::new(),
            scroll: Rc::new(std::cell::Cell::new([0.0; 3])),
            spot_shift: rune::Live::new([0.0; 2]),
            m: Metrics::default(),
        };
        s.set_files(files);
        s
    }

    /// The row heights the view draws and scrolls by.
    pub fn metrics(&self) -> Metrics {
        self.m
    }

    /// Draw rows `m` high (a density change); rows rebuild when it differs.
    pub fn set_metrics(&mut self, m: Metrics) {
        if self.m != m {
            self.m = m;
            self.rebuild();
        }
    }

    /// Show `files` instead, every range collapsed again and the selections, cursor and spotlight
    /// cleared. Annotations stay; drop or remap them with [`DiffState::set_annotations`]. Hunk ids
    /// carry over from the files shown before (see [`DiffState::hunk_id`]).
    pub fn set_files(&mut self, files: Vec<impl Into<Arc<FileDiff>>>) {
        let files: Vec<Arc<FileDiff>> = files.into_iter().map(Into::into).collect();
        self.hunk_ids = reconcile_ids(&self.files, &self.idx, &self.hunk_ids, &files);
        self.spot = None;
        self.ui = files
            .iter()
            .map(|f| FileUi {
                collapsed: false,
                whole: false,
                revealed: vec![(0, 0); f.hunks.len() + 1],
            })
            .collect();
        let (mut gap_base, mut hunk_base) = (0u32, 0u32);
        self.idx = files
            .iter()
            .map(|f| {
                let mut ix = index_file(f);
                ix.gap_base = gap_base;
                ix.hunk_base = hunk_base;
                gap_base += f.hunks.len() as u32 + 1;
                hunk_base += f.hunks.len() as u32;
                ix
            })
            .collect();
        self.columns = files.iter().zip(&self.idx).map(|(f, ix)| text_columns(f, ix)).max().unwrap_or(0);
        self.files = files;
        self.selection = None;
        self.span = None;
        self.cursor = None;
        self.visual = None;
        self.pending = Pending::default();
        self.chunks.borrow_mut().clear();
        self.visible = 0;
        self.reported = None;
        self.visible_rows = 0..0;
        self.pending_expand = None;
        self.index_annotations();
        self.new_files = true;
        self.rebuild();
    }

    pub fn files(&self) -> &[Arc<FileDiff>] {
        &self.files
    }

    /// The files back, for a reload that keeps most of them.
    pub fn into_files(self) -> Vec<Arc<FileDiff>> {
        self.files
    }

    /// Repo ids to show before each file's path (by [`FileDiff::repo`]); empty for none.
    pub fn set_repo_ids(&mut self, ids: Vec<String>) {
        self.repo_ids = ids;
        self.rev += 1;
    }

    /// The path a file's header shows: `<repo>/<path>` when repo ids are set.
    pub fn display_path(&self, file: usize) -> String {
        let f = &self.files[file];
        match self.repo_ids.get(f.repo as usize).filter(|id| !id.is_empty()) {
            Some(id) => format!("{id}/{}", f.path),
            None => f.path.clone(),
        }
    }

    /// The repo id shown before a file's path, when repo ids are set.
    pub fn repo_id(&self, file: usize) -> Option<&str> {
        let f = self.files.get(file)?;
        self.repo_ids.get(f.repo as usize).map(String::as_str).filter(|id| !id.is_empty())
    }

    /// The index of the file at `path` (its path, or old path for a rename).
    pub fn file_index(&self, path: &str) -> Option<usize> {
        self.files
            .iter()
            .position(|f| f.path == path || f.old_path.as_deref() == Some(path))
    }

    pub fn mode(&self) -> DiffMode {
        self.mode
    }

    /// Switch between unified and split. Rows keep their keys where they can (unchanged and
    /// removed lines), so a switch made in a handler that calls `Tx::motion` springs them to
    /// their new places.
    pub fn set_mode(&mut self, mode: DiffMode) {
        if self.mode != mode {
            self.mode = mode;
            self.rebuild();
        }
    }

    /// Split draws every file unified while fewer than `n` columns of code fit each side (0, the
    /// default: never). The view reports the columns as it lays out; see
    /// [`DiffState::layout_reason`].
    pub fn set_min_split_columns(&mut self, n: u32) {
        let was = self.narrow();
        self.min_split = n;
        if self.narrow() != was {
            self.rebuild();
        }
    }

    pub fn min_split_columns(&self) -> u32 {
        self.min_split
    }

    /// The columns of code one split side has at the view's width (the view calls this as it
    /// lays out); whether what the view shows changed.
    pub fn set_columns_per_side(&mut self, cols: u32) -> bool {
        let (was, before) = (self.narrow(), self.side_cols);
        self.side_cols = Some(cols);
        if self.narrow() != was {
            self.rebuild();
            return true;
        }
        was && before != Some(cols)
    }

    /// The columns of code one split side has, once the view has reported them.
    pub fn columns_per_side(&self) -> Option<u32> {
        self.side_cols
    }

    /// Split mode is on but the view is too narrow for two sides, so files draw unified.
    pub fn narrow(&self) -> bool {
        self.mode == DiffMode::Split && self.min_split > 0 && self.side_cols.is_some_and(|c| c < self.min_split)
    }

    /// Why split mode draws unified, for a toolbar's tooltip; `None` when it doesn't.
    pub fn layout_reason(&self) -> Option<String> {
        let cols = self.side_cols.filter(|_| self.narrow())?;
        Some(format!("{cols} columns of code fit each side; split needs {}", self.min_split))
    }

    /// Whether removed and added lines pair side by side in the rows.
    fn pairs(&self) -> bool {
        self.mode == DiffMode::Split && !self.narrow()
    }

    /// How `file` lays out as drawn now.
    pub fn layout(&self, file: usize) -> Layout {
        match self.draw(file) {
            Draw::Split => Layout::Split,
            _ => Layout::Single,
        }
    }

    pub(crate) fn draw(&self, file: usize) -> Draw {
        if !self.pairs() {
            return Draw::Unified;
        }
        match self.files.get(file).and_then(|f| one_sided(f)) {
            Some(side) => Draw::One(side),
            None => Draw::Split,
        }
    }

    pub fn wrap(&self) -> bool {
        self.wrap
    }

    /// Whether long lines wrap as drawn: split always wraps, each side half the width.
    pub(crate) fn wraps(&self) -> bool {
        self.wrap || self.mode == DiffMode::Split
    }

    /// Wrap long lines (rows grow), or keep them on one line and scroll sideways (the default).
    /// Split wraps either way, as side-by-side review does, so both sides stay in view.
    pub fn set_wrap(&mut self, wrap: bool) {
        if self.wrap != wrap {
            self.wrap = wrap;
            self.rev += 1;
        }
    }

    /// Highlight code with `h` instead of the built-in scanners.
    pub fn set_highlighter(&mut self, h: Highlighter) {
        self.highlighter = h;
        self.chunks.borrow_mut().clear();
        self.rev += 1;
    }

    pub fn collapsed(&self, file: usize) -> bool {
        self.ui.get(file).is_some_and(|u| u.collapsed)
    }

    /// Show only `file`'s header, or its lines again.
    pub fn set_collapsed(&mut self, file: usize, collapsed: bool) {
        if let Some(u) = self.ui.get_mut(file).filter(|u| u.collapsed != collapsed) {
            u.collapsed = collapsed;
            self.rebuild();
        }
    }

    /// Collapse (or open) every file `pick` chooses, with one rebuild: e.g. generated files on
    /// load, so a huge snapshot costs no rows until it is opened.
    pub fn set_collapsed_where(&mut self, collapsed: bool, pick: impl Fn(&FileDiff) -> bool) {
        let mut changed = false;
        for (f, u) in self.files.iter().zip(&mut self.ui) {
            if u.collapsed != collapsed && pick(f) {
                u.collapsed = collapsed;
                changed = true;
            }
        }
        if changed {
            self.rebuild();
        }
    }

    /// Give `file` its whole old and new texts (as fetched for expanding context), or drop them
    /// with `None`. Expanded ranges fold again, except on a whole file, which opens in full.
    pub fn set_texts(&mut self, file: usize, old: Option<Arc<str>>, new: Option<Arc<str>>) {
        let Some(f) = self.files.get_mut(file) else { return };
        let f = Arc::make_mut(f);
        f.old_text = old;
        f.new_text = new;
        self.texts_changed(file);
    }

    /// [`DiffState::set_texts`] by handing over another diff's copy of the same file that already
    /// has them, so diffs over the same files share one copy instead of each cloning it.
    pub fn share_file(&mut self, file: usize, f: Arc<FileDiff>) {
        let Some(slot) = self.files.get_mut(file) else { return };
        *slot = f;
        self.texts_changed(file);
    }

    fn texts_changed(&mut self, file: usize) {
        let f = &self.files[file];
        let (gap_base, hunk_base) = (self.idx[file].gap_base, self.idx[file].hunk_base);
        let mut ix = index_file(f);
        ix.gap_base = gap_base;
        ix.hunk_base = hunk_base;
        self.columns = self.columns.max(text_columns(f, &ix));
        self.idx[file] = ix;
        self.ui[file].revealed.iter_mut().for_each(|r| *r = (0, 0));
        if self.ui[file].whole {
            self.reveal_whole(file);
        }
        self.chunks.borrow_mut().retain(|k| k.0 != file as u32);
        self.rebuild();
        if let Some((f, gap, how)) = self.pending_expand.take_if(|p| p.0 == file) {
            self.expand(f, gap, how);
        }
    }

    /// [`DiffState::expand`] now, or as soon as [`DiffState::set_texts`] gives `file` its texts.
    pub fn request_expand(&mut self, file: usize, gap: usize, how: Expand) {
        if self.expandable(file) {
            self.expand(file, gap, how);
        } else {
            self.pending_expand = Some((file, gap, how));
        }
    }

    /// Fold every unchanged range of `file` that [`DiffState::expand`] opened; a whole file
    /// stops being whole.
    pub fn collapse_expanded(&mut self, file: usize) {
        let Some(u) = self.ui.get_mut(file) else { return };
        u.whole = false;
        if u.revealed.iter().any(|&r| r != (0, 0)) {
            u.revealed.iter_mut().for_each(|r| *r = (0, 0));
            self.rebuild();
        }
    }

    /// Whether `file` shows as a whole file: every unchanged range open, or to open once its
    /// texts arrive.
    pub fn whole(&self, file: usize) -> bool {
        self.ui.get(file).is_some_and(|u| u.whole)
    }

    /// Show all of `file` with its changes inline, or fold it back to its hunks. Without whole
    /// texts the file is marked and opens when [`DiffState::set_texts`] gives them.
    pub fn set_whole(&mut self, file: usize, on: bool) {
        if file >= self.ui.len() {
            return;
        }
        if !on {
            self.collapse_expanded(file);
            return;
        }
        self.ui[file].whole = true;
        if self.reveal_whole(file) {
            self.rebuild();
        }
    }

    /// What of `file` shows beyond its hunks.
    pub fn opened(&self, file: usize) -> Option<Opened> {
        self.ui.get(file).map(|u| Opened {
            whole: u.whole,
            revealed: u.revealed.clone(),
        })
    }

    /// Restores what [`DiffState::opened`] read (from another diff over the same files), with
    /// one rebuild. Revealed lines apply only where the file has its texts.
    pub fn set_opened(&mut self, opened: impl IntoIterator<Item = (usize, Opened)>) {
        let mut changed = false;
        for (file, o) in opened {
            let Some(u) = self.ui.get_mut(file) else { continue };
            if u.whole != o.whole {
                u.whole = o.whole;
                changed = true;
            }
            let expandable = self.files[file].old_text.is_some() || self.files[file].new_text.is_some();
            let u = &mut self.ui[file];
            if expandable && o.revealed.len() == u.revealed.len() && u.revealed != o.revealed {
                u.revealed = o.revealed;
                changed = true;
            }
            if o.whole {
                changed |= self.reveal_whole(file);
            }
        }
        if changed {
            self.rebuild();
        }
    }

    /// Show every file `pick` chooses whole (or fold it back), with one rebuild: expand-all.
    pub fn set_whole_where(&mut self, on: bool, pick: impl Fn(&FileDiff) -> bool) {
        let mut changed = false;
        for f in 0..self.files.len() {
            if !pick(&self.files[f]) {
                continue;
            }
            self.ui[f].whole = on;
            changed |= if on { self.reveal_whole(f) } else { self.fold(f) };
        }
        if changed {
            self.rebuild();
        }
    }

    /// Whether `file` waits for its whole texts: a whole-file view or an expansion was asked for
    /// before [`DiffState::set_texts`] gave them.
    pub fn needs_texts(&self, file: usize) -> bool {
        !self.expandable(file) && (self.whole(file) || self.pending_expand.is_some_and(|p| p.0 == file))
    }

    /// Opens every unchanged range of `file` whose length is known; whether anything opened.
    fn reveal_whole(&mut self, file: usize) -> bool {
        if !self.expandable(file) {
            return false;
        }
        let mut changed = false;
        for (r, g) in self.ui[file].revealed.iter_mut().zip(&self.idx[file].gaps) {
            if let Some(len) = g.len
                && r.0 + r.1 < len
            {
                *r = (len, 0);
                changed = true;
            }
        }
        changed
    }

    /// Folds every opened range of `file` without rebuilding; whether any was open.
    fn fold(&mut self, file: usize) -> bool {
        let u = &mut self.ui[file];
        let open = u.revealed.iter().any(|&r| r != (0, 0));
        u.revealed.iter_mut().for_each(|r| *r = (0, 0));
        open
    }

    /// Whether `file`'s unchanged ranges can be shown: it has a whole text.
    pub fn expandable(&self, file: usize) -> bool {
        self.files.get(file).is_some_and(|f| f.old_text.is_some() || f.new_text.is_some())
    }

    /// Lines of `file`'s unchanged range before hunk `gap` still hidden, when known.
    pub fn hidden(&self, file: usize, gap: usize) -> Option<u32> {
        let g = self.idx.get(file)?.gaps.get(gap)?;
        let (top, bottom) = self.ui[file].revealed[gap];
        g.len.map(|n| n.saturating_sub(top + bottom))
    }

    /// Reveal more of `file`'s unchanged range before hunk `gap` (`hunks.len()`: after the
    /// last). Does nothing without the file's whole text.
    pub fn expand(&mut self, file: usize, gap: usize, how: Expand) {
        let Some(hidden) = self.hidden(file, gap) else { return };
        if hidden == 0 || !self.expandable(file) {
            return;
        }
        let last = gap == self.files[file].hunks.len();
        let r = &mut self.ui[file].revealed[gap];
        let step = hidden.min(EXPAND_STEP);
        match how {
            Expand::All => r.0 += hidden,
            Expand::Up if last => r.0 += step,
            Expand::Up => r.1 += step,
            Expand::Down => r.0 += step,
        }
        self.rebuild();
    }

    /// The unchanged range of `file` holding line `at` (its gap index) and the line's offset in it;
    /// `None` when a hunk holds the line.
    fn gap_of(&self, file: usize, at: LineRef) -> Option<(usize, u32)> {
        let f = self.files.get(file)?;
        let h = f.hunks.partition_point(|hk| model::end(hk, at.side) <= at.line);
        if f.hunks.get(h).is_some_and(|hk| model::first(hk, at.side) <= at.line) {
            return None;
        }
        let g = self.idx.get(file)?.gaps.get(h)?;
        let off = at.line.checked_sub(if at.side == Side::Old { g.old } else { g.new })?;
        g.len.is_none_or(|len| off < len).then_some((h, off))
    }

    /// Whether `file` shows line `at`, in a hunk or an unchanged range that is open; a collapsed
    /// file counts as showing its lines.
    pub fn shows_line(&self, file: usize, at: LineRef) -> bool {
        if at.line == 0 || file >= self.files.len() {
            return false;
        }
        if self.line_index(file, at.side, at.line).is_some() {
            return true;
        }
        let Some((gap, off)) = self.gap_of(file, at) else { return false };
        let Some(len) = self.idx[file].gaps[gap].len else { return false };
        let (top, bottom) = self.ui[file].revealed[gap];
        off < top || off >= len - bottom.min(len)
    }

    /// Opens the unchanged range holding `file`'s line `at`, now or once its texts arrive; whether
    /// the line sits in one.
    pub fn reveal_line(&mut self, file: usize, at: LineRef) -> bool {
        let Some((gap, _)) = self.gap_of(file, at).filter(|_| at.line > 0) else {
            return false;
        };
        if self.ui[file].collapsed {
            self.set_collapsed(file, false);
        }
        self.request_expand(file, gap, Expand::All);
        true
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Show these annotations under their lines, in order where several share a line.
    pub fn set_annotations(&mut self, annotations: Vec<Annotation>) {
        self.annotations = annotations;
        self.index_annotations();
        self.rev += 1;
    }

    /// The annotations under `file`'s line `at`.
    pub(crate) fn annotations_at(&self, file: usize, at: LineRef) -> impl Iterator<Item = &Annotation> {
        self.by_line
            .get(&(file as u32, at.side, at.line))
            .into_iter()
            .flatten()
            .map(|&i| &self.annotations[i as usize])
    }

    /// Range and span annotations covering `file`'s line `at`.
    pub(crate) fn covering(&self, file: usize, at: LineRef) -> impl Iterator<Item = &Annotation> {
        self.spans_by_file
            .get(&(file as u32))
            .into_iter()
            .flatten()
            .map(|&i| &self.annotations[i as usize])
            .filter(move |a| {
                let (s, e) = a.anchor.lines();
                a.anchor.side() == at.side && s <= at.line && at.line <= e
            })
    }

    /// Byte ranges of line `at`'s text that span annotations select.
    pub(crate) fn span_marks(&self, file: usize, at: LineRef) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for a in self.covering(file, at) {
            let Anchor::Span {
                start_line,
                start_col,
                end_line,
                end_col,
                ..
            } = a.anchor
            else {
                continue;
            };
            let Some(text) = self.side_line(file, at.side, at.line) else {
                continue;
            };
            let from = if at.line == start_line { byte_of_col(text, start_col) } else { 0 };
            let to = if at.line == end_line {
                byte_of_col(text, end_col)
            } else {
                text.len()
            };
            if from < to {
                out.push(from..to);
            }
        }
        out
    }

    fn index_annotations(&mut self) {
        self.by_line.clear();
        self.spans_by_file.clear();
        for (i, a) in self.annotations.iter().enumerate() {
            let at = a.at();
            self.by_line.entry((a.file as u32, at.side, at.line)).or_default().push(i as u32);
            if !matches!(a.anchor, Anchor::Line { .. }) {
                self.spans_by_file.entry(a.file as u32).or_default().push(i as u32);
            }
        }
    }

    pub fn selection(&self) -> Option<LineRange> {
        self.selection
    }

    pub fn set_selection(&mut self, sel: Option<LineRange>) {
        self.selection = sel;
    }

    /// The text selected across code, once a drag over it ends.
    pub fn span_selection(&self) -> Option<&SpanSelection> {
        self.span.as_ref()
    }

    pub fn set_span_selection(&mut self, span: Option<SpanSelection>) {
        if span.is_some() {
            self.visual = None;
        }
        self.span = span;
    }

    /// The selected lines' text on the side the selection started on, one per line, without
    /// numbers or markers: what copying the selection gives.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let side = sel.start.side;
        let (a, b) = self.selected_rows(&sel)?;
        let mut out = Vec::new();
        for r in a..=b {
            if let Some(n) = self.row_line(self.rows[r as usize], side) {
                out.push(self.side_line(sel.file, side, n).unwrap_or_default());
            }
        }
        Some(out.join("\n"))
    }

    /// The selection's first and last rows.
    fn selected_rows(&self, sel: &LineRange) -> Option<(u32, u32)> {
        let a = self.row_of(sel.file, sel.start.side, sel.start.line)?;
        let b = self.row_of(sel.file, sel.end.side, sel.end.line)?;
        Some((a.min(b), a.max(b)))
    }

    /// Whether line `at` is in `sel`.
    pub(crate) fn contains(&self, sel: &LineRange, at: LineRef) -> bool {
        let Some((a, b)) = self.selected_rows(sel) else { return false };
        self.row_of(sel.file, at.side, at.line).is_some_and(|r| (a..=b).contains(&r))
    }

    /// Whether row `r`'s line on `side` is selected.
    pub(crate) fn row_selected(&self, r: usize, side: Option<Side>) -> bool {
        let Some(sel) = self.selection else { return false };
        if self.rows[r].file() != sel.file {
            return false;
        }
        let Some((a, b)) = self.selected_rows(&sel) else { return false };
        if !(a as usize..=b as usize).contains(&r) {
            return false;
        }
        side.is_none_or(|s| s == sel.start.side || s == sel.end.side)
    }

    /// A press on line `at`'s number: a new selection, or with Shift held one extended to it.
    /// Pressing the one selected line clears it on release, unless the press drags.
    pub(crate) fn press_line(&mut self, file: usize, at: LineRef) {
        self.visual = None;
        self.selection_at_press = self.selection;
        let sel = match self.selection {
            Some(s) if self.shift && s.file == file => LineRange { end: at, ..s },
            _ => LineRange::line(file, at),
        };
        self.unselect = !self.shift && self.selection == Some(sel);
        self.selecting = true;
        self.set_selection(Some(sel));
    }

    /// The pointer, dragging from a line number, reached line `at`.
    pub(crate) fn drag_to(&mut self, file: usize, at: LineRef) {
        if let Some(s) = self.selection.filter(|s| self.selecting && s.file == file && s.end != at) {
            self.unselect = false;
            self.set_selection(Some(LineRange { end: at, ..s }));
        }
    }

    /// The press on the line numbers ended: the selection it leaves.
    pub(crate) fn release_line(&mut self) -> Option<LineRange> {
        self.selection_at_press = None;
        if std::mem::take(&mut self.unselect) {
            self.selection = None;
        }
        self.selecting = false;
        self.selection
    }

    pub(crate) fn cancel_line(&mut self) {
        if self.selecting {
            self.selection = self.selection_at_press.take();
        }
        self.selecting = false;
        self.unselect = false;
    }

    pub(crate) fn copy_side(&self) -> Side {
        self.copy_side
    }

    /// Notes the pointer entering or leaving a line's code; returns whether that changed the
    /// side a selection copies, the only part the view reads. A held press keeps the cell it
    /// started in entered, so dragging across to the other side doesn't switch.
    pub(crate) fn hover_text(&mut self, side: Side, cell: &str, inside: bool) -> bool {
        let was = self.copy_side;
        if inside {
            if !self.hovered.iter().any(|(_, c)| c == cell) {
                self.hovered.push((side, cell.to_owned()));
            }
            if self.hovered.iter().all(|(s, _)| *s == side) {
                self.copy_side = side;
            }
        } else {
            self.hovered.retain(|(_, c)| c != cell);
        }
        self.copy_side != was
    }

    /// The pointer entered a line (a split half): true when that moved the mounted `+` and `?`.
    /// Leaving keeps them (hidden), and a text selection holds them: a rebuild then can garble its copy.
    pub(crate) fn hover_line(&mut self, key: &str, side: Option<Side>, inside: bool) -> bool {
        let here = self.hover_line.as_ref().is_some_and(|(k, s)| k == key && *s == side);
        if !inside || here || self.span.is_some() {
            return false;
        }
        self.hover_line = Some((key.to_owned(), side));
        true
    }

    /// The hovered line's side when the row keyed `key` is hovered (`Some(None)` in unified).
    pub(crate) fn hovered_line(&self, key: &str) -> Option<Option<Side>> {
        self.hover_line.as_ref().filter(|(k, _)| k == key).map(|(_, s)| *s)
    }

    /// The span a text selection over the code covers: from its first selected code text to its
    /// last, both on one side of one file. Texts outside the code (annotation widgets) are
    /// ignored.
    pub fn span_from_texts(&self, texts: &[SelectedText], text: &str) -> Option<SpanSelection> {
        let mut code = texts
            .iter()
            .filter(|t| t.is_selected() && t.path().ends_with("/code/text") && !t.path().contains("/notes/"));
        let first = code.next()?;
        let last = code.next_back().unwrap_or(first);
        let (f, side, a) = self.code_line(first.path())?;
        let (f2, side2, b) = self.code_line(last.path())?;
        if f != f2 || side != side2 {
            return None;
        }
        let wrap = self.wraps();
        let col = |n: u32, byte: usize| self.side_line(f, side, n).map_or(0, |t| shown_to_col(t, wrap, byte));
        let (mut end_line, mut end_col) = (b, col(b, last.range().end));
        // A drag that stops at the very start of a line selected nothing on it.
        if end_col == 0
            && b > a
            && let Some(prev) = self.side_line(f, side, b - 1)
        {
            (end_line, end_col) = (b - 1, prev.chars().count() as u32);
        }
        Some(SpanSelection {
            file: f,
            side,
            start_line: a,
            start_col: col(a, first.range().start),
            end_line,
            end_col,
            text: text.to_owned(),
        })
    }

    /// The file, side and line number of the code text at element `path`.
    fn code_line(&self, path: &str) -> Option<(usize, Side, u32)> {
        let segs: Vec<&str> = path.split('/').collect();
        let (j, (f, kind, num)) = segs.iter().enumerate().find_map(|(j, s)| parse_key(s).map(|k| (j, k)))?;
        let side = if segs[j + 1..].contains(&"old") {
            Side::Old
        } else if segs[j + 1..].contains(&"new") {
            Side::New
        } else {
            match kind {
                b'o' => Side::Old,
                b'n' => Side::New,
                _ => self.copy_side,
            }
        };
        let key_side = if kind == b'n' { Side::New } else { Side::Old };
        let r = self.row_of(f, key_side, num)?;
        Some((f, side, self.row_line(self.rows[r as usize], side)?))
    }

    pub fn visible_file(&self) -> usize {
        self.visible
    }

    /// The line cursor: a file and one of its lines.
    pub fn cursor(&self) -> Option<(usize, LineRef)> {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: Option<(usize, LineRef)>) {
        let prev = std::mem::replace(&mut self.cursor, cursor);
        self.jumped(prev);
    }

    /// The row the cursor is on, when it is shown.
    pub(crate) fn cursor_row(&self) -> Option<u32> {
        let (f, at) = self.cursor?;
        self.row_of(f, at.side, at.line)
    }

    /// Move the cursor to the next (or previous) line, at the column it aims for, scrolling only
    /// as far as keeps it off the viewport's edges.
    pub fn cursor_step(&mut self, forward: bool, tx: &mut Tx) -> bool {
        self.cursor_steps(forward, 1, tx)
    }

    /// [`DiffState::cursor_step`] `n` lines at once.
    pub fn cursor_steps(&mut self, forward: bool, n: u32, tx: &mut Tx) -> bool {
        let from = self
            .cursor_row()
            .map_or_else(|| self.position() as i64 - i64::from(forward), i64::from);
        let Some(r) = self.step_rows(from, forward, n) else { return false };
        self.put_cursor(r, tx);
        true
    }

    /// The row `n` line rows on from row `from` (fewer at the end of the rows); `None` when
    /// there is none at all.
    fn step_rows(&self, from: i64, forward: bool, n: u32) -> Option<u32> {
        let (mut r, mut found) = (from, None);
        for _ in 0..n.max(1) {
            loop {
                r += if forward { 1 } else { -1 };
                let Some(&row) = usize::try_from(r).ok().and_then(|i| self.rows.get(i)) else {
                    return found;
                };
                if row.has_line() {
                    break;
                }
            }
            found = Some(r as u32);
        }
        found
    }

    pub fn cursor_down(&mut self, tx: &mut Tx) -> bool {
        self.cursor_step(true, tx)
    }

    pub fn cursor_up(&mut self, tx: &mut Tx) -> bool {
        self.cursor_step(false, tx)
    }

    /// Put the cursor on row `r`'s line, preferring the cursor's side, at the column it aims for.
    fn put_cursor(&mut self, r: u32, tx: &mut Tx) {
        let prefer = self.cursor.map_or(Side::New, |(_, at)| at.side);
        self.aim();
        let side = self.side_for(self.rows[r as usize], prefer);
        self.land_at_want(r, side, tx);
    }

    /// The side the cursor takes on `row`: the side a one-sided line is on, else `prefer`.
    pub(crate) fn side_for(&self, row: Row, prefer: Side) -> Side {
        match row {
            Row::Line { f, i } => match self.files[f as usize].lines[i as usize].kind {
                LineKind::Removed => Side::Old,
                LineKind::Added => Side::New,
                LineKind::Context => prefer,
            },
            _ => prefer,
        }
    }

    /// Row `r`'s text on `side`, when it shows that side.
    pub(crate) fn row_text(&self, r: u32, side: Side) -> Option<&str> {
        let row = *self.rows.get(r as usize)?;
        let file = &self.files[row.file()];
        let i = match row {
            Row::Line { i, .. } => i,
            Row::Pair { del, add, .. } => {
                if side == Side::Old {
                    del
                } else {
                    add
                }
            }
            Row::Ctx { old, new, .. } => {
                let n = if side == Side::Old { old } else { new };
                return Some(self.side_line(row.file(), side, n).unwrap_or(""));
            }
            _ => return None,
        };
        model::number(file, i, side)?;
        Some(file.line_text(i))
    }

    /// Row `r` is on screen below the sticky header.
    fn in_view(&self, r: usize) -> bool {
        let top = self.row_below_header().map_or(self.visible_rows.start, |t| t as usize);
        top <= r && r + 1 < self.visible_rows.end
    }

    /// The lines a comment at the cursor covers: the line selection when the cursor is in it,
    /// else the cursor's line.
    pub fn comment_at_cursor(&self) -> Option<LineRange> {
        let (f, at) = self.cursor?;
        match self.selection {
            Some(sel) if sel.file == f && self.contains(&sel, at) => Some(sel),
            _ => Some(LineRange::line(f, at)),
        }
    }

    /// Scroll `file`'s header to the top.
    pub fn scroll_to_file(&mut self, file: usize, tx: &mut Tx) {
        let Some(&r) = self.file_rows.get(file) else { return };
        self.reveal_at_top(r, tx);
        self.visible = file;
        self.nav = Some(r);
    }

    /// Scroll to `file`'s line `at`, opening the range it's in if it's hidden, and put it
    /// below the header. The cursor moves there.
    pub fn scroll_to_line(&mut self, file: usize, at: LineRef, tx: &mut Tx) {
        if file >= self.files.len() {
            return;
        }
        self.open_line(file, at);
        let Some(r) = self.row_of(file, at.side, at.line) else { return };
        self.reveal_below_header(r, tx);
        self.visible = file;
        self.nav = Some(r);
        let prev = self.cursor.replace((file, at));
        self.jumped(prev);
    }

    /// Opens `file` and the unchanged range holding line `at`, so the line has a row.
    fn open_line(&mut self, file: usize, at: LineRef) {
        if self.collapsed(file) {
            self.set_collapsed(file, false);
        }
        if self.row_of(file, at.side, at.line).is_none() {
            let gap = self.idx[file].gaps.iter().position(|g| {
                let first = if at.side == Side::Old { g.old } else { g.new };
                at.line >= first && g.len.is_none_or(|n| at.line < first + n)
            });
            if let Some(gap) = gap {
                self.expand(file, gap, Expand::All);
            }
        }
    }

    /// Point the reader at a range: rows outside it dim and one outline sits around it, sliding
    /// to the next range when the spot changes. `None` clears it. Paint only: no row changes.
    pub fn set_spotlight(&mut self, spot: Option<Spot>) {
        self.spot = spot.filter(|s| s.file < self.files.len());
        self.spot_rows = self.find_spot_rows();
    }

    pub fn spotlight(&self) -> Option<&Spot> {
        self.spot.as_ref()
    }

    /// Mark `query`'s matches in the code (ASCII matches ignore case), the one on line
    /// `current` (file, index into its lines) brighter; an empty query clears them.
    pub fn set_find(&mut self, query: &str, current: Option<(usize, u32)>) {
        let next = (!query.is_empty()).then(|| {
            let q = if query.is_ascii() {
                query.to_ascii_lowercase()
            } else {
                query.to_owned()
            };
            (q, current)
        });
        if next != self.find {
            self.find = next;
            self.rev += 1;
        }
    }

    /// Byte ranges of find's matches in `text`, and whether line `at` holds the current hit.
    pub(crate) fn find_marks(&self, text: &str, at: Option<(usize, u32)>) -> (Vec<Range<usize>>, bool) {
        let Some((q, current)) = &self.find else {
            return (Vec::new(), false);
        };
        let hay = if q.is_ascii() { text.to_ascii_lowercase() } else { text.to_owned() };
        let marks = hay.match_indices(q.as_str()).map(|(i, m)| i..i + m.len()).collect();
        (marks, at.is_some() && *current == at)
    }

    /// The spot's first and last rows, when its lines have rows.
    pub fn spotlight_rows(&self) -> Option<(u32, u32)> {
        self.spot_rows
    }

    /// Whether row `row` draws dimmed: a spot is set and the row is outside it (the spot file's
    /// header stays lit).
    pub fn dimmed(&self, row: usize) -> bool {
        let Some(spot) = &self.spot else { return false };
        if self.rows.get(row) == Some(&Row::File(spot.file as u32)) {
            return false;
        }
        self.spot_rows.is_none_or(|(a, b)| row < a as usize || row > b as usize)
    }

    fn find_spot_rows(&self) -> Option<(u32, u32)> {
        let s = self.spot.as_ref()?;
        let (lo, hi) = (s.lines.0.min(s.lines.1), s.lines.0.max(s.lines.1));
        // Lines without a row (in a folded range) are skipped; at most a screenful is searched.
        let first = (lo..=hi.min(lo.saturating_add(4096))).find_map(|n| self.row_of(s.file, s.side, n))?;
        let last = (lo.max(hi.saturating_sub(4096))..=hi)
            .rev()
            .find_map(|n| self.row_of(s.file, s.side, n))?;
        Some((first, last.max(first)))
    }

    /// Scroll so `file`'s lines `lines` (on `side`) sit where `align` says, opening what hides
    /// them. The cursor moves to the first line.
    pub fn scroll_to_range(&mut self, file: usize, side: Side, lines: (u32, u32), align: Align, tx: &mut Tx) {
        if file >= self.files.len() {
            return;
        }
        let (lo, hi) = (lines.0.min(lines.1), lines.0.max(lines.1));
        self.open_line(file, LineRef::new(side, lo));
        self.open_line(file, LineRef::new(side, hi));
        let Some((n, r)) = (lo..=hi).find_map(|n| self.row_of(file, side, n).map(|r| (n, r))) else {
            return;
        };
        let h = match self.scroll.get()[1] {
            h if h > 0.0 => h,
            _ => self.visible_rows.len().max(30) as f32 * self.m.line,
        };
        let above = match align {
            Align::Top => self.m.header,
            Align::Third => (h / 5.0).max(self.m.header),
            Align::Center => h / 2.0,
        };
        let target = self.row_y(r as usize) - above;
        let mut top = (0..=r as usize).rev().find(|&t| self.row_y(t) <= target).unwrap_or(0);
        // Snap to the nearer row start: a tall row (a collapsed file's header, a gap) would
        // otherwise leave the range that much lower than asked.
        let next = top + 1;
        if next <= r as usize
            && self.row_y(next) - target < target - self.row_y(top)
            && self.row_y(r as usize) - self.row_y(next) >= self.m.header
        {
            top = next;
        }
        self.reveal_at_top(top as u32, tx);
        self.visible = file;
        self.nav = Some(r);
        let prev = self.cursor.replace((file, LineRef::new(side, n)));
        self.jumped(prev);
    }

    /// The viewport's scroll offset in points, as last drawn: for keeping the reading place.
    pub fn scroll_offset(&self) -> f32 {
        self.scroll.get()[0]
    }

    /// How far the code is scrolled sideways in points, as last drawn.
    pub fn scroll_offset_x(&self) -> f32 {
        self.scroll.get()[2]
    }

    /// The viewport's height in points, as last drawn (0 before the first frame).
    pub fn viewport_height(&self) -> f32 {
        self.scroll.get()[1]
    }

    /// The view records where it is scrolled here every frame it moves (no commit).
    pub(crate) fn scroll_cell(&self) -> Rc<std::cell::Cell<[f32; 3]>> {
        self.scroll.clone()
    }

    /// The hunk at the reading position: the file and hunk index of the row under the sticky
    /// header, a hunk counting from the unchanged lines before it (the file's first hunk while
    /// its header is there).
    pub fn visible_hunk(&self) -> Option<(usize, usize)> {
        let r = self.position();
        let f = self.rows.get(r as usize)?.file();
        let n = self.files[f].hunks.len();
        let base = self.idx[f].gap_base as usize;
        let starts = self.gap_rows.get(base..base + n)?;
        if n == 0 || starts.first() == Some(&NONE) {
            return None;
        }
        Some((f, starts.partition_point(|&s| s <= r).saturating_sub(1)))
    }

    /// A stable id for `file`'s hunk `hunk`: a hash of its path and first and last changed lines,
    /// carried across [`DiffState::set_files`] by old-side range (one hunk it overlaps: that
    /// one's id; several: the biggest's; none: its own hash). 16 hex digits.
    pub fn hunk_id(&self, file: usize, hunk: usize) -> Option<String> {
        let ix = self.idx.get(file)?;
        (hunk < self.files[file].hunks.len()).then(|| format!("{:016x}", self.hunk_ids[ix.hunk_base as usize + hunk]))
    }

    /// The hunk index of `file` whose id is `id`.
    pub fn hunk_by_id(&self, file: usize, id: &str) -> Option<usize> {
        let ix = self.idx.get(file)?;
        let id = u64::from_str_radix(id, 16).ok()?;
        let base = ix.hunk_base as usize;
        self.hunk_ids[base..base + self.files[file].hunks.len()]
            .iter()
            .position(|&h| h == id)
    }

    /// A hash of everything `file`'s hunk `hunk` shows (each line's kind and text): equal while
    /// the hunk is unchanged, for keeping it viewed. 16 hex digits.
    pub fn hunk_hash(&self, file: usize, hunk: usize) -> Option<String> {
        let f = self.files.get(file)?;
        let h = f.hunks.get(hunk)?;
        let mut hasher = blake3::Hasher::new();
        for i in h.lines.clone() {
            hasher.update(&[f.lines[i as usize].kind as u8]);
            hasher.update(f.line_text(i).as_bytes());
            hasher.update(b"\n");
        }
        Some(format!("{:016x}", first_u64(hasher.finalize())))
    }

    /// Bring the next (or previous) hunk to just below the sticky header, with the cursor on
    /// its first line.
    pub fn next_hunk(&mut self, forward: bool, tx: &mut Tx) -> bool {
        let cursor = self.cursor_row();
        let from = cursor.unwrap_or_else(|| self.position());
        let target = if forward {
            self.hunk_rows.get(self.hunk_rows.partition_point(|&r| r <= from)).copied()
        } else {
            // With a cursor the hunk it is in counts as current, so "previous" skips it.
            let back = if cursor.is_some() { 2 } else { 1 };
            let before = if cursor.is_some() {
                self.hunk_rows.partition_point(|&r| r <= from)
            } else {
                self.hunk_rows.partition_point(|&r| r < from)
            };
            before.checked_sub(back).map(|i| self.hunk_rows[i])
        };
        let Some(r) = target else { return false };
        self.reveal_below_header(r, tx);
        self.visible = self.rows[r as usize].file();
        self.nav = Some(r);
        if let Some(first) =
            (r as usize..self.rows.len().min(r as usize + 4)).find(|&i| matches!(self.rows[i], Row::Line { .. } | Row::Pair { .. }))
        {
            let row = self.rows[first];
            let side = match row {
                Row::Line { f, i } if self.files[f as usize].lines[i as usize].kind == LineKind::Removed => Side::Old,
                Row::Pair { .. } => Side::Old,
                _ => Side::New,
            };
            if let Some(n) = self.row_line(row, side) {
                let prev = self.cursor.replace((row.file(), LineRef::new(side, n)));
                self.jumped(prev);
            }
        }
        true
    }

    /// Scroll to the next (or previous) file's header.
    pub fn next_file(&mut self, forward: bool, tx: &mut Tx) -> bool {
        let at = match self.cursor {
            Some((f, _)) if self.cursor_row().is_some() => f,
            _ => match self.rows.get(self.position() as usize) {
                Some(row) => row.file(),
                None => return false,
            },
        };
        let target = if forward { at + 1 } else { at.wrapping_sub(1) };
        let Some(f) = (target < self.files.len()).then_some(target) else {
            return false;
        };
        self.scroll_to_file(f, tx);
        self.cursor = None;
        self.sync_visual();
        true
    }

    /// The row under the sticky header: where navigation last put it, unless the list has
    /// scrolled away since.
    fn position(&self) -> u32 {
        let top = self.row_below_header();
        match (self.nav, top) {
            (Some(n), Some(t)) if t.abs_diff(n) <= 3 => n,
            (_, Some(t)) => t,
            (Some(n), None) => n,
            (None, None) => 0,
        }
    }

    fn row_below_header(&self) -> Option<u32> {
        if self.visible_rows.is_empty() {
            return None;
        }
        let mut r = self.visible_rows.start;
        if self.sticky().is_some() {
            let mut room = 0.0;
            while r + 1 < self.rows.len() && room < self.m.header {
                room += self.estimate(r);
                r += 1;
            }
        }
        Some(r as u32)
    }

    /// Handle a key for the diff (UX keymap): `j`/`k` move the line cursor, `n`/`p` the next
    /// and previous hunk, `]`/`[` the next and previous file, Escape clears the line selection,
    /// then the text selection, then the cursor; Cmd-C copies the line selection, and Shift
    /// (with `App::modifier_keys`) makes a click on a line number extend the selection. Call it
    /// from the app's `on_key`; it returns whether it took the key.
    pub fn key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        match &ev.key {
            Key::Modifier {
                modifier: Modifier::ShiftLeft | Modifier::ShiftRight,
                ..
            } => {
                self.shift = ev.mods.shift;
                false
            }
            _ if tx.focused().is_some() && !ev.is_command('c') => false,
            Key::Text(t) if !ev.mods.cmd && !ev.mods.ctrl && !ev.mods.alt => match t.as_str() {
                "j" => self.cursor_down(tx),
                "k" => self.cursor_up(tx),
                "n" => self.next_hunk(true, tx),
                "p" => self.next_hunk(false, tx),
                "]" => self.next_file(true, tx),
                "[" => self.next_file(false, tx),
                _ => false,
            },
            Key::Escape if self.selection.is_some() => {
                self.set_selection(None);
                true
            }
            Key::Escape if self.span.is_some() => {
                self.span = None;
                tx.clear_text_selection();
                true
            }
            Key::Escape if self.cursor.is_some() => {
                self.cursor = None;
                self.sync_visual();
                true
            }
            _ if ev.is_command('c') => match self.selected_text() {
                Some(text) => {
                    tx.copy(text);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    /// The scroll viewport's rect.
    pub(crate) fn set_viewport(&self, r: [f32; 4]) {
        self.view_w.set(r[2]);
    }

    pub(crate) fn view_width(&self) -> f32 {
        self.view_w.get()
    }

    /// The virtual list's visible row range changed to `rows`: returns `(reported_file, sticky_changed)`.
    pub(crate) fn set_visible_rows(&mut self, rows: Range<usize>) -> (Option<usize>, bool) {
        let before_sticky = self.sticky();
        self.visible_rows = rows;
        if let Some(&r) = self.rows.get(self.visible_rows.start) {
            self.visible = r.file();
        }
        let reported = (!self.files.is_empty() && self.reported != Some(self.visible)).then(|| {
            self.reported = Some(self.visible);
            self.visible
        });
        (reported, self.sticky() != before_sticky)
    }

    /// The row reached by moving `y` points vertically from the top of row `from`.
    pub(crate) fn row_from(&self, from: u32, mut y: f32) -> Option<u32> {
        let mut r = usize::try_from(from).ok()?.min(self.rows.len().checked_sub(1)?);
        while y < 0.0 && r > 0 {
            r -= 1;
            y += self.estimate(r);
        }
        while r + 1 < self.rows.len() {
            let h = self.estimate(r);
            if y < h {
                break;
            }
            y -= h;
            r += 1;
        }
        Some(r as u32)
    }

    /// The visible file's header has scrolled above the viewport's top, so a copy of it sticks
    /// there.
    pub(crate) fn sticky(&self) -> Option<usize> {
        if self.files.is_empty() || self.visible_rows.is_empty() {
            return None;
        }
        let f = self.visible;
        let header = *self.file_rows.get(f)? as usize;
        (self.visible_rows.start > header).then_some(f)
    }

    /// Scroll so row `r` is at the top: to the end first, so the reveal comes from below.
    fn reveal_at_top(&self, r: u32, tx: &mut Tx) {
        if let Some(&last) = self.rows.last() {
            tx.scroll_into_view(self.key_of(last));
        }
        tx.scroll_into_view(self.key_of(self.rows[r as usize]));
    }

    /// Scroll so row `r` sits just below the sticky header: the row a header's height above it
    /// goes to the top.
    fn reveal_below_header(&self, r: u32, tx: &mut Tx) {
        let mut top = r as usize;
        let mut room = 0.0;
        while top > 0 && room < self.m.header {
            if matches!(self.rows[top], Row::File(_)) {
                break;
            }
            top -= 1;
            room += self.estimate(top);
        }
        self.reveal_at_top(top as u32, tx);
    }

    fn estimate(&self, r: usize) -> f32 {
        match self.rows[r] {
            Row::File(_) => self.m.header,
            Row::Note(_) => self.m.note,
            Row::Gap { .. } => self.m.gap,
            _ => self.m.line,
        }
    }

    /// The index in [`FileDiff::lines`] of line `n` of `side` of file `f`, when a hunk holds it.
    pub(crate) fn line_index(&self, f: usize, side: Side, n: u32) -> Option<u32> {
        let file = &self.files[f];
        let v = &self.idx[f].side[side as usize];
        let p = v.partition_point(|&i| model::number(file, i, side).unwrap_or(0) < n);
        v.get(p).copied().filter(|&i| model::number(file, i, side) == Some(n))
    }

    /// Line `n` of `side` of file `f`, from its hunks, or an unchanged range of its whole text.
    pub fn side_line(&self, f: usize, side: Side, n: u32) -> Option<&str> {
        let file = self.files.get(f)?;
        if let Some(i) = self.line_index(f, side, n) {
            return Some(file.line_text(i));
        }
        let h = file.hunks.partition_point(|h| model::end(h, side) <= n);
        let g = *self.idx[f].gaps.get(h)?;
        let off = n.checked_sub(if side == Side::Old { g.old } else { g.new })?;
        self.context_line(f, g.old.saturating_add(off), g.new.saturating_add(off))
    }

    /// Unchanged line `old` of the old side (with `new` its number on the new side), from
    /// whichever whole text the file has.
    fn context_line(&self, f: usize, old: u32, new: u32) -> Option<&str> {
        let file = &self.files[f];
        let [o, n] = &self.idx[f].texts;
        let from_old = o.as_ref().zip(file.old_text.as_deref()).and_then(|(ix, t)| ix.line(t, old));
        from_old.or_else(|| n.as_ref().zip(file.new_text.as_deref()).and_then(|(ix, t)| ix.line(t, new)))
    }

    /// Word changes of line `i` of `file` against the line it pairs with: byte ranges of its
    /// text. Computed on demand, so loading never pays for them.
    pub fn word_changes(&self, file: usize, i: u32) -> Vec<Range<usize>> {
        let f = &self.files[file];
        let Some(b) = model::block_of(&self.idx[file].blocks, i) else {
            return Vec::new();
        };
        if b.pairs() > WORD_MAX_PAIRS {
            return Vec::new();
        }
        let Some(p) = b.partner(i) else { return Vec::new() };
        let (del, add) = if i < p { (i, p) } else { (p, i) };
        let (a, c) = (f.line_text(del), f.line_text(add));
        if a.len() > WORD_MAX_BYTES || c.len() > WORD_MAX_BYTES {
            return Vec::new();
        }
        let (old, new) = myers::words(a, c);
        if i == del { old } else { new }
    }

    /// Colour-only highlights of line `n` of `side` of file `f`, relative to the line.
    pub(crate) fn highlights(&self, f: usize, side: Side, n: u32) -> Vec<(Range<usize>, Paint)> {
        let lang = model::language(&self.files[f].path);
        if self.highlighter.is_builtin() && !lang.is_some_and(rune_kit::syntax::known) {
            return Vec::new();
        }
        let c = n.saturating_sub(1) / CHUNK;
        let key = (f as u32, side, c);
        let chunk = self.chunks.borrow().get(&key).cloned();
        let chunk = chunk.unwrap_or_else(|| {
            let mut text = String::new();
            let mut starts = Vec::with_capacity(CHUNK as usize + 1);
            for m in c * CHUNK + 1..=(c + 1) * CHUNK {
                starts.push(text.len() as u32);
                text.push_str(self.side_line(f, side, m).unwrap_or(""));
                text.push('\n');
            }
            starts.push(text.len() as u32);
            let hl = self.highlighter.highlight(lang, &text);
            let chunk = Rc::new(Chunk { starts, hl });
            self.chunks.borrow_mut().insert(key, chunk.clone());
            chunk
        });
        let k = (n.saturating_sub(1) % CHUNK) as usize;
        let (a, b) = (chunk.starts[k] as usize, chunk.starts[k + 1] as usize - 1);
        let first = chunk.hl.partition_point(|(r, _)| r.end <= a);
        chunk.hl[first..]
            .iter()
            .take_while(|(r, _)| r.start < b)
            .map(|(r, p)| (r.start.max(a) - a..r.end.min(b) - a, *p))
            .filter(|(r, _)| !r.is_empty())
            .collect()
    }

    /// Row `r`'s line number on `side`.
    pub(crate) fn row_line(&self, r: Row, side: Side) -> Option<u32> {
        match r {
            Row::Ctx { old, new, .. } => Some(if side == Side::Old { old } else { new }),
            Row::Line { f, i } => model::number(&self.files[f as usize], i, side),
            Row::Pair { f, del, add } => model::number(&self.files[f as usize], if side == Side::Old { del } else { add }, side),
            _ => None,
        }
    }

    /// The row showing line `n` of `side` of file `f`, if one does.
    pub(crate) fn row_of(&self, f: usize, side: Side, n: u32) -> Option<u32> {
        let file = self.files.get(f)?;
        if n == 0 || self.ui[f].collapsed {
            return None;
        }
        let ix = &self.idx[f];
        let h = file.hunks.partition_point(|h| model::end(h, side) <= n);
        if let Some(hunk) = file.hunks.get(h).filter(|hk| model::first(hk, side) <= n) {
            let t = self.line_index(f, side, n)?;
            let start = *self.hunk_line_rows.get((ix.hunk_base + h as u32) as usize)?;
            if start == NONE || !hunk.lines.contains(&t) {
                return None;
            }
            return match self.pairs() {
                false => Some(start + (t - hunk.lines.start)),
                true => {
                    let target = match model::block_of(&ix.blocks, t) {
                        Some(b) if t >= b.a0 => b.partner(t).unwrap_or(t),
                        _ => t,
                    };
                    let end = self.gap_rows[(ix.gap_base + h as u32 + 1) as usize];
                    let rows = &self.rows[start as usize..end as usize];
                    let p = rows.partition_point(|r| r.min_line() < target);
                    (rows.get(p).map(|r| r.min_line()) == Some(target)).then_some(start + p as u32)
                }
            };
        }
        let g = *ix.gaps.get(h)?;
        let len = g.len?;
        let off = n.checked_sub(if side == Side::Old { g.old } else { g.new })?;
        if off >= len {
            return None;
        }
        let r0 = self.gap_rows[(ix.gap_base + h as u32) as usize];
        if r0 == NONE {
            return None;
        }
        let (top, bottom) = self.ui[f].revealed[h];
        let bottom = bottom.min(len);
        if off < top {
            Some(r0 + off)
        } else if off >= len - bottom {
            let hidden = len.saturating_sub(top + bottom);
            Some(r0 + top + u32::from(hidden > 0) + (off - (len - bottom)))
        } else {
            None
        }
    }

    /// Where row `r` sits, to find it again after the rows change.
    fn place(&self, r: Row) -> Place {
        let f = r.file();
        match self
            .row_line(r, Side::Old)
            .map(|n| (Side::Old, n))
            .or_else(|| self.row_line(r, Side::New).map(|n| (Side::New, n)))
        {
            Some((side, n)) => Place::Line(f, side, n),
            None => Place::File(f),
        }
    }

    fn row_at(&self, p: Place) -> Option<u32> {
        match p {
            Place::File(f) => self.file_rows.get(f).copied(),
            Place::Line(f, side, n) => self.row_of(f, side, n).or_else(|| self.file_rows.get(f).copied()),
        }
    }

    /// Lay the rows out again from the files, modes and expanded ranges. No allocation per
    /// line: the rows vector and per-hunk tables only.
    fn rebuild(&mut self) {
        self.rev += 1;
        self.nav = None;
        let top = (!self.visible_rows.is_empty())
            .then(|| self.rows.get(self.visible_rows.start).map(|&r| self.place(r)))
            .flatten();
        // Written over the old rows, so a layout that comes out the same keeps the row keys.
        let mut rows = RowsOut {
            rows: std::mem::take(&mut self.rows),
            n: 0,
            same: true,
        };
        let old_len = rows.rows.len();
        let estimate: usize = self.files.iter().map(|f| f.lines.len() + 2 * f.hunks.len() + 2).sum();
        rows.rows.reserve(estimate.saturating_sub(old_len));
        self.file_rows.clear();
        self.hunk_rows.clear();
        self.gap_rows.clear();
        self.hunk_line_rows.clear();
        self.tall_rows.clear();
        let pairs = self.pairs();
        let m = self.m;
        let mut extra = 0.0f32;
        let tall_rows = &mut self.tall_rows;
        let mut tall = |rows: &RowsOut, h: f32| {
            extra += h - m.line;
            tall_rows.push((rows.n as u32 - 1, extra));
        };
        for (f, file) in self.files.iter().enumerate() {
            let fu = f as u32;
            let ix = &self.idx[f];
            self.file_rows.push(rows.n as u32);
            rows.push(Row::File(fu));
            tall(&rows, m.header);
            if self.ui[f].collapsed {
                self.gap_rows.extend(std::iter::repeat_n(NONE, file.hunks.len() + 1));
                self.hunk_line_rows.extend(std::iter::repeat_n(NONE, file.hunks.len()));
                continue;
            }
            if file.hunks.is_empty() {
                self.gap_rows.push(NONE);
                rows.push(Row::Note(fu));
                tall(&rows, m.note);
                continue;
            }
            let mut blocks = ix.blocks.iter().peekable();
            let mut folded = false;
            for gap in 0..=file.hunks.len() {
                let g = ix.gaps[gap];
                let (top, bottom) = self.ui[f].revealed[gap];
                self.gap_rows.push(rows.n as u32);
                let ctx = |k: u32| Row::Ctx {
                    f: fu,
                    old: g.old + k,
                    new: g.new + k,
                };
                (0..top).for_each(|k| rows.push(ctx(k)));
                let hidden = g.len.map(|n| n.saturating_sub(top + bottom));
                let first_row = rows.n as u32;
                // A tail of unknown length gets a row (to load the file) when no other row offers it.
                let tail_unknown = hidden.is_none()
                    && gap == file.hunks.len()
                    && !file.binary
                    && !folded
                    && !matches!(file.status, FileStatus::Added | FileStatus::Deleted);
                if hidden.is_some_and(|n| n > 0) || tail_unknown {
                    rows.push(Row::Gap { f: fu, gap: gap as u32 });
                    tall(&rows, m.gap);
                    folded = true;
                }
                if let Some(n) = g.len {
                    (n - bottom.min(n)..n).for_each(|k| rows.push(ctx(k)));
                }
                let Some(h) = file.hunks.get(gap) else { break };
                self.hunk_rows.push(first_row);
                self.hunk_line_rows.push(rows.n as u32);
                let mut i = h.lines.start;
                while i < h.lines.end {
                    if pairs && file.lines[i as usize].kind != LineKind::Context {
                        while blocks.next_if(|b| b.e <= i).is_some() {}
                        if let Some(b) = blocks.peek().filter(|b| b.d0 == i) {
                            let (nd, na) = (b.a0 - b.d0, b.e - b.a0);
                            for k in 0..nd.max(na) {
                                rows.push(match (k < nd, k < na) {
                                    (true, true) => Row::Pair {
                                        f: fu,
                                        del: b.d0 + k,
                                        add: b.a0 + k,
                                    },
                                    (true, false) => Row::Line { f: fu, i: b.d0 + k },
                                    _ => Row::Line { f: fu, i: b.a0 + k },
                                });
                            }
                            i = b.e;
                            continue;
                        }
                    }
                    rows.push(Row::Line { f: fu, i });
                    i += 1;
                }
            }
        }
        rows.rows.truncate(rows.n);
        let same = rows.same && rows.n == old_len;
        self.rows = rows.rows;
        // Keys come from the rows and the files' line numbers, which change only with new files.
        if std::mem::take(&mut self.new_files) || !same {
            static KEYS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            self.keys_rev = KEYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !self.visible_rows.is_empty() {
            let len = self.visible_rows.len();
            self.visible_rows = match top.and_then(|p| self.row_at(p)) {
                Some(start) => start as usize..(start as usize + len).min(self.rows.len()),
                None => 0..0,
            };
        }
        self.spot_rows = self.find_spot_rows();
    }

    /// A row's key: the same for an unchanged or removed line in both modes, so switching moves
    /// it rather than replacing it. Formatted on demand: the state keeps no key strings.
    pub(crate) fn key_of(&self, r: Row) -> String {
        match r {
            Row::File(f) => format!("df{f}"),
            Row::Note(f) => format!("df{f}.note"),
            Row::Gap { f, gap } => format!("df{f}.g{gap}"),
            Row::Ctx { f, old, .. } => format!("df{f}.c{old}"),
            Row::Line { f, i } => {
                let l = &self.files[f as usize].lines[i as usize];
                match l.kind {
                    LineKind::Context => format!("df{f}.c{}", l.old),
                    LineKind::Removed => format!("df{f}.o{}", l.old),
                    LineKind::Added => format!("df{f}.n{}", l.new),
                }
            }
            Row::Pair { f, del, .. } => format!("df{f}.o{}", self.files[f as usize].lines[del as usize].old),
        }
    }

    /// Row `i`'s key.
    pub fn row_key(&self, i: usize) -> Option<String> {
        self.rows.get(i).map(|&r| self.key_of(r))
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The rows the virtual list last reported on screen.
    pub fn visible_rows(&self) -> Range<usize> {
        self.visible_rows.clone()
    }

    /// Each file's header row, by file.
    pub fn file_rows(&self) -> &[u32] {
        &self.file_rows
    }

    /// The row of hunk `hunk`'s first line in `file`; `None` when the file is collapsed.
    pub fn hunk_row(&self, file: usize, hunk: usize) -> Option<u32> {
        let ix = self.idx.get(file)?;
        if hunk >= self.files[file].hunks.len() {
            return None;
        }
        self.hunk_line_rows
            .get((ix.hunk_base + hunk as u32) as usize)
            .copied()
            .filter(|&r| r != NONE)
    }

    /// The file row `row` belongs to.
    pub fn row_file(&self, row: usize) -> Option<usize> {
        self.rows.get(row).map(|r| r.file())
    }

    /// The row showing `file`'s line `at`, if one does (not in a folded range or collapsed file).
    pub fn row_of_line(&self, file: usize, at: LineRef) -> Option<u32> {
        self.row_of(file, at.side, at.line)
    }

    /// Each annotation that has a row, by the row of the line it sits under, in row order (in
    /// annotation order within a row), with its id.
    pub fn annotation_rows(&self) -> Vec<(u32, &str)> {
        let mut out: Vec<(u32, &str)> = self
            .annotations
            .iter()
            .filter_map(|a| {
                let at = a.at();
                Some((self.row_of(a.file, at.side, at.line)?, a.id.as_str()))
            })
            .collect();
        out.sort_by_key(|&(r, _)| r);
        out
    }

    /// Estimated top of row `row` in points, from the fixed row heights (unwrapped lines, no
    /// annotation widgets): for placing marks in proportion to the scrolled content.
    pub fn row_y(&self, row: usize) -> f32 {
        let p = self.tall_rows.partition_point(|&(r, _)| (r as usize) < row);
        let extra = p.checked_sub(1).map_or(0.0, |i| self.tall_rows[i].1);
        row as f32 * self.m.line + extra
    }

    /// Estimated height of all rows in points, as [`DiffState::row_y`] measures.
    pub fn rows_height(&self) -> f32 {
        self.row_y(self.rows.len())
    }

    /// The digits `file`'s line numbers take.
    pub(crate) fn digits(&self, file: usize) -> u32 {
        self.idx.get(file).map_or(3, |ix| ix.digits)
    }

    /// The widest line of any file, in columns.
    pub(crate) fn columns(&self) -> u32 {
        self.columns
    }

    /// Bytes the state holds beyond its files: rows, indexes and tables (capacity, not length).
    pub fn heap_bytes(&self) -> usize {
        use std::mem::size_of;
        let idx: usize = self
            .idx
            .iter()
            .map(|ix| {
                (ix.side[0].capacity() + ix.side[1].capacity()) * 4
                    + ix.blocks.capacity() * size_of::<Block>()
                    + ix.gaps.capacity() * size_of::<Gap>()
                    + ix.texts.iter().flatten().map(|t| t.len() as usize * 8).sum::<usize>()
            })
            .sum();
        self.rows.capacity() * size_of::<Row>()
            + idx
            + self.idx.capacity() * size_of::<FileIdx>()
            + (self.file_rows.capacity() + self.gap_rows.capacity() + self.hunk_line_rows.capacity() + self.hunk_rows.capacity()) * 4
            + self.tall_rows.capacity() * size_of::<(u32, f32)>()
            + self.hunk_ids.capacity() * 8
            + self.ui.iter().map(|u| u.revealed.capacity() * 8).sum::<usize>()
    }
}

/// The derived tables for one file: one pass over its lines.
fn index_file(f: &FileDiff) -> FileIdx {
    let (mut old, mut new) = (0usize, 0usize);
    for l in &f.lines {
        old += usize::from(l.old != 0);
        new += usize::from(l.new != 0);
    }
    let mut side = [Vec::with_capacity(old), Vec::with_capacity(new)];
    for (i, l) in f.lines.iter().enumerate() {
        if l.old != 0 {
            side[0].push(i as u32);
        }
        if l.new != 0 {
            side[1].push(i as u32);
        }
    }
    let texts = [f.old_text.as_deref().map(LineIndex::new), f.new_text.as_deref().map(LineIndex::new)];
    let mut ix = FileIdx {
        side,
        blocks: model::blocks(f),
        gaps: Vec::new(),
        texts,
        digits: 3,
        gap_base: 0,
        hunk_base: 0,
    };
    ix.gaps = gaps(f, &ix);
    let last = last_line(f, &ix);
    ix.digits = (last.max(1).ilog10() + 1).max(3);
    ix
}

/// The widest line of `f`'s hunks and whole texts.
fn text_columns(f: &FileDiff, ix: &FileIdx) -> u32 {
    let texts = [(&ix.texts[0], &f.old_text), (&ix.texts[1], &f.new_text)];
    let whole = texts
        .into_iter()
        .filter_map(|(i, t)| i.as_ref().zip(t.as_deref()))
        .flat_map(|(i, t)| (1..=i.len()).filter_map(move |n| i.line(t, n)))
        .map(model::columns)
        .max()
        .unwrap_or(0);
    f.max_columns.max(whole)
}

/// How many lines the old side has, when known: from the old text, or the new text and the
/// hunks' line counts.
fn old_len(f: &FileDiff, ix: &FileIdx) -> Option<u32> {
    if let Some(t) = &ix.texts[0] {
        return Some(t.len());
    }
    let t = ix.texts[1].as_ref()?;
    Some((i64::from(t.len()) - i64::from(f.additions) + i64::from(f.deletions)).max(0) as u32)
}

/// The largest line number either side shows.
fn last_line(f: &FileDiff, ix: &FileIdx) -> u32 {
    let hunks = f
        .hunks
        .last()
        .map_or(0, |h| model::end(h, Side::Old).max(model::end(h, Side::New)).saturating_sub(1));
    let texts = ix.texts.iter().flatten().map(LineIndex::len).max().unwrap_or(0);
    hunks.max(texts)
}

/// The unchanged ranges before each hunk of `f` and after its last.
fn gaps(f: &FileDiff, ix: &FileIdx) -> Vec<Gap> {
    let mut out = Vec::with_capacity(f.hunks.len() + 1);
    let (mut old, mut new) = (1, 1);
    for h in &f.hunks {
        let first = model::first(h, Side::Old);
        out.push(Gap {
            old,
            new,
            len: Some(first.saturating_sub(old)),
        });
        old = model::end(h, Side::Old);
        new = model::end(h, Side::New);
    }
    let len = old_len(f, ix).map(|n| (n + 1).saturating_sub(old));
    out.push(Gap {
        old,
        new,
        len: if f.hunks.is_empty() { Some(0) } else { len },
    });
    out
}

/// The side a file has lines on when it has only one: added (and binary, rename-only or
/// unavailable: nothing to pair) on the new side, deleted on the old.
fn one_sided(f: &FileDiff) -> Option<Side> {
    match f.status {
        FileStatus::Added => Some(Side::New),
        FileStatus::Deleted => Some(Side::Old),
        _ if f.binary || f.contents_unavailable || f.hunks.is_empty() => Some(Side::New),
        _ => None,
    }
}

/// Rows written over the last layout's, noting whether any came out different.
struct RowsOut {
    rows: Vec<Row>,
    n: usize,
    same: bool,
}

impl RowsOut {
    fn push(&mut self, r: Row) {
        match self.rows.get_mut(self.n) {
            Some(old) if *old == r => {}
            Some(old) => {
                *old = r;
                self.same = false;
            }
            None => {
                self.rows.push(r);
                self.same = false;
            }
        }
        self.n += 1;
    }
}

fn first_u64(h: blake3::Hash) -> u64 {
    let b = h.as_bytes();
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Each hunk's own id: its path and first and last changed lines (all its lines when none
/// changed), with a repeat count so equal hunks in one file differ.
fn own_ids(f: &FileDiff) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::with_capacity(f.hunks.len());
    let mut seen = std::collections::HashSet::with_capacity(f.hunks.len());
    // Per full hash, the last repeat count taken: every count below it is already used, so the
    // next equal hunk starts after it and gets the id counting from 0 would.
    let mut repeats: HashMap<[u8; 32], u32> = HashMap::new();
    for h in &f.hunks {
        let changed = |i: &u32| f.lines[*i as usize].kind != LineKind::Context;
        let first = h
            .lines
            .clone()
            .find(changed)
            .or(Some(h.lines.start).filter(|_| !h.lines.is_empty()));
        let last = h
            .lines
            .clone()
            .rev()
            .find(changed)
            .or(h.lines.end.checked_sub(1).filter(|_| !h.lines.is_empty()));
        let mut hasher = blake3::Hasher::new();
        hasher.update(f.path.as_bytes());
        for i in [first, last].into_iter().flatten() {
            hasher.update(&[0, f.lines[i as usize].kind as u8]);
            hasher.update(f.line_text(i).as_bytes());
        }
        let full = hasher.finalize();
        let mut id = first_u64(full);
        if !seen.insert(id) {
            let n = repeats.entry(*full.as_bytes()).or_insert(0);
            loop {
                *n += 1;
                let mut again = hasher.clone();
                again.update(&n.to_le_bytes());
                id = first_u64(again.finalize());
                if seen.insert(id) {
                    break;
                }
            }
        }
        out.push(id);
    }
    out
}

/// A hunk's old-side lines as a closed range (a pure insertion is the point where it goes).
fn old_range(h: &Hunk) -> (u32, u32) {
    let a = model::first(h, Side::Old);
    (a, model::end(h, Side::Old).saturating_sub(1).max(a))
}

/// Hunk ids for `files`, carried from `prev` (shown with `prev_ids`, indexed by `prev_idx`'s
/// hunk bases) by path, or old path for a rename: a hunk with the same own id keeps it; else
/// the hunks it overlaps on the old side give theirs, the biggest's when several, bigger new
/// hunks claiming first so a split hunk's smaller part gets its own.
fn reconcile_ids(prev: &[Arc<FileDiff>], prev_idx: &[FileIdx], prev_ids: &[u64], files: &[Arc<FileDiff>]) -> Vec<u64> {
    let by_path: HashMap<&str, usize> = prev.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
    let mut out = Vec::with_capacity(files.iter().map(|f| f.hunks.len()).sum());
    for f in files {
        let mut ids = own_ids(f);
        let old = by_path
            .get(f.path.as_str())
            .or_else(|| f.old_path.as_deref().and_then(|p| by_path.get(p)))
            .copied()
            .filter(|&k| prev_idx.get(k).is_some());
        if let Some(k) = old {
            let of = &prev[k];
            let base = prev_idx[k].hunk_base as usize;
            let Some(old_ids) = prev_ids.get(base..base + of.hunks.len()) else {
                out.extend(ids);
                continue;
            };
            let mut taken = vec![false; old_ids.len()];
            let mut settled = vec![false; ids.len()];
            // Each id's first position, as a scan from the front would find it.
            let mut first_of: HashMap<u64, usize> = HashMap::with_capacity(old_ids.len());
            for (o, &id) in old_ids.iter().enumerate() {
                first_of.entry(id).or_insert(o);
            }
            for (j, id) in ids.iter().enumerate() {
                if let Some(&o) = first_of.get(id).filter(|&&o| !taken[o]) {
                    taken[o] = true;
                    settled[j] = true;
                }
            }
            let mut order: Vec<usize> = (0..ids.len()).filter(|&j| !settled[j]).collect();
            order.sort_by_key(|&j| std::cmp::Reverse(f.hunks[j].lines.len()));
            let ranges: Vec<(u32, u32)> = of.hunks.iter().map(old_range).collect();
            // Hunks in diff order have both ends ascending; then only a window can overlap.
            let sorted = ranges.windows(2).all(|w| w[0].0 <= w[1].0 && w[0].1 <= w[1].1);
            for j in order {
                let (a, b) = old_range(&f.hunks[j]);
                let from = if sorted { ranges.partition_point(|&(_, d)| d < a) } else { 0 };
                let best = ranges[from..]
                    .iter()
                    .enumerate()
                    .map(|(k, &r)| (from + k, r))
                    .take_while(|&(_, (c, _))| !sorted || c <= b)
                    .filter(|&(o, (c, d))| !taken[o] && a <= d && c <= b)
                    .max_by_key(|&(o, _)| (of.hunks[o].lines.len(), std::cmp::Reverse(o)))
                    .map(|(o, _)| o);
                if let Some(o) = best {
                    taken[o] = true;
                    ids[j] = old_ids[o];
                }
            }
        }
        out.extend(ids);
    }
    out
}

/// `df{f}.{c|o|n}{n}` as (file, kind, number).
fn parse_key(s: &str) -> Option<(usize, u8, u32)> {
    let rest = s.strip_prefix("df")?;
    let (f, line) = rest.split_once('.')?;
    let kind = *line.as_bytes().first()?;
    if !matches!(kind, b'c' | b'o' | b'n') {
        return None;
    }
    Some((f.parse().ok()?, kind, line[1..].parse().ok()?))
}

/// The byte offset of char column `col` in `text` (its end when past it).
pub(crate) fn byte_of_col(text: &str, col: u32) -> usize {
    text.char_indices().nth(col as usize).map_or(text.len(), |(b, _)| b)
}

/// The char column in `orig` of byte `b` of its drawn form (tabs padded, and with `wrap`,
/// zero-width spaces after `.`, `(`, `[`, `/`: see `view::tabbed`).
pub(crate) fn shown_to_col(orig: &str, wrap: bool, b: usize) -> u32 {
    let mut shown = 0usize;
    let mut col = 0usize;
    let mut chars = 0u32;
    let mut it = orig.chars().peekable();
    while let Some(ch) = it.next() {
        if shown >= b {
            return chars;
        }
        shown += ch.len_utf8();
        col += 1;
        if ch == '\t' {
            let pad = 3 - (col - 1) % 4;
            shown += pad;
            col += pad;
        } else if wrap && matches!(ch, '.' | '(' | '[' | '/') && it.peek().is_some_and(|c| !c.is_whitespace()) {
            shown += '\u{200B}'.len_utf8();
        }
        chars += 1;
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{from_texts, parse};

    fn numbered(n: u32) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    fn sample() -> DiffState {
        let old = numbered(100);
        let new = old.replace("line 50\n", "line fifty\n");
        DiffState::new(vec![from_texts("a.txt", &old, &new)])
    }

    fn shown(s: &DiffState) -> Vec<String> {
        s.rows
            .iter()
            .map(|&r| match r {
                Row::File(_) => "file".into(),
                Row::Note(_) => "note".into(),
                Row::Gap { gap, .. } => format!("gap{gap}:{}", s.hidden(0, gap as usize).unwrap()),
                Row::Ctx { old, new, .. } => format!("{old}={new}"),
                Row::Line { .. } | Row::Pair { .. } => {
                    let (o, n) = (s.row_line(r, Side::Old), s.row_line(r, Side::New));
                    format!(
                        "{}|{}",
                        o.map_or("-".into(), |v| v.to_string()),
                        n.map_or("-".into(), |v| v.to_string())
                    )
                }
            })
            .collect()
    }

    #[test]
    fn a_row_is_sixteen_bytes() {
        assert!(std::mem::size_of::<Row>() <= 16, "Row is {} bytes", std::mem::size_of::<Row>());
    }

    #[test]
    fn collapsed_ranges_expand_by_twenty_and_all() {
        let mut s = sample();
        assert_eq!(shown(&s)[..3], ["file", "gap0:46", "47|47"]);
        assert_eq!(s.hidden(0, 1), Some(47));
        s.expand(0, 0, Expand::Up);
        assert_eq!(s.hidden(0, 0), Some(26));
        let rows = shown(&s);
        assert_eq!(rows[1..4], ["gap0:26", "27=27", "28=28"]);
        s.expand(0, 1, Expand::Down);
        let rows = shown(&s);
        let after = rows.iter().position(|r| r == "53|53").unwrap();
        assert_eq!(rows[after + 1..after + 3], ["54=54", "55=55"]);
        assert_eq!(rows.last().unwrap(), "gap1:27");
        s.expand(0, 1, Expand::All);
        assert_eq!(s.hidden(0, 1), Some(0));
        assert_eq!(shown(&s).last().unwrap(), "100=100");
        s.expand(0, 0, Expand::All);
        assert_eq!(shown(&s).len(), 1 + 101);
        assert_eq!(s.side_line(0, Side::Old, 10), Some("line 10"));
        assert_eq!(s.side_line(0, Side::New, 50), Some("line fifty"));
    }

    #[test]
    fn collapse_expanded_folds_every_opened_range() {
        let mut s = sample();
        let folded = shown(&s);
        s.expand(0, 0, Expand::Up);
        s.expand(0, 1, Expand::All);
        assert_ne!(shown(&s), folded);
        s.collapse_expanded(0);
        assert_eq!(shown(&s), folded);
        assert_eq!((s.hidden(0, 0), s.hidden(0, 1)), (Some(46), Some(47)));
    }

    #[test]
    fn texts_given_later_make_a_parsed_file_expandable() {
        let old = numbered(100);
        let new = old.replace("line 50\n", "line fifty\n");
        let f = from_texts("a.txt", &old, &new);
        let parsed = DiffState::new(vec![rain_core::diff::FileDiff {
            old_text: None,
            new_text: None,
            ..f
        }]);
        let mut s = parsed;
        assert!(!s.expandable(0));
        assert_eq!(s.hidden(0, 1), None);
        s.set_texts(0, Some(Arc::from(old.as_str())), Some(Arc::from(new.as_str())));
        assert!(s.expandable(0));
        assert_eq!(s.hidden(0, 1), Some(47));
        s.expand(0, 1, Expand::All);
        assert_eq!(shown(&s).last().unwrap(), "100=100");
        s.set_texts(0, None, None);
        assert_eq!(s.hidden(0, 1), None);

        s.request_expand(0, 0, Expand::All);
        assert!(s.hidden(0, 0).is_some_and(|n| n > 0), "waits for the texts");
        s.set_texts(0, Some(Arc::from(old.as_str())), Some(Arc::from(new.as_str())));
        assert_eq!(s.hidden(0, 0), Some(0), "the asked-for expansion lands with them");
    }

    #[test]
    fn whole_file_changes_rebuild_once() {
        let old = numbered(100);
        let new = old.replace("line 50\n", "line fifty\n");
        let mut s = DiffState::new((0..4).map(|i| from_texts(&format!("f{i}"), &old, &new)).collect());
        let rev = s.rev;
        s.set_whole_where(true, |f| f.path != "f2");
        assert_eq!(s.rev, rev + 1, "expand-all is one rebuild");
        s.set_whole_where(true, |f| f.path != "f2");
        assert_eq!(s.rev, rev + 1, "nothing left to open: no rebuild");
        s.set_whole(0, true);
        assert_eq!(s.rev, rev + 1);
        s.set_whole_where(false, |_| true);
        assert_eq!(s.rev, rev + 2);
        assert!(s.ui.iter().all(|u| !u.whole && u.revealed.iter().all(|&r| r == (0, 0))));
    }

    #[test]
    fn split_pairs_changes_and_keeps_keys() {
        let old = "a\nb\nc\nd\n";
        let new = "a\nB\nC\nX\nd\n";
        let mut s = DiffState::new(vec![from_texts("x", old, new)]);
        let unified = shown(&s);
        assert_eq!(unified, ["file", "1|1", "2|-", "3|-", "-|2", "-|3", "-|4", "4|5"]);
        let key_b = s.key_of(s.rows[2]);
        s.set_mode(DiffMode::Split);
        assert_eq!(shown(&s), ["file", "1|1", "2|2", "3|3", "-|4", "4|5"]);
        assert_eq!(s.key_of(s.rows[2]), key_b);
    }

    #[test]
    fn a_tail_of_unknown_length_still_has_a_row_to_load_it() {
        let top = "diff --git a/m b/m\n--- a/m\n+++ b/m\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n";
        let s = DiffState::new(parse(top));
        assert_eq!(s.hidden(0, 1), None);
        assert!(matches!(s.rows.last(), Some(Row::Gap { gap: 1, .. })), "{:?}", s.rows);

        let mid = "diff --git a/m b/m\n--- a/m\n+++ b/m\n@@ -5,2 +5,2 @@\n-a\n+b\n c\n";
        let s = DiffState::new(parse(mid));
        let gaps: Vec<u32> = s
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Gap { gap, .. } => Some(*gap),
                _ => None,
            })
            .collect();
        assert_eq!(gaps, [0], "the first gap's row already loads the file");
    }

    #[test]
    fn one_sided_files_lay_out_single_in_split_with_one_row_per_line() {
        let patch = "diff --git a/m b/m\n--- a/m\n+++ b/m\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n\
                     diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@\n+x\n+y\n+z\n\
                     diff --git a/g b/g\ndeleted file mode 100644\n--- a/g\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-p\n-q\n";
        let mut s = DiffState::new(parse(patch));
        s.set_mode(DiffMode::Split);
        let of = |s: &DiffState, f: u32| s.rows.iter().filter(|r| r.file() == f as usize).copied().collect::<Vec<_>>();
        assert!(of(&s, 0).iter().any(|r| matches!(r, Row::Pair { .. })), "a modified file pairs");
        for f in [1, 2] {
            let rows = of(&s, f);
            assert!(rows[1..].iter().all(|r| matches!(r, Row::Line { .. })), "file {f}: {rows:?}");
            assert_eq!(rows.len(), 1 + s.files[f as usize].lines.len());
            assert_eq!(s.layout(f as usize), Layout::Single);
            assert_eq!(s.draw(f as usize), Draw::One(if f == 1 { Side::New } else { Side::Old }));
        }
        assert_eq!(s.layout(0), Layout::Split);
        s.set_min_split_columns(90);
        s.set_columns_per_side(60);
        assert!(of(&s, 0).iter().all(|r| !matches!(r, Row::Pair { .. })), "narrow: no pairs");
        assert_row_of_matches(&s, "narrow split");
        s.set_columns_per_side(120);
        assert!(of(&s, 0).iter().any(|r| matches!(r, Row::Pair { .. })));
    }

    /// Every (file, side, line) a row shows, the way the vendored state indexed them.
    fn hashed(s: &DiffState) -> HashMap<(u32, Side, u32), u32> {
        let mut m = HashMap::new();
        for (i, &r) in s.rows.iter().enumerate() {
            for side in [Side::Old, Side::New] {
                if let Some(n) = s.row_line(r, side) {
                    m.insert((r.file() as u32, side, n), i as u32);
                }
            }
        }
        m
    }

    fn assert_row_of_matches(s: &DiffState, what: &str) {
        let m = hashed(s);
        assert!(!m.is_empty());
        for (&(f, side, n), &r) in &m {
            assert_eq!(s.row_of(f as usize, side, n), Some(r), "{what}: file {f} {side:?} {n}");
        }
        for f in 0..s.files().len() {
            for side in [Side::Old, Side::New] {
                for n in 0..2000 {
                    if !m.contains_key(&(f as u32, side, n)) {
                        assert_eq!(s.row_of(f, side, n), None, "{what}: file {f} {side:?} {n} has no row");
                    }
                }
            }
        }
    }

    #[test]
    fn row_of_equals_the_old_hash_map_on_fixture_m() {
        let patch = include_str!("../../../fixtures/m.patch");
        let mut s = DiffState::new(parse(patch));
        assert!(s.files().len() > 10);
        assert_row_of_matches(&s, "unified");
        s.set_mode(DiffMode::Split);
        assert_row_of_matches(&s, "split");
        s.set_collapsed(1, true);
        assert_row_of_matches(&s, "split, one collapsed");
        let mut e = sample();
        e.expand(0, 0, Expand::Up);
        e.expand(0, 1, Expand::Down);
        assert_row_of_matches(&e, "expanded");
        let old = "a\nb\nc\nd\ne\nf\n";
        let new = "a\nX\nf\nY\nZ\nW\n";
        let mut lopsided = DiffState::new(vec![from_texts("x", old, new)]);
        assert_row_of_matches(&lopsided, "lopsided unified");
        lopsided.set_mode(DiffMode::Split);
        assert_row_of_matches(&lopsided, "lopsided split");
    }

    #[test]
    fn line_selection_copies_one_side() {
        let old = "keep\nold one\nold two\nend\n";
        let new = "keep\nnew one\nnew two\nend\n";
        let mut s = DiffState::new(vec![from_texts("x", old, new)]);
        s.press_line(0, LineRef::new(Side::New, 1));
        s.release_line();
        s.shift = true;
        s.press_line(0, LineRef::new(Side::New, 4));
        s.release_line();
        assert_eq!(s.selected_text().as_deref(), Some("keep\nnew one\nnew two\nend"));
        s.set_selection(Some(LineRange {
            file: 0,
            start: LineRef::new(Side::Old, 3),
            end: LineRef::new(Side::Old, 2),
        }));
        assert_eq!(s.selected_text().as_deref(), Some("old one\nold two"));
        s.shift = false;
        let one = LineRange::line(0, LineRef::new(Side::New, 1));
        s.press_line(0, one.start);
        s.release_line();
        s.press_line(0, one.start);
        assert_eq!(s.release_line(), None);
        s.press_line(0, one.start);
        s.release_line();
        s.press_line(0, one.start);
        s.drag_to(0, LineRef::new(Side::New, 2));
        assert_eq!(
            s.release_line(),
            Some(LineRange {
                end: LineRef::new(Side::New, 2),
                ..one
            })
        );
    }

    #[test]
    fn cancelling_a_gutter_drag_restores_the_selection_before_the_press() {
        let mut s = sample();
        for original in [None, Some(LineRange::line(0, LineRef::new(Side::Old, 2)))] {
            s.set_selection(original);
            s.press_line(0, LineRef::new(Side::New, 1));
            s.drag_to(0, LineRef::new(Side::New, 3));
            s.cancel_line();
            assert_eq!(s.selection(), original);
            assert!(!s.selecting && !s.unselect);
        }
    }

    #[test]
    fn copy_follows_the_hovered_side_unless_a_press_holds_the_other() {
        let mut s = sample();
        s.hover_text(Side::Old, "a", true);
        assert_eq!(s.copy_side(), Side::Old);
        s.hover_text(Side::New, "b", true);
        assert_eq!(s.copy_side(), Side::Old);
        s.hover_text(Side::Old, "a", false);
        s.hover_text(Side::New, "b", false);
        s.hover_text(Side::New, "c", true);
        assert_eq!(s.copy_side(), Side::New);
    }

    #[test]
    fn the_visible_file_follows_rows_at_the_top() {
        let files = (0..3).map(|i| from_texts(&format!("f{i}"), "a\n", "b\n")).collect();
        let mut s = DiffState::new(files);
        assert_eq!(s.rows.len(), 9);
        assert_eq!(s.set_visible_rows(0..6), (Some(0), false));
        assert_eq!(s.sticky(), None);
        assert_eq!(s.set_visible_rows(1..7), (None, true));
        assert_eq!((s.visible_file(), s.sticky()), (0, Some(0)));
        assert_eq!(s.set_visible_rows(3..9), (Some(1), true));
        assert_eq!((s.visible_file(), s.sticky()), (1, None));
        assert_eq!(s.set_visible_rows(4..9), (None, true));
        assert_eq!((s.visible_file(), s.sticky()), (1, Some(1)));
        assert_eq!(s.set_visible_rows(7..9), (Some(2), true));
        assert_eq!((s.visible_file(), s.sticky()), (2, Some(2)));
        assert_eq!(s.row_from(7, 25.0), Some(8));
    }

    #[test]
    fn a_mode_switch_keeps_the_reader_on_the_same_line() {
        let old: String = (1..=40).map(|i| format!("v{i}\n")).collect();
        let new = old.replace("v10\n", "ten\n").replace("v20\n", "twenty\n");
        let mut s = DiffState::new(vec![from_texts("x", &old, &new)]);
        s.expand(0, 1, Expand::All);
        let r = s.row_of(0, Side::Old, 22).unwrap() as usize;
        s.set_visible_rows(r..r + 10);
        s.set_mode(DiffMode::Split);
        let top = s.visible_rows.start;
        assert_eq!(s.row_line(s.rows[top], Side::Old), Some(22));
    }

    #[test]
    fn word_changes_are_computed_for_paired_lines_only() {
        let s = DiffState::new(vec![from_texts("a", "let a = 1;\nkeep\n", "let a = 2;\nkeep\nnew\n")]);
        let f = &s.files()[0];
        let removed = f.lines.iter().position(|l| l.kind == LineKind::Removed).unwrap() as u32;
        let added = f.lines.iter().position(|l| l.kind == LineKind::Added).unwrap() as u32;
        assert_eq!(&f.line_text(removed)[s.word_changes(0, removed)[0].clone()], "1");
        assert_eq!(&f.line_text(added)[s.word_changes(0, added)[0].clone()], "2");
        let last = f.lines.len() as u32 - 1;
        assert!(s.word_changes(0, last).is_empty());
    }

    #[test]
    fn span_annotations_mark_the_selected_bytes_of_each_line() {
        let mut s = DiffState::new(vec![from_texts("a", "a\nb\n", "héllo world\nsecond line\nthird\n")]);
        let span = Anchor::Span {
            side: Side::New,
            start_line: 1,
            start_col: 1,
            end_line: 3,
            end_col: 2,
        };
        s.set_annotations(vec![
            Annotation::new(0, span, "s"),
            Annotation::new(
                0,
                Anchor::Range {
                    side: Side::Old,
                    start: 1,
                    end: 2,
                },
                "r",
            ),
        ]);
        assert_eq!(s.span_marks(0, LineRef::new(Side::New, 1)), vec![1..12]);
        assert_eq!(s.span_marks(0, LineRef::new(Side::New, 2)), vec![0..11]);
        assert_eq!(s.span_marks(0, LineRef::new(Side::New, 3)), vec![0..2]);
        assert!(s.span_marks(0, LineRef::new(Side::Old, 1)).is_empty());
        assert_eq!(
            s.covering(0, LineRef::new(Side::Old, 2)).map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["r"]
        );
        assert_eq!(s.covering(0, LineRef::new(Side::New, 4)).count(), 0);
        assert_eq!(
            s.annotations_at(0, LineRef::new(Side::New, 3))
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["s"]
        );
    }

    #[test]
    fn shown_bytes_map_back_to_char_columns() {
        assert_eq!(shown_to_col("a\tb", false, 0), 0);
        assert_eq!(shown_to_col("a\tb", false, 4), 2);
        assert_eq!(shown_to_col("a\tb", false, 5), 3);
        assert_eq!(shown_to_col("a.b", true, 5), 2);
        assert_eq!(shown_to_col("é x", false, 2), 1);
        assert_eq!(byte_of_col("é x", 1), 2);
        assert_eq!(parse_key("df12.n340"), Some((12, b'n', 340)));
        assert_eq!(parse_key("df1.g2"), None);
    }

    #[test]
    fn hostile_and_fuzzed_patches_never_panic() {
        let cases = [
            "",
            "diff --git\n--- \n+++ \n@@ -4294967295,4294967295 +4294967295,4294967295 @@\n-a\n+b\n\\ No newline",
            "diff --git \"a/unclosed b/unclosed\n--- \"a/\\777\\x\n+++ b/\n@@ -0,0 +0,0 @@\n",
            "diff --git 🚀 a/x 🚀 b/y\n@@ -1,999999 +1,999999 @@\n+truncated",
            "--- a/f\n+++ b/f\n@@ -10,2 +10,2 @@\n ctx\n-old\n+new\n@@ -2,1 +2,1 @@\n-out_of_order\n+ok\n",
        ];
        for patch in cases {
            let mut s = DiffState::new(parse(patch));
            s.set_mode(DiffMode::Split);
        }
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed as usize) % n
        };
        let fragments = [
            "diff --git a/f.rs b/f.rs\n",
            "--- a/f.rs\n",
            "+++ b/f.rs\n",
            "--- /dev/null\n",
            "+++ /dev/null\n",
            "@@ -1,3 +1,3 @@ fn f()\n",
            "@@ -0,0 +1,5 @@\n",
            "@@ -4294967295,10 +0,4294967295 @@\n",
            "@@ malformed @@\n",
            " context line\n",
            "-removed line\n",
            "+added line\n",
            "\\ No newline at end of file\n",
            "Binary files a/x and b/x differ\n",
            "\r\n",
        ];
        for _ in 0..300 {
            let len = 1 + next(24);
            let patch: String = (0..len).map(|_| fragments[next(fragments.len())]).collect();
            let mut state = DiffState::new(parse(&patch));
            for mode in [DiffMode::Split, DiffMode::Unified] {
                state.set_mode(mode);
                for f in 0..state.files().len() {
                    for side in [Side::Old, Side::New] {
                        let _ = state.side_line(f, side, 1);
                        let _ = state.row_of(f, side, 1);
                    }
                }
                for r in 0..state.rows.len() {
                    let _ = state.key_of(state.rows[r]);
                }
            }
        }
    }

    /// `own_ids` as first written: each repeat counts up from 0 until its id is unused.
    fn own_ids_counting(f: &FileDiff) -> Vec<u64> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for h in &f.hunks {
            let changed = |i: &u32| f.lines[*i as usize].kind != LineKind::Context;
            let first = h
                .lines
                .clone()
                .find(changed)
                .or(Some(h.lines.start).filter(|_| !h.lines.is_empty()));
            let last = h
                .lines
                .clone()
                .rev()
                .find(changed)
                .or(h.lines.end.checked_sub(1).filter(|_| !h.lines.is_empty()));
            let mut hasher = blake3::Hasher::new();
            hasher.update(f.path.as_bytes());
            for i in [first, last].into_iter().flatten() {
                hasher.update(&[0, f.lines[i as usize].kind as u8]);
                hasher.update(f.line_text(i).as_bytes());
            }
            let mut id = first_u64(hasher.finalize());
            let mut n = 0u32;
            while !seen.insert(id) {
                n += 1;
                let mut again = hasher.clone();
                again.update(&n.to_le_bytes());
                id = first_u64(again.finalize());
            }
            out.push(id);
        }
        out
    }

    /// `reconcile_ids` as first written: linear scans for the exact and the overlap passes.
    fn reconcile_scanning(prev: &[Arc<FileDiff>], prev_idx: &[FileIdx], prev_ids: &[u64], files: &[Arc<FileDiff>]) -> Vec<u64> {
        let by_path: HashMap<&str, usize> = prev.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
        let mut out = Vec::new();
        for f in files {
            let mut ids = own_ids_counting(f);
            if let Some(&k) = by_path.get(f.path.as_str()) {
                let of = &prev[k];
                let base = prev_idx[k].hunk_base as usize;
                let old_ids = &prev_ids[base..base + of.hunks.len()];
                let mut taken = vec![false; old_ids.len()];
                let mut settled = vec![false; ids.len()];
                for (j, id) in ids.iter().enumerate() {
                    if let Some(o) = old_ids.iter().position(|o| o == id).filter(|&o| !taken[o]) {
                        taken[o] = true;
                        settled[j] = true;
                    }
                }
                let mut order: Vec<usize> = (0..ids.len()).filter(|&j| !settled[j]).collect();
                order.sort_by_key(|&j| std::cmp::Reverse(f.hunks[j].lines.len()));
                for j in order {
                    let (a, b) = old_range(&f.hunks[j]);
                    let best = of
                        .hunks
                        .iter()
                        .enumerate()
                        .filter(|&(o, h)| {
                            let (c, d) = old_range(h);
                            !taken[o] && a <= d && c <= b
                        })
                        .max_by_key(|&(o, h)| (h.lines.len(), std::cmp::Reverse(o)))
                        .map(|(o, _)| o);
                    if let Some(o) = best {
                        taken[o] = true;
                        ids[j] = old_ids[o];
                    }
                }
            }
            out.extend(ids);
        }
        out
    }

    /// 1,000 hunks, most with the same first and last changed lines (an EF snapshot's), some
    /// with another pair, so repeats of two ids interleave.
    fn snapshot_texts(value: impl Fn(usize) -> &'static str) -> (String, String) {
        let block = |k: usize, v: &str| -> String {
            let ctx: String = (0..8).map(|j| format!("ctx {k} {j}\n")).collect();
            format!("{ctx}HasMaxLength({v});\n")
        };
        let old = (0..1000).map(|k| block(k, if k % 7 == 0 { "100" } else { "200" })).collect();
        let new = (0..1000).map(|k| block(k, value(k))).collect();
        (old, new)
    }

    #[test]
    fn row_keys_change_only_with_the_rows_or_the_files() {
        let mut s = sample();
        let rev = s.keys_rev;
        s.set_metrics(Metrics {
            line: s.metrics().line + 4.0,
            ..s.metrics()
        });
        assert_eq!(s.keys_rev, rev, "a density change keeps every row");
        s.set_collapsed(0, true);
        assert_ne!(s.keys_rev, rev, "collapsing drops rows");
        let rev = s.keys_rev;
        s.set_files(s.files().to_vec());
        assert_ne!(s.keys_rev, rev, "new files always re-key");
    }

    #[test]
    fn repeated_hunk_ids_are_the_ones_counting_from_zero_gives() {
        let (old, new) = snapshot_texts(|k| if k % 7 == 0 { "128" } else { "256" });
        let f = from_texts("Model.Designer.cs", &old, &new);
        assert_eq!(f.hunks.len(), 1000);
        let ids = own_ids(&f);
        assert_eq!(ids, own_ids_counting(&f), "persisted viewed hunks keep their ids");
        assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), 1000);
    }

    #[test]
    fn carried_hunk_ids_are_the_ones_the_scans_give() {
        let (old, new) = snapshot_texts(|k| if k % 7 == 0 { "128" } else { "256" });
        let before = DiffState::new(vec![from_texts("Model.Designer.cs", &old, &new)]);
        // Some hunks change, some go, and an insertion shifts the rest.
        let (_, edited) = snapshot_texts(|k| match k % 11 {
            0 => "300",
            5 => "200",
            _ if k % 7 == 0 => "128",
            _ => "256",
        });
        let edited = edited.replacen("ctx 500 0\n", "ctx 500 0\nnew line\nanother\n", 1);
        let after = vec![Arc::new(from_texts("Model.Designer.cs", &old, &edited))];
        assert_eq!(
            reconcile_ids(&before.files, &before.idx, &before.hunk_ids, &after),
            reconcile_scanning(&before.files, &before.idx, &before.hunk_ids, &after)
        );
    }
}
