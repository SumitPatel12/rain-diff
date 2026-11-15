//! Drawing a [`DiffState`]: a virtual list of file headers, hunk separators and lines, a sticky
//! header for the file being read, and the handlers that feed selection, expansion, text spans
//! and the reading position back into the state.

use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use rain_core::diff::{FileStatus, LineKind, Side};
use rune::{
    Color, Cursor, Drag, El, GrowthSpec, Icon, Live, Phase, ScrollPos, ScrollSpec, Span, TextHighlight, TextStyle, Tx, col, icon, memo,
    rich, row, text,
};
use rune_kit::Kit;
use rune_kit::syntax::SyntaxTheme;

use crate::model;
use crate::state::{
    Annotation, DiffState, Draw, EXPAND_STEP, Expand, GAP_H, HEADER_H, LINE_H, LineRange, LineRef, NOTE_H, Row, SpanSelection,
};

static EXPAND_UP: Icon = Icon {
    name: "kit-diff-expand-up",
    view: 16.0,
    body: r#"<path d="M4.5 9.5L8 6L11.5 9.5" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static EXPAND_DOWN: Icon = Icon {
    name: "kit-diff-expand-down",
    view: 16.0,
    body: r#"<path d="M4.5 6.5L8 10L11.5 6.5" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static EXPAND_ALL: Icon = Icon {
    name: "kit-diff-expand-all",
    view: 16.0,
    body: r#"<path d="M5 5.5L8 2.5L11 5.5M5 10.5L8 13.5L11 10.5" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static PLUS: Icon = Icon {
    name: "kit-diff-plus",
    view: 16.0,
    body: r#"<path d="M8 4V12M4 8H12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>"#,
};

/// Width of the coloured bar that marks an added or removed line (and the cursor's line).
const BAR: f32 = 3.0;
/// Width of the bar that marks the lines a range or span comment covers.
const RANGE_BAR: f32 = 3.0;
/// Space between the line numbers and the code, and after the code.
const CODE_PAD: (f32, f32) = (12.0, 24.0);
/// An all-added file this long draws calm (`DiffView::calm_added_files`).
const CALM_LINES: u32 = 200;
/// How much rows outside a spotlight keep of their opacity.
const DIM: f32 = 0.45;
/// The spotlight outline's stroke.
const SPOT_W: f32 = 2.0;

/// The [`Tx`] scroll key of the viewport of a [`DiffView`] with the default key `"diff"`, for
/// `tx.scroll_to_y(SCROLL_KEY, y)`. A view keyed `k` scrolls as `"{k}/scroll"`.
pub const SCROLL_KEY: &str = "diff/scroll";

/// Fills a theme gives added and removed lines in place of the kit-derived tints.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ChangeFills {
    add: Color,
    del: Color,
}

type Get<S> = Rc<dyn Fn(&S) -> &DiffState>;
type GetMut<S> = Rc<dyn Fn(&mut S) -> &mut DiffState>;

/// Draws the [`DiffState`] an app keeps in its state: give it the state's accessors and build it
/// in the view. It fills the height it's given and scrolls; Cmd-C, `j`/`k`, `n`/`p` and `]`/`[`
/// work once the app routes keys to [`DiffState::key`].
///
/// ```ignore
/// DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff)
///     .annotation(|s: &App, a: &Annotation| comment(s, &a.id))
///     .on_comment(|s: &mut App, lines, _| s.compose(lines))
///     .on_span_select(|s: &mut App, span, _| s.offer_comment(span))
///     .on_visible_file(|s: &mut App, f| s.tree.reveal(f))
///     .build(s, &k)
/// ```
#[allow(clippy::type_complexity)]
pub struct DiffView<S> {
    get: Get<S>,
    get_mut: GetMut<S>,
    key: String,
    calm: bool,
    fills: Option<ChangeFills>,
    selection: Option<Color>,
    annotation: Option<Rc<dyn Fn(&S, &Annotation) -> El>>,
    header: Option<Rc<dyn Fn(&S, usize) -> El>>,
    header_annotation: Option<Rc<dyn Fn(&S, usize) -> Option<El>>>,
    on_select: Option<Rc<dyn Fn(&mut S, Option<LineRange>, &mut Tx)>>,
    on_comment: Option<Rc<dyn Fn(&mut S, LineRange, &mut Tx)>>,
    on_span_select: Option<Rc<dyn Fn(&mut S, Option<SpanSelection>, &mut Tx)>>,
    on_visible_file: Option<Rc<dyn Fn(&mut S, usize)>>,
    on_need_texts: Option<Rc<dyn Fn(&mut S, usize)>>,
    scroll_live: Option<Live<[f32; 4]>>,
    on_scroll: Option<Rc<dyn Fn(ScrollPos)>>,
}

impl<S: 'static> DiffView<S> {
    pub fn new(get: impl Fn(&S) -> &DiffState + 'static, get_mut: impl Fn(&mut S) -> &mut DiffState + 'static) -> DiffView<S> {
        DiffView {
            get: Rc::new(get),
            get_mut: Rc::new(get_mut),
            key: "diff".into(),
            calm: false,
            fills: None,
            selection: None,
            annotation: None,
            header: None,
            header_annotation: None,
            on_select: None,
            on_comment: None,
            on_span_select: None,
            on_visible_file: None,
            on_need_texts: None,
            scroll_live: None,
            on_scroll: None,
        }
    }

    /// Write where the viewport is scrolled into `live` every frame it moves, with no view run:
    /// `[offset_y, max_y, viewport_h, drawn_h]` in points (see [`ScrollPos`]).
    pub fn scroll_live(mut self, live: Live<[f32; 4]>) -> Self {
        self.scroll_live = Some(live);
        self
    }

    /// Called with where the viewport is scrolled in every frame it moves, before that frame
    /// draws; it can only set live values (see [`El::on_scroll_offset`]).
    pub fn on_scroll_offset(mut self, f: impl Fn(ScrollPos) + 'static) -> Self {
        self.on_scroll = Some(Rc::new(f));
        self
    }

    /// Offer to expand the unchanged ranges of files without whole texts: a press records the
    /// expansion and calls `f` with the file, which should fetch the texts and hand them to
    /// [`DiffState::set_texts`]; the expansion then happens.
    pub fn on_need_texts(mut self, f: impl Fn(&mut S, usize) + 'static) -> Self {
        self.on_need_texts = Some(Rc::new(f));
        self
    }

    /// The view's element key (`"diff"` by default); give each diff in a window its own.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }

    /// Draw files of 200 lines or more that are all added with only the bar by their lines,
    /// not the full green wash: a new file reads as text, not as a wall of change.
    pub fn calm_added_files(mut self, on: bool) -> Self {
        self.calm = on;
        self
    }

    /// Paint added and removed lines `add` and `del` rather than tints of the kit's success and
    /// danger colours; their gutters and changed words mix those colours in, as the tints do.
    pub fn change_fills(mut self, add: Color, del: Color) -> Self {
        self.fills = Some(ChangeFills { add, del });
        self
    }

    /// The text selection's colour, in place of the kit's.
    pub fn selection_color(mut self, c: Color) -> Self {
        self.selection = Some(c);
        self
    }

    /// Build each [`Annotation`] under its last line. Rows holding annotations are built every
    /// commit, so a widget can read app state (a composer's text) freely.
    pub fn annotation(mut self, f: impl Fn(&S, &Annotation) -> El + 'static) -> Self {
        self.annotation = Some(Rc::new(f));
        self
    }

    /// Extra controls at the end of each file's header (a "viewed" toggle, a menu), by file.
    pub fn header_actions(mut self, f: impl Fn(&S, usize) -> El + 'static) -> Self {
        self.header = Some(Rc::new(f));
        self
    }

    /// A widget under a file's header in the list (file-level comments), by file; `None` for
    /// none.
    pub fn header_annotation(mut self, f: impl Fn(&S, usize) -> Option<El> + 'static) -> Self {
        self.header_annotation = Some(Rc::new(f));
        self
    }

    /// Called when a click, shift-click or drag on the line numbers ends, with the selection.
    pub fn on_select(mut self, f: impl Fn(&mut S, Option<LineRange>, &mut Tx) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Show a "+" by the line numbers of the hovered line, and call `f` with the lines it stands
    /// for when pressed: the selection when the line is in it, else that line.
    pub fn on_comment(mut self, f: impl Fn(&mut S, LineRange, &mut Tx) + 'static) -> Self {
        self.on_comment = Some(Rc::new(f));
        self
    }

    /// Called when a drag over the code ends with text selected (the span, by line and column,
    /// and its text), and with `None` when that selection clears. The state keeps the latest in
    /// [`DiffState::span_selection`].
    pub fn on_span_select(mut self, f: impl Fn(&mut S, Option<SpanSelection>, &mut Tx) + 'static) -> Self {
        self.on_span_select = Some(Rc::new(f));
        self
    }

    /// Called when the file at the top of the view changes as it scrolls, to follow it in a
    /// file tree.
    pub fn on_visible_file(mut self, f: impl Fn(&mut S, usize) + 'static) -> Self {
        self.on_visible_file = Some(Rc::new(f));
        self
    }

    pub fn build(self, s: &S, k: &Kit) -> El {
        let st = (self.get)(s);
        let look = Look::new(k, self.fills, self.selection);
        let wrap = st.wraps();
        let key = self.key.clone();
        if !wrap && let Some(r) = rune::memo::layout(&format!("{key}/scroll")) {
            st.set_viewport(r);
        }
        let v = Rc::new(View { look, d: self });
        let count = st.rows.len();
        let mut list = col()
            .key("rows")
            .w_full()
            .growth(GrowthSpec::default())
            .selection_group(true)
            .role("list")
            .label("Diff");
        if !wrap {
            list = list.min_w(v.content_width(st).max(st.view_width()));
        }
        let build = v.clone();
        let vis_mut = v.d.get_mut.clone();
        let on_file = v.d.on_visible_file.clone();
        let sel_mut = v.d.get_mut.clone();
        let on_span = v.d.on_span_select.clone();
        let list = list
            .virtual_rows(count, LINE_H, keys(v.d.get.clone()), move |s: &S, i| build.row(s, i))
            .virtual_rows_version(st.keys_rev)
            .on_visible_rows(move |s: &mut S, rows| {
                let (reported, sticky_changed) = vis_mut(s).set_visible_rows(rows);
                if let (Some(f), Some(cb)) = (reported, &on_file) {
                    cb(s, f);
                    return true;
                }
                sticky_changed
            })
            .on_selection_change(move |s: &mut S, change, tx| {
                if change.dragging() {
                    tx.live_only();
                    return;
                }
                let st = sel_mut(s);
                let span = change.selection().and_then(|sel| st.span_from_texts(sel.texts(), sel.text()));
                if st.span_selection() == span.as_ref() {
                    tx.live_only();
                    return;
                }
                st.set_span_selection(span.clone());
                match &on_span {
                    Some(cb) => cb(s, span, tx),
                    None => tx.live_only(),
                }
            });
        let scroll = col()
            .key("scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec { stick_bottom: false });
        let scroll = if wrap { scroll } else { scroll.scroll_xy() };
        let (live, f) = (v.d.scroll_live.clone(), v.d.on_scroll.clone());
        let (at_cell, shift) = (st.scroll_cell(), st.spot_shift.clone());
        let scroll = scroll.on_scroll_offset(move |at| {
            at_cell.set([at.offset[1], at.viewport[1]]);
            shift.set([0.0, -at.offset[1]]);
            if let Some(live) = &live {
                live.set([at.offset[1], at.max[1], at.viewport[1], at.drawn[1]]);
            }
            if let Some(f) = &f {
                f(at);
            }
        });
        let scroll = scroll.child(list);
        let mut root = col().key(key.clone()).w_full().grow(1.0).min_h(0.0).fill(v.look.bg).child(scroll);
        if st.min_split_columns() > 0 {
            let digits = (0..st.files().len()).map(|f| st.digits(f)).max().unwrap_or(3);
            let (adv, gutter) = (v.look.adv, BAR + v.look.number_w(digits) + CODE_PAD.0 + CODE_PAD.1);
            let fit = v.d.get_mut.clone();
            root = root.on_layout_tx(move |s: &mut S, r, tx| {
                let cols = (((r[2] - 1.0) / 2.0 - gutter) / adv).max(0.0) as u32;
                if !fit(s).set_columns_per_side(cols) {
                    tx.live_only();
                }
            });
        }
        if let Some(spot) = v.spot(st, &key) {
            root = root.child(spot);
        }
        if let Some(f) = st.sticky() {
            let header = v.file_header(s, st, f, true);
            root = root.child(header.key("sticky").absolute(0.0, 0.0).w_full());
        }
        root
    }
}

/// The row-key closure for [`El::virtual_rows`]: keys are formatted from the row on demand.
fn keys<S: 'static>(get: Get<S>) -> impl for<'a> Fn(&'a S, usize) -> Cow<'a, str> + 'static {
    move |s, i| {
        let st = get(s);
        Cow::Owned(st.key_of(st.rows[i]))
    }
}

/// `t` of the way from `base` toward `hue` in OKLab, at `base`'s alpha (rune-kit's `palette::tint`).
fn tint(base: Color, hue: Color, t: f32) -> Color {
    let [a, b] = [base.to_oklab(), hue.to_oklab()];
    Color::from_oklab([0, 1, 2, 3].map(|i| if i == 3 { a[3] } else { a[i] + (b[i] - a[i]) * t }))
}

/// `t` of the way from `a` to `b` in sRGB: `b` at alpha `t` painted over `a`.
fn srgb_mix(a: Color, b: Color, t: f32) -> Color {
    use rune::color::{srgb_decode, srgb_encode};
    let m = |x: f32, y: f32| srgb_decode(srgb_encode(x) + (srgb_encode(y) - srgb_encode(x)) * t);
    Color {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: a.a,
    }
}

/// Colours and type, from the kit's palette so themes restyle them.
struct Look {
    font: TextStyle,
    label: TextStyle,
    small: TextStyle,
    /// The advance of a mono column.
    adv: f32,
    bg: Color,
    text: Color,
    muted: Color,
    faint: Color,
    border: Color,
    header_bg: Color,
    hover: Color,
    add: Tints,
    del: Tints,
    gap_bg: Color,
    empty_bg: Color,
    selected: Color,
    selected_gutter: Color,
    selection: Color,
    accent: Color,
    on_accent: Color,
    range_bar: Color,
    span_mark: Color,
    triangle_down: &'static Icon,
    triangle_right: &'static Icon,
    syntax: SyntaxTheme,
    /// Changes when anything above does, for memoised rows.
    version: u64,
}

#[derive(Clone, Copy)]
struct Tints {
    line: Color,
    gutter: Color,
    word: Color,
    number: Color,
    bar: Color,
    count: Color,
}

impl Look {
    fn new(k: &Kit, fills: Option<ChangeFills>, selection: Option<Color>) -> Look {
        let c = &k.c;
        let bg = c.bg_base;
        let dark = k.dark;
        let tints = |hue: Color, fg: Color, fill: Option<Color>| {
            let (line, gutter, word) = match fill {
                Some(line) => (line, srgb_mix(line, hue, 0.12), srgb_mix(line, hue, 0.25)),
                None => (
                    tint(bg, hue, if dark { 0.13 } else { 0.1 }),
                    tint(bg, hue, if dark { 0.2 } else { 0.17 }),
                    tint(bg, hue, if dark { 0.32 } else { 0.27 }),
                ),
            };
            Tints {
                line,
                gutter,
                word,
                number: tint(c.text_muted, hue, 0.7),
                bar: hue,
                count: fg,
            }
        };
        let font = TextStyle {
            size: 13.0,
            line_height: LINE_H,
            ..k.ty.mono
        };
        let info = c.info.icon;
        let warn = c.warning.icon;
        let mut look = Look {
            font,
            label: TextStyle {
                size: 13.0,
                line_height: 20.0,
                ..k.ty.ui_medium
            },
            small: TextStyle {
                size: 12.0,
                line_height: 16.0,
                ..k.ty.mono
            },
            adv: 0.6 * font.size,
            bg,
            text: c.text_base,
            muted: c.text_muted,
            faint: c.text_faint,
            border: c.border_weak,
            header_bg: c.layer_01,
            hover: k.alpha(c.text_base, if dark { 0.06 } else { 0.035 }),
            add: tints(c.success.icon, c.success.fg, fills.map(|f| f.add)),
            del: tints(c.danger.icon, c.danger.fg, fills.map(|f| f.del)),
            gap_bg: tint(bg, info, if dark { 0.1 } else { 0.06 }),
            empty_bg: tint(bg, c.text_base, if dark { 0.03 } else { 0.025 }),
            selected: tint(bg, info, if dark { 0.2 } else { 0.14 }),
            selected_gutter: tint(bg, info, if dark { 0.32 } else { 0.26 }),
            selection: selection.unwrap_or_else(|| k.selection()),
            accent: info,
            on_accent: c.white,
            range_bar: warn,
            span_mark: k.alpha(warn, if dark { 0.3 } else { 0.25 }),
            triangle_down: k.glyph.triangle_down,
            triangle_right: k.glyph.triangle_right,
            syntax: SyntaxTheme::from_kit(k),
            version: 0,
        };
        let mut h = std::hash::DefaultHasher::new();
        for c in [
            look.bg,
            look.text,
            look.add.bar,
            look.del.bar,
            look.add.line,
            look.del.line,
            look.selection,
            look.accent,
            look.header_bg,
            look.range_bar,
        ] {
            [c.r, c.g, c.b, c.a].map(f32::to_bits).hash(&mut h);
        }
        look.font.size.to_bits().hash(&mut h);
        look.version = h.finish();
        look
    }

    fn tints(&self, kind: LineKind) -> Option<&Tints> {
        match kind {
            LineKind::Added => Some(&self.add),
            LineKind::Removed => Some(&self.del),
            LineKind::Context => None,
        }
    }

    fn gutter_fill(&self, kind: LineKind, selected: bool, calm: bool) -> Color {
        match (selected, self.tints(kind)) {
            (true, _) => self.selected_gutter,
            (false, Some(t)) if !calm => t.gutter,
            _ => self.bg,
        }
    }

    fn line_fill(&self, kind: LineKind, selected: bool, calm: bool) -> Color {
        match (selected, self.tints(kind)) {
            (true, _) => self.selected,
            (false, Some(t)) if !calm => t.line,
            _ => self.bg,
        }
    }

    /// One line-number column for numbers of `digits` digits.
    fn number_w(&self, digits: u32) -> f32 {
        (digits as f32 * self.adv + 16.0).ceil()
    }
}

/// One side's line in a row: what a cell draws.
#[derive(Clone)]
struct Cell<'a> {
    kind: LineKind,
    old: Option<u32>,
    new: Option<u32>,
    text: &'a str,
    changes: Vec<Range<usize>>,
    no_newline: bool,
}

impl Cell<'_> {
    /// The side its text is read from, and its number there.
    fn source(&self) -> (Side, u32) {
        match (self.kind, self.old, self.new) {
            (LineKind::Removed, Some(n), _) => (Side::Old, n),
            (_, _, Some(n)) => (Side::New, n),
            (_, Some(n), None) => (Side::Old, n),
            _ => (Side::New, 1),
        }
    }

    fn label(&self) -> String {
        let (_, n) = self.source();
        let kind = match self.kind {
            LineKind::Added => "added",
            LineKind::Removed => "removed",
            LineKind::Context => "unchanged",
        };
        format!("{kind} line {n}: {}", self.text)
    }
}

/// What a row draws beyond its lines, for the memo version: the cursor, comment ranges over
/// its lines and span marks.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Marks {
    cursor: bool,
    range: [bool; 2],
    spans: u64,
}

struct View<S> {
    d: DiffView<S>,
    look: Look,
}

impl<S: 'static> View<S> {
    /// How wide unwrapped (unified) content is, so the list doesn't change width as rows are built.
    fn content_width(&self, st: &DiffState) -> f32 {
        let l = &self.look;
        let digits = (0..st.files().len()).map(|f| st.digits(f)).max().unwrap_or(3);
        BAR + 2.0 * l.number_w(digits) + st.columns() as f32 * l.adv + CODE_PAD.0 + CODE_PAD.1
    }

    /// The most a full-width part of a row (a header, an annotation) spans: the viewport, not
    /// the unwrapped content.
    fn view_w(&self, st: &DiffState, split_half: bool) -> Option<f32> {
        let w = st.view_width();
        (w > 0.0 && !st.wraps()).then_some(if split_half { (w / 2.0).floor() } else { w })
    }

    fn calm(&self, st: &DiffState, f: usize) -> bool {
        let file = &st.files()[f];
        self.d.calm && file.additions >= CALM_LINES && file.deletions == 0 && file.lines.len() == file.additions as usize
    }

    fn marks(&self, st: &DiffState, i: usize, r: Row) -> Marks {
        let f = r.file();
        let mut m = Marks {
            cursor: st.cursor_row() == Some(i as u32),
            ..Marks::default()
        };
        if st.annotations().is_empty() {
            return m;
        }
        let mut h = std::hash::DefaultHasher::new();
        for side in [Side::Old, Side::New] {
            if let Some(n) = st.row_line(r, side) {
                let at = LineRef::new(side, n);
                m.range[side as usize] = st.covering(f, at).next().is_some();
                if m.range[side as usize] {
                    st.span_marks(f, at).hash(&mut h);
                }
            }
        }
        m.spans = h.finish();
        m
    }

    fn row(&self, s: &S, i: usize) -> El {
        let st = (self.d.get)(s);
        let r = st.rows[i];
        let key = st.key_of(r);
        let dim = st.dimmed(i);
        let lit = |e: El| if dim { e.opacity(DIM) } else { e };
        match r {
            Row::File(f) => lit(self.file_row(s, st, f as usize)),
            Row::Note(f) => memo(&key, (self.look.version, st.rev, st.view_width().to_bits(), dim), || {
                lit(self.note(st, f as usize))
            }),
            Row::Gap { f, gap } => memo(&key, (self.look.version, st.rev, st.view_width().to_bits(), dim), || {
                lit(self.gap(st, f as usize, gap as usize))
            }),
            _ if !st.annotations().is_empty() && self.d.annotation.is_some() && self.annotated(st, r) => lit(self.line(s, st, i, &key)),
            _ => {
                let version = (
                    self.look.version,
                    st.rev,
                    st.row_selected(i, None),
                    st.copy_side(),
                    st.selection().map(|s| s.start.side),
                    self.marks(st, i, r),
                    self.d.calm,
                    dim,
                );
                memo(&key, version, || lit(self.line(s, st, i, &key)))
            }
        }
    }

    fn annotated(&self, st: &DiffState, r: Row) -> bool {
        let f = r.file();
        [Side::Old, Side::New].into_iter().any(|side| {
            st.row_line(r, side)
                .is_some_and(|n| st.annotations_at(f, LineRef::new(side, n)).next().is_some())
        })
    }

    fn cells<'a>(&self, st: &'a DiffState, r: Row) -> (Option<Cell<'a>>, Option<Cell<'a>>) {
        let of = |f: u32, i: u32| {
            let file = &st.files()[f as usize];
            let l = &file.lines[i as usize];
            let changes = if l.kind == LineKind::Context {
                Vec::new()
            } else {
                st.word_changes(f as usize, i)
            };
            Cell {
                kind: l.kind,
                old: (l.old != 0).then_some(l.old),
                new: (l.new != 0).then_some(l.new),
                text: file.line_text(i),
                changes,
                no_newline: l.no_newline,
            }
        };
        match r {
            Row::Ctx { f, old, new } => {
                let text = st
                    .side_line(f as usize, Side::New, new)
                    .or_else(|| st.side_line(f as usize, Side::Old, old))
                    .unwrap_or("");
                let c = Cell {
                    kind: LineKind::Context,
                    old: Some(old),
                    new: Some(new),
                    text,
                    changes: Vec::new(),
                    no_newline: false,
                };
                (Some(c), None)
            }
            Row::Line { f, i } => (Some(of(f, i)), None),
            Row::Pair { f, del, add } => (Some(of(f, del)), Some(of(f, add))),
            _ => (None, None),
        }
    }

    fn line(&self, s: &S, st: &DiffState, i: usize, key: &str) -> El {
        let r = st.rows[i];
        let f = r.file();
        let (a, b) = self.cells(st, r);
        let Some(a) = a else { return col() };
        let marks = self.marks(st, i, r);
        let draw = st.draw(f);
        let line = match draw {
            Draw::Unified => self.unified(st, i, f, a, marks, key, None),
            Draw::One(side) => self.unified(st, i, f, a, marks, key, Some(side)),
            Draw::Split => {
                let (old, new) = match (b, a.kind) {
                    (Some(b), _) => (Some(a), Some(b)),
                    (None, LineKind::Removed) => (Some(a), None),
                    (None, LineKind::Added) => (None, Some(a)),
                    (None, LineKind::Context) => (Some(a.clone()), Some(a)),
                };
                self.split(st, i, f, old, new, marks, key)
            }
        };
        let Some(build) = self.d.annotation.as_ref() else { return line };
        let notes = |side: Side| -> Vec<El> {
            st.row_line(r, side).map_or(Vec::new(), |n| {
                st.annotations_at(f, LineRef::new(side, n)).map(|a| note(build(s, a), a)).collect()
            })
        };
        let has = |side: Side| {
            st.row_line(r, side)
                .is_some_and(|n| st.annotations_at(f, LineRef::new(side, n)).next().is_some())
        };
        if !has(Side::Old) && !has(Side::New) {
            return line;
        }
        let mut out = col().w_full().child(line.key("line"));
        match draw {
            Draw::Unified | Draw::One(_) => {
                let mut c = col().key("notes").w_full();
                if let Some(w) = self.view_w(st, false) {
                    c = c.w(w).sticky_left(0.0);
                }
                let mut seen = std::collections::HashSet::new();
                let all = st
                    .row_line(r, Side::Old)
                    .into_iter()
                    .flat_map(|n| st.annotations_at(f, LineRef::new(Side::Old, n)))
                    .chain(
                        st.row_line(r, Side::New)
                            .into_iter()
                            .flat_map(|n| st.annotations_at(f, LineRef::new(Side::New, n))),
                    )
                    .filter(|a| seen.insert(a.id.clone()));
                out = out.child(c.children(all.map(|a| note(build(s, a), a))));
            }
            Draw::Split => {
                let half = |key: &'static str, els: Vec<El>| {
                    let mut c = col().key(key).w_frac(0.5).min_w(0.0);
                    if let Some(w) = self.view_w(st, true) {
                        c = c.max_w(w);
                    }
                    c.children(els)
                };
                out = out.child(
                    row()
                        .key("notes")
                        .w_full()
                        .items_start()
                        .child(half("old", notes(Side::Old)))
                        .child(half("new", notes(Side::New))),
                );
            }
        }
        out
    }

    /// A line in one column: both numbers, or with `one` that side's only (a one-sided file in
    /// split mode, across the whole width).
    #[allow(clippy::too_many_arguments)]
    fn unified(&self, st: &DiffState, i: usize, f: usize, c: Cell<'_>, marks: Marks, key: &str, one: Option<Side>) -> El {
        let l = &self.look;
        let selected = st.row_selected(i, None);
        let calm = self.calm(st, f);
        let fill = l.line_fill(c.kind, selected, calm);
        let digits = st.digits(f);
        let copy = match c.kind {
            LineKind::Context => true,
            LineKind::Added => st.copy_side() == Side::New,
            LineKind::Removed => st.copy_side() == Side::Old,
        };
        let hover_side = if c.kind == LineKind::Removed { Side::Old } else { Side::New };
        // The marker, numbers and "+" hold to the viewport's left edge while the code scrolls
        // sideways under them, on the gutter's own opaque fill.
        let mut gutter = row()
            .key("gutter")
            .self_stretch()
            .items_stretch()
            .fill(l.gutter_fill(c.kind, selected, calm))
            .child(self.bar(c.kind, marks.cursor));
        let numbers = match one {
            Some(side) => {
                let n = if side == Side::Old { c.old } else { c.new };
                gutter = gutter.child(self.number(st, i, f, n, side, c.kind, selected, digits, calm).key("n"));
                1.0
            }
            None => {
                gutter = gutter
                    .child(self.number(st, i, f, c.old, Side::Old, c.kind, selected, digits, calm).key("no"))
                    .child(self.number(st, i, f, c.new, Side::New, c.kind, selected, digits, calm).key("nn"));
                2.0
            }
        };
        if marks.range[0] || marks.range[1] {
            gutter = gutter.child(self.range_bar().absolute(BAR + numbers * l.number_w(digits) - RANGE_BAR, 0.0));
        }
        if self.d.on_comment.is_some() {
            let at = c
                .new
                .map(|n| LineRef::new(Side::New, n))
                .or(c.old.map(|n| LineRef::new(Side::Old, n)));
            if let Some(at) = at {
                gutter = gutter.child(self.plus(f, at).absolute(BAR + numbers * l.number_w(digits) - 10.0, 2.0));
            }
        }
        if !st.wraps() {
            gutter = gutter.sticky_left(0.0);
        }
        let label = c.label();
        row()
            .w_full()
            .min_h(LINE_H)
            .items_start()
            .fill(fill)
            .hover(l.hover)
            .hover_group()
            .role("listitem")
            .label(label)
            .child(gutter)
            .child(self.code(st, f, &c, copy, hover_side, key).key("code"))
    }

    #[allow(clippy::too_many_arguments)]
    fn split<'a>(
        &self,
        st: &'a DiffState,
        i: usize,
        f: usize,
        old: Option<Cell<'a>>,
        new: Option<Cell<'a>>,
        marks: Marks,
        key: &str,
    ) -> El {
        let digits = st.digits(f);
        let calm = self.calm(st, f);
        let half = |side: Side, c: Option<Cell<'a>>| -> El {
            let l = &self.look;
            let hk = if side == Side::Old { "old" } else { "new" };
            let mut h = row().key(hk).w_frac(0.5).min_w(0.0).min_h(LINE_H).items_start().self_stretch();
            let Some(c) = c else {
                return h.fill(l.empty_bg);
            };
            let kind = c.kind;
            let selected = st.row_selected(i, Some(side));
            let fill = l.line_fill(kind, selected, calm);
            let n = if side == Side::Old { c.old } else { c.new };
            let mut cell = c;
            if kind == LineKind::Context {
                cell.text = st.side_line(f, side, n.unwrap_or(1)).unwrap_or(cell.text);
            }
            let copy = st.copy_side() == side;
            let label = cell.label();
            h = h
                .fill(fill)
                .hover(l.hover)
                .hover_group()
                .role("listitem")
                .label(label)
                .child(self.bar(kind, marks.cursor && side == Side::Old))
                .child(self.number(st, i, f, n, side, kind, selected, digits, calm).key("n"))
                .child(self.code(st, f, &cell, copy, side, key).key("code"));
            if marks.range[side as usize] {
                h = h.child(self.range_bar().absolute(BAR + l.number_w(digits) - RANGE_BAR, 0.0));
            }
            if let (Some(n), true) = (n, self.d.on_comment.is_some()) {
                h = h.child(self.plus(f, LineRef::new(side, n)).absolute(BAR + l.number_w(digits) - 10.0, 2.0));
            }
            h
        };
        let divider = col().key("divider").w(1.0).self_stretch().fill(self.look.border);
        row()
            .w_full()
            .min_h(LINE_H)
            .items_stretch()
            .child(half(Side::Old, old))
            .child(divider)
            .child(half(Side::New, new))
    }

    fn bar(&self, kind: LineKind, cursor: bool) -> El {
        let b = col().key("bar").w(BAR).self_stretch().a11y_hidden();
        if cursor {
            return b.fill(self.look.accent);
        }
        match self.look.tints(kind) {
            Some(t) => b.fill(t.bar),
            None => b,
        }
    }

    /// The bar beside the numbers of lines a range or span comment covers.
    fn range_bar(&self) -> El {
        col().key("range").w(RANGE_BAR).h_full().fill(self.look.range_bar).a11y_hidden()
    }

    /// A line-number cell: pressing it selects the line, shift-pressing extends the selection to
    /// it, and dragging down the numbers selects the lines passed.
    #[allow(clippy::too_many_arguments)]
    fn number(
        &self,
        st: &DiffState,
        i: usize,
        f: usize,
        n: Option<u32>,
        side: Side,
        kind: LineKind,
        selected: bool,
        digits: u32,
        calm: bool,
    ) -> El {
        let l = &self.look;
        let unified = st.draw(f) != Draw::Split;
        let mut c = row()
            .w(l.number_w(digits))
            .min_h(LINE_H)
            .self_stretch()
            .px(8.0)
            .justify_end()
            .items_start()
            .a11y_hidden();
        if !unified {
            c = c.fill(l.gutter_fill(kind, selected, calm));
        }
        if let Some(n) = n {
            let color = if selected {
                l.text
            } else {
                l.tints(kind).map_or(l.faint, |t| t.number)
            };
            c = c.child(text(n.to_string()).font(l.font).color(color));
        }
        let at = n.map(|n| LineRef::new(side, n)).or_else(|| {
            let other = if side == Side::Old { Side::New } else { Side::Old };
            unified
                .then(|| st.row_line(st.rows[i], other).map(|n| LineRef::new(other, n)))
                .flatten()
        });
        let Some(at) = at else { return c };
        let (get, on_select) = (self.d.get_mut.clone(), self.d.on_select.clone());
        let row_at = i as u32;
        c.cursor(Cursor::Pointer).on_drag(move |s: &mut S, d: &mut Drag, tx| {
            let st = get(s);
            match d.phase {
                Phase::Down => st.press_line(f, at),
                Phase::Move => {
                    let Some(r) = st.row_from(row_at, d.y) else { return };
                    let r = st.rows[r as usize];
                    if r.file() != f {
                        return;
                    }
                    let to = st.row_line(r, side).map(|n| LineRef::new(side, n)).or_else(|| {
                        let other = if side == Side::Old { Side::New } else { Side::Old };
                        unified.then(|| st.row_line(r, other).map(|n| LineRef::new(other, n))).flatten()
                    });
                    if let Some(to) = to {
                        st.drag_to(f, to);
                    }
                }
                Phase::Up => {
                    if d.cancelled() {
                        st.cancel_line();
                        return;
                    }
                    let sel = st.release_line();
                    if let Some(cb) = &on_select {
                        cb(s, sel, tx);
                    }
                }
            }
        })
    }

    /// The "+" that appears by the numbers of the hovered line.
    fn plus(&self, f: usize, at: LineRef) -> El {
        let l = &self.look;
        let (get, cb) = (self.d.get_mut.clone(), self.d.on_comment.clone());
        row()
            .key("plus")
            .size(16.0, 16.0)
            .center()
            .radius(4.0)
            .fill(l.accent)
            .group_opacity(0.0, 1.0)
            .cursor(Cursor::Pointer)
            .role("button")
            .label("Comment on this line")
            .child(icon(&PLUS, 12.0).color(l.on_accent))
            .on_press_tx(move |s: &mut S, tx| {
                let st = get(s);
                let lines = match st.selection() {
                    Some(sel) if sel.file == f && st.contains(&sel, at) => sel,
                    _ => LineRange::line(f, at),
                };
                if let Some(cb) = &cb {
                    cb(s, lines, tx);
                }
            })
    }

    /// The code of a line: syntax colours under word changes, tabs padded to four columns, span
    /// comments' text tinted without moving a glyph.
    fn code(&self, st: &DiffState, f: usize, c: &Cell<'_>, copy: bool, side: Side, row_key: &str) -> El {
        let l = &self.look;
        let (src, n) = c.source();
        let syntax: Vec<(Range<usize>, Color)> = st.highlights(f, src, n).into_iter().map(|(r, p)| (r, l.syntax.paint(p))).collect();
        let word = l.tints(c.kind).map(|t| t.word);
        let spans = layer(c.text.len(), &syntax, word.map_or(&[][..], |_| &c.changes[..]), word);
        let marks = st.span_marks(f, LineRef::new(src, n));
        let (shown, spans, pads, marks) = tabbed(c.text, spans, marks, st.wraps());
        let mut t = rich(shown, spans).font(l.font).color(l.text).key("text");
        t = if st.wraps() { t.w_full() } else { t.single_line() };
        if !marks.is_empty() {
            t = t.text_highlights(marks.into_iter().map(|r| TextHighlight::new(r, l.span_mark)));
        }
        if copy {
            t = t.selectable(true).selection_color(l.selection);
            t = pads.into_iter().fold(t, |t, r| t.uncopied(r));
        }
        let (get, cell) = (self.d.get_mut.clone(), format!("{row_key}.{}", side as u8));
        let mut code =
            col()
                .grow(1.0)
                .min_w(0.0)
                .pl(CODE_PAD.0)
                .pr(CODE_PAD.1)
                .child(t)
                .on_hover_change(move |s: &mut S, inside, tx: &mut Tx| {
                    if !get(s).hover_text(side, &cell, inside) {
                        tx.live_only();
                    }
                });
        if c.no_newline {
            code = code.child(
                text("No newline at end of file")
                    .font(l.small)
                    .color(l.muted)
                    .opacity(0.6)
                    .key("eof")
                    .pb(2.0),
            );
        }
        code
    }

    fn gutter_w(&self, st: &DiffState, f: usize) -> f32 {
        let n = self.look.number_w(st.digits(f));
        BAR + if st.draw(f) == Draw::Unified { 2.0 * n } else { n }
    }

    /// The spotlight's outline, over the list and riding its scroll, with its step and caption:
    /// one element keyed `spot`, so a new range slides it there. Placed from the rows' layout
    /// once they are built, from the row height estimates until then.
    fn spot(&self, st: &DiffState, key: &str) -> Option<El> {
        let spot = st.spotlight()?;
        let (a, b) = st.spotlight_rows()?;
        let l = &self.look;
        let at = |r: u32| rune::memo::layout(&format!("{key}/scroll/rows/{}", st.key_of(st.rows[r as usize])));
        let origin = rune::memo::layout(&format!("{key}/scroll/rows"));
        let (top, bottom) = match (origin, at(a), at(b)) {
            (Some(o), Some(t), Some(e)) => (t[1] - o[1], e[1] + e[3] - o[1]),
            _ => (st.row_y(a as usize), st.row_y(b as usize + 1)),
        };
        let mut outline = col()
            .key("spot")
            .pin(Some(SPOT_W), Some(top - SPOT_W), Some(SPOT_W), None)
            .h(bottom - top + 2.0 * SPOT_W)
            .radius(4.0)
            .border(SPOT_W, l.accent)
            .live_offset(&st.spot_shift)
            .items_end()
            .a11y_hidden();
        let caption = match (spot.step, spot.caption.as_deref()) {
            (Some((i, n)), Some(c)) => Some(format!("{i}/{n} · {c}")),
            (Some((i, n)), None) => Some(format!("{i}/{n}")),
            (None, Some(c)) => Some(c.to_owned()),
            (None, None) => None,
        };
        if let Some(c) = caption {
            outline = outline.child(
                row()
                    .key("caption")
                    .px(6.0)
                    .h(18.0)
                    .items_center()
                    .radius(4.0)
                    .fill(l.accent)
                    .offset(0.0, -18.0)
                    .child(text(c).font(l.small).color(l.on_accent).single_line()),
            );
        }
        Some(
            col()
                .key("spot-layer")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .clip_rounded(0.0)
                .child(outline),
        )
    }

    /// The unchanged lines between two hunks, folded: buttons to show more, and the next hunk's
    /// header.
    fn gap(&self, st: &DiffState, f: usize, gap: usize) -> El {
        let l = &self.look;
        let file = &st.files()[f];
        let hidden = st.hidden(f, gap).unwrap_or(0);
        let last = gap == file.hunks.len();
        let expandable = st.expandable(f);
        let mut buttons = Vec::new();
        if expandable {
            let mut add = |how: Expand, glyph: &'static Icon, label: String| {
                let get = self.d.get_mut.clone();
                buttons.push(
                    row()
                        .key(format!("{how:?}"))
                        .size(24.0, 24.0)
                        .center()
                        .radius(6.0)
                        .hover(l.hover)
                        .cursor(Cursor::Pointer)
                        .role("button")
                        .label(label)
                        .child(icon(glyph, 16.0).color(l.muted))
                        .on_press_tx(move |s: &mut S, _| get(s).expand(f, gap, how)),
                );
            };
            let step = hidden.min(EXPAND_STEP);
            if hidden <= EXPAND_STEP {
                add(Expand::All, &EXPAND_ALL, format!("Show {hidden} hidden lines"));
            } else {
                if gap > 0 {
                    add(Expand::Down, &EXPAND_DOWN, format!("Show {step} more lines below"));
                }
                if !last {
                    add(Expand::Up, &EXPAND_UP, format!("Show {step} more lines above"));
                }
            }
        } else if let Some(need) = self
            .d
            .on_need_texts
            .clone()
            .filter(|_| !file.binary && st.hidden(f, gap) != Some(0))
        {
            let get = self.d.get_mut.clone();
            let how = if gap == 0 { Expand::Up } else { Expand::Down };
            buttons.push(
                row()
                    .key("load")
                    .size(24.0, 24.0)
                    .center()
                    .radius(6.0)
                    .hover(l.hover)
                    .cursor(Cursor::Pointer)
                    .role("button")
                    .label("Load the whole file to show unchanged lines")
                    .child(icon(if gap == 0 { &EXPAND_UP } else { &EXPAND_DOWN }, 16.0).color(l.muted))
                    .on_press_tx(move |s: &mut S, _| {
                        get(s).request_expand(f, gap, how);
                        need(s, f);
                    }),
            );
        }
        let buttons = row()
            .key("expand")
            .w(self.gutter_w(st, f))
            .h(GAP_H)
            .center()
            .gap(2.0)
            .children(buttons);
        let mut inner = row().key("inner").h(GAP_H).items_center().gap(12.0).child(buttons);
        if let Some(w) = self.view_w(st, false) {
            inner = inner.max_w(w).sticky_left(0.0);
        }
        if hidden > 0 {
            let count = text(if hidden == 1 {
                "1 unchanged line".to_owned()
            } else {
                format!("{hidden} unchanged lines")
            })
            .font(l.small)
            .color(l.muted)
            .key("count");
            let count = if expandable && hidden > EXPAND_STEP {
                let get = self.d.get_mut.clone();
                row()
                    .key("all")
                    .px(6.0)
                    .h(24.0)
                    .items_center()
                    .radius(6.0)
                    .hover(l.hover)
                    .cursor(Cursor::Pointer)
                    .role("button")
                    .label(format!("Show all {hidden} lines"))
                    .on_press_tx(move |s: &mut S, _| get(s).expand(f, gap, Expand::All))
                    .child(count)
            } else {
                row().key("all").px(6.0).child(count)
            };
            inner = inner.child(count);
        }
        let header = file.hunks.get(gap).map(|h| model::hunk_header(file, h));
        if let Some(h) = &header {
            inner = inner.child(text(h.clone()).font(l.small).color(l.faint).single_line().key("header"));
        }
        let label = match &header {
            Some(h) => format!("{hidden} unchanged lines, then {h}"),
            None => format!("{hidden} unchanged lines"),
        };
        row()
            .w_full()
            .h(GAP_H)
            .items_center()
            .fill(l.gap_bg)
            .role("group")
            .label(label)
            .child(inner)
    }

    /// A file with no lines to show.
    fn note(&self, st: &DiffState, f: usize) -> El {
        let l = &self.look;
        let file = &st.files()[f];
        let msg = if file.binary {
            "Binary file not shown"
        } else if file.contents_unavailable {
            "Contents unavailable"
        } else if file.status == FileStatus::Renamed || file.status == FileStatus::Copied {
            "File moved without changes"
        } else if file.status == FileStatus::Added || file.status == FileStatus::Deleted {
            "Empty file"
        } else {
            "No changes"
        };
        let mut inner = row()
            .key("inner")
            .h(NOTE_H)
            .items_center()
            .pl(self.gutter_w(st, f) + CODE_PAD.0)
            .child(text(msg).font(l.small).color(l.muted));
        if let Some(w) = self.view_w(st, false) {
            inner = inner.max_w(w).sticky_left(0.0);
        }
        row().w_full().h(NOTE_H).fill(l.bg).role("note").label(msg).child(inner)
    }

    /// A file's row: its header, and the file-level widget under it when the app gives one.
    fn file_row(&self, s: &S, st: &DiffState, f: usize) -> El {
        let header = self.file_header(s, st, f, false);
        let Some(slot) = self.d.header_annotation.as_ref().and_then(|b| b(s, f)) else {
            return header;
        };
        let mut c = col().key("file-note").w_full();
        if let Some(w) = self.view_w(st, false) {
            c = c.w(w).sticky_left(0.0);
        }
        col().w_full().child(header.key("header")).child(c.child(slot))
    }

    fn file_header(&self, s: &S, st: &DiffState, f: usize, sticky: bool) -> El {
        let l = &self.look;
        let file = &st.files()[f];
        let collapsed = st.collapsed(f);
        let get = self.d.get_mut.clone();
        let toggle = row()
            .key("toggle")
            .size(24.0, 24.0)
            .center()
            .radius(6.0)
            .hover(l.hover)
            .cursor(Cursor::Pointer)
            .role("button")
            .label(if collapsed { "Expand file" } else { "Collapse file" })
            .expanded(!collapsed)
            .child(icon(if collapsed { l.triangle_right } else { l.triangle_down }, 16.0).color(l.muted))
            .on_press_tx(move |s: &mut S, tx| {
                let st = get(s);
                st.set_collapsed(f, !st.collapsed(f));
                if sticky {
                    st.scroll_to_file(f, tx);
                }
            });
        let path = |p: String, c: Color| text(p).font(l.label).color(c).truncate_head().min_w(0.0).shrink();
        let shown = st.display_path(f);
        let mut name = row().key("path").items_center().gap(6.0).min_w(0.0).shrink();
        name = match (file.status, file.old_path.as_deref()) {
            (FileStatus::Renamed | FileStatus::Copied, Some(a)) => name
                .child(path(a.to_owned(), l.muted).key("old"))
                .child(text("→").font(l.label).color(l.faint).key("arrow"))
                .child(path(shown.clone(), l.text).key("new")),
            _ => name.child(path(shown.clone(), l.text).key("new")),
        };
        let status = match file.status {
            FileStatus::Added => Some(("Added", l.add.count)),
            FileStatus::Deleted => Some(("Deleted", l.del.count)),
            FileStatus::Renamed => Some(("Renamed", l.muted)),
            FileStatus::Copied => Some(("Copied", l.muted)),
            FileStatus::Modified => None,
        };
        let mut inner = row()
            .key("inner")
            .w_full()
            .grow(1.0)
            .px(10.0)
            .gap(8.0)
            .items_center()
            .child(toggle)
            .child(name);
        if let Some((word, color)) = status {
            inner = inner.child(text(word).font(l.small).color(color).key("status"));
        }
        if file.generated {
            inner = inner.child(text("Generated").font(l.small).color(l.faint).key("generated"));
        }
        inner = inner.child(row().key("space").grow(1.0).a11y_hidden());
        let (adds, dels) = (file.additions, file.deletions);
        if adds > 0 {
            inner = inner.child(text(format!("+{adds}")).font(l.small).color(l.add.count).key("adds"));
        }
        if dels > 0 {
            inner = inner.child(text(format!("−{dels}")).font(l.small).color(l.del.count).key("dels"));
        }
        if let Some(h) = &self.d.header {
            inner = inner.child(h(s, f).key("actions"));
        }
        let mut label = shown;
        if let (FileStatus::Renamed, Some(a)) = (file.status, file.old_path.as_deref()) {
            label.push_str(&format!(", renamed from {a}"));
        }
        label.push_str(&format!(", {adds} added, {dels} removed"));
        let line = |key: &'static str, c: Color| col().key(key).w_full().h(1.0).fill(c).a11y_hidden();
        let mut h = col().w_full().h(HEADER_H).fill(l.header_bg).role("heading").label(label);
        // In the list, as wide as the view and held to its left edge while the code scrolls
        // sideways, so the counts and actions stay in sight.
        if let (false, Some(w)) = (sticky, self.view_w(st, false)) {
            h = h.w(w).sticky_left(0.0);
        }
        h = h.child(line("top", if f > 0 && !sticky { l.border } else { l.header_bg }));
        h.child(inner).child(line("bottom", l.border))
    }
}

/// An annotation's widget, keyed by its id so it keeps its state as rows come and go.
fn note(el: El, a: &Annotation) -> El {
    col().key(format!("a.{}", a.id)).w_full().child(el)
}

/// A line as drawn: text, spans, inserted (uncopied) ranges and span marks, from `tabbed`.
type Drawn = (String, Vec<Span>, Vec<Range<usize>>, Vec<Range<usize>>);

/// Spans for a line from its syntax colours and the byte ranges given `bg` (word changes): cut
/// where either starts or ends, so each piece has one colour and one background.
fn layer(len: usize, syntax: &[(Range<usize>, Color)], words: &[Range<usize>], bg: Option<Color>) -> Vec<Span> {
    let mut cuts: Vec<usize> = syntax
        .iter()
        .flat_map(|(r, _)| [r.start, r.end])
        .chain(words.iter().flat_map(|r| [r.start, r.end]))
        .collect();
    cuts.retain(|&c| c <= len);
    cuts.sort_unstable();
    cuts.dedup();
    let mut out: Vec<Span> = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let color = syntax
            .get(syntax.partition_point(|(r, _)| r.end <= a))
            .filter(|(r, _)| r.start <= a)
            .map(|(_, c)| *c);
        let on = words.get(words.partition_point(|r| r.end <= a)).is_some_and(|r| r.start <= a);
        let bg = bg.filter(|_| on);
        if color.is_none() && bg.is_none() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.end as usize == a && last.color == color && last.bg == bg => last.end = b as u32,
            _ => {
                let mut sp = Span::new(a, b);
                sp.color = color;
                sp.bg = bg;
                out.push(sp);
            }
        }
    }
    out
}

/// `code` as drawn: text layout draws a tab as one space, so each tab is followed by spaces up
/// to the next four-column stop; and when `wrap`, a zero-width space after `.`, `(`, `[` and `/`
/// lets a long call chain or path break there. Returns the text, `spans` and `marks` moved to
/// match, and the inserted ranges (drawn, never copied). `state::shown_to_col` inverts it.
fn tabbed(code: &str, mut spans: Vec<Span>, mut marks: Vec<Range<usize>>, wrap: bool) -> Drawn {
    if !(code.contains('\t') || wrap && code.contains(['.', '(', '[', '/'])) {
        return (code.to_owned(), spans, Vec::new(), marks);
    }
    let mut shown = String::with_capacity(code.len() + 8);
    // (byte in `code` after the insertion, bytes added up to and including it)
    let mut shifts: Vec<(usize, usize)> = Vec::new();
    let mut pads = Vec::new();
    let mut col = 0;
    let mut chars = code.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        shown.push(ch);
        col += 1;
        let start = shown.len();
        if ch == '\t' {
            let pad = 3 - (col - 1) % 4;
            shown.extend(std::iter::repeat_n(' ', pad));
            col += pad;
        } else if wrap && matches!(ch, '.' | '(' | '[' | '/') && chars.peek().is_some_and(|(_, c)| !c.is_whitespace()) {
            shown.push('\u{200B}');
        }
        if shown.len() > start {
            pads.push(start..shown.len());
            shifts.push((i + ch.len_utf8(), shifts.last().map_or(0, |s| s.1) + shown.len() - start));
        }
    }
    let shift = |b: usize| match shifts.partition_point(|s| s.0 <= b) {
        0 => b,
        n => b + shifts[n - 1].1,
    };
    for sp in &mut spans {
        (sp.start, sp.end) = (shift(sp.start as usize) as u32, shift(sp.end as usize) as u32);
    }
    for m in &mut marks {
        *m = shift(m.start)..shift(m.end);
    }
    (shown, spans, pads, marks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::shown_to_col;

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn word_backgrounds_cut_syntax_colours() {
        let red = Color::WHITE;
        let green = Color::BLACK;
        let spans = layer(14, &[(0..3, red), (12..13, red)], &[4..9], Some(green));
        let got: Vec<(u32, u32, Option<Color>, Option<Color>)> = spans.iter().map(|s| (s.start, s.end, s.color, s.bg)).collect();
        assert_eq!(
            got,
            vec![(0, 3, Some(red), None), (4, 9, None, Some(green)), (12, 13, Some(red), None)]
        );
        let spans = layer(10, &[(0..6, red)], &[3..8], Some(green));
        let got: Vec<(u32, u32, Option<Color>, Option<Color>)> = spans.iter().map(|s| (s.start, s.end, s.color, s.bg)).collect();
        assert_eq!(
            got,
            vec![(0, 3, Some(red), None), (3, 6, Some(red), Some(green)), (6, 8, None, Some(green))]
        );
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn tabs_pad_to_four_columns_without_copying_the_padding() {
        let (shown, spans, pads, marks) = tabbed("a\tb\t\tc", vec![Span::new(2, 3)], vec![2..6], false);
        assert_eq!(shown, "a\t  b\t  \t   c");
        assert_eq!(pads, vec![2..4, 6..8, 9..12]);
        assert_eq!(&shown[spans[0].start as usize..spans[0].end as usize], "b");
        assert_eq!(&shown[marks[0].clone()], "b\t  \t   c");
        let copied: String = shown
            .char_indices()
            .filter(|(i, _)| !pads.iter().any(|r| r.contains(i)))
            .map(|(_, c)| c)
            .collect();
        assert_eq!(copied, "a\tb\t\tc");
        for (b, col) in [(0, 0), (2, 2), (4, 2), (5, 3)] {
            assert_eq!(shown_to_col("a\tb\t\tc", false, b), col, "byte {b}");
        }
    }

    #[test]
    fn wrapped_lines_can_break_inside_call_chains() {
        let (shown, spans, pads, _) = tabbed("a.b(c) x/ y", vec![Span::new(2, 3)], Vec::new(), true);
        assert_eq!(shown, "a.\u{200B}b(\u{200B}c) x/ y");
        assert_eq!(&shown[spans[0].start as usize..spans[0].end as usize], "b");
        let copied: String = shown
            .char_indices()
            .filter(|(i, _)| !pads.iter().any(|r| r.contains(i)))
            .map(|(_, c)| c)
            .collect();
        assert_eq!(copied, "a.b(c) x/ y");
        assert_eq!(shown_to_col("a.b(c) x/ y", true, 5), 2);
    }
}
