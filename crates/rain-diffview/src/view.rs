//! Drawing a [`DiffState`]: a virtual list of file headers, hunk separators and lines, a sticky
//! header for the file being read, and the handlers that feed selection, expansion, text spans
//! and the reading position back into the state.
//!
//! A line reads left to right: a marks column (folded threads and notes, and the hover's
//! comment and ask buttons), the old and new numbers, the `+`/`−` sign, then the code.

use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use rain_core::diff::{FileStatus, LineKind, Side};
use rune::{
    Color, Cursor, Drag, El, GrowthSpec, Icon, Live, Phase, ScrollPos, ScrollSpec, Span, TextHighlight, TextStyle, Tx, col, el, icon, memo,
    rich, row, text,
};
use rune_kit::Kit;
use rune_kit::syntax::SyntaxTheme;

use crate::model;
use crate::state::vim::{SCROLLOFF, VisualSpan};
use crate::state::{Annotation, DiffState, Draw, EXPAND_STEP, Expand, LineRange, LineRef, Metrics, Row, SpanSelection};

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
static CHEVRON_DOWN: Icon = Icon {
    name: "kit-diff-chevron-down",
    view: 16.0,
    body: r#"<path d="M4.5 6.25L8 9.75L11.5 6.25" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static PLUS: Icon = Icon {
    name: "kit-diff-plus",
    view: 16.0,
    body: r#"<path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>"#,
};
static QUESTION: Icon = Icon {
    name: "kit-diff-question",
    view: 16.0,
    body: r#"<path d="M5.6 5.9a2.5 2.5 0 1 1 3.4 2.3c-.6.3-1 .8-1 1.5v.4" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><circle cx="8" cy="12.9" r="1.1" fill="currentColor"/>"#,
};
static CHEVRON_RIGHT: Icon = Icon {
    name: "kit-diff-chevron-right",
    view: 16.0,
    body: r#"<path d="M6.25 4.5L9.75 8L6.25 11.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

/// The marks column: folded threads and notes, and the hover's `+` and `?`.
const MARKS: f32 = 22.0;
/// The `+` / `−` sign column, unified and split.
const SIGN: (f32, f32) = (18.0, 16.0);
/// The least a line-number column takes.
const NUMBER_MIN: f32 = 38.0;
/// The cursor's bar at the line's left edge.
const CURSOR_BAR: f32 = 2.0;
/// Width of the bar that marks the lines a range or span comment covers.
const RANGE_BAR: f32 = 3.0;
/// Space before and after the code.
const CODE_PAD: (f32, f32) = (0.0, 24.0);
/// An all-added file this long draws calm (`DiffView::calm_added_files`).
const CALM_LINES: u32 = 200;
/// How much rows outside a spotlight keep of their opacity.
const DIM: f32 = 0.45;
/// The spotlight outline's stroke.
const SPOT_W: f32 = 2.0;
/// The code's and the line numbers' type sizes.
const CODE_SIZE: f32 = 13.0;
const NUMBER_SIZE: f32 = 12.0;
/// The advance of a mono column of code.
pub(crate) const ADV: f32 = 0.6 * CODE_SIZE;

/// One line-number column for numbers of `digits` digits.
fn number_w(digits: u32) -> f32 {
    (digits as f32 * 0.6 * NUMBER_SIZE + 16.0).ceil().max(NUMBER_MIN)
}

/// Where the code starts in a unified line, after the gutter that holds to the left edge.
pub(crate) fn code_left(digits: u32) -> f32 {
    MARKS + 2.0 * number_w(digits) + SIGN.0 + CODE_PAD.0
}

thread_local! {
    static LINE_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static FOLD_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Lines built on this thread so far, memoised ones not counted: a test reads it around a key
/// to check the key rebuilt only the rows it touched.
pub fn line_builds() -> u64 {
    LINE_BUILDS.with(|n| n.get())
}

/// Gap and file-note rows built on this thread so far, memoised ones not counted.
pub fn fold_builds() -> u64 {
    FOLD_BUILDS.with(|n| n.get())
}

/// The [`Tx`] scroll key of the viewport of a [`DiffView`] with the default key `"diff"`, for
/// `tx.scroll_to_y(SCROLL_KEY, y)`. A view keyed `k` scrolls as `"{k}/scroll"`.
pub const SCROLL_KEY: &str = "diff/scroll";

/// A theme's colours for the diff, in place of tints derived from the kit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffColors {
    pub add_line: Color,
    pub del_line: Color,
    pub add_word: Color,
    pub del_word: Color,
    /// The empty side of a split row.
    pub filler: Color,
    /// The hunk separator's band.
    pub gap: Color,
    /// The file header's band.
    pub header: Color,
    pub hairline: Color,
    /// The cursor's bar.
    pub cursor: Color,
    /// An unchanged line under the cursor.
    pub cursor_line: Color,
    pub selection: Color,
    /// The hover's `+` (comment) and `?` (ask) buttons, and the text on them.
    pub comment: Color,
    pub ask: Color,
    pub on_button: Color,
    /// Find's matches, and the current one.
    pub find_hit: Color,
    pub find_current: Color,
}

/// Fills a theme gives added and removed lines in place of the kit-derived tints.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ChangeFills {
    add: Color,
    del: Color,
}

type Get<S> = Rc<dyn Fn(&S) -> &DiffState>;
type GetMut<S> = Rc<dyn Fn(&mut S) -> &mut DiffState>;
type LinesFn<S> = Rc<dyn Fn(&mut S, LineRange, &mut Tx)>;

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
    colors: Option<DiffColors>,
    annotation: Option<Rc<dyn Fn(&S, &Annotation) -> El>>,
    gutter_mark: Option<Rc<dyn Fn(&S, &[&Annotation]) -> Option<El>>>,
    header: Option<Rc<dyn Fn(&S, usize) -> El>>,
    header_lead: Option<Rc<dyn Fn(&S, usize) -> Option<El>>>,
    header_annotation: Option<Rc<dyn Fn(&S, usize) -> Option<El>>>,
    on_select: Option<Rc<dyn Fn(&mut S, Option<LineRange>, &mut Tx)>>,
    on_comment: Option<LinesFn<S>>,
    on_ask: Option<LinesFn<S>>,
    on_span_select: Option<Rc<dyn Fn(&mut S, Option<SpanSelection>, &mut Tx)>>,
    on_visible_file: Option<Rc<dyn Fn(&mut S, usize)>>,
    on_need_texts: Option<Rc<dyn Fn(&mut S, usize)>>,
    scroll_live: Option<Live<[f32; 4]>>,
    on_scroll: Option<Rc<dyn Fn(ScrollPos)>>,
    block: bool,
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
            colors: None,
            annotation: None,
            gutter_mark: None,
            header: None,
            header_lead: None,
            header_annotation: None,
            on_select: None,
            on_comment: None,
            on_ask: None,
            on_span_select: None,
            on_visible_file: None,
            on_need_texts: None,
            scroll_live: None,
            on_scroll: None,
            block: true,
        }
    }

    /// Draw the cursor as a block on its character (the default); off while a text field outside
    /// the diff has the keyboard.
    pub fn block_cursor(mut self, on: bool) -> Self {
        self.block = on;
        self
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

    /// Draw files of 200 lines or more that are all added with only the sign by their lines,
    /// not the full green wash: a new file reads as text, not as a wall of change.
    pub fn calm_added_files(mut self, on: bool) -> Self {
        self.calm = on;
        self
    }

    /// Paint added and removed lines `add` and `del` rather than tints of the kit's success and
    /// danger colours; their changed words mix those colours in, as the tints do.
    pub fn change_fills(mut self, add: Color, del: Color) -> Self {
        self.fills = Some(ChangeFills { add, del });
        self
    }

    /// The text selection's colour, in place of the kit's.
    pub fn selection_color(mut self, c: Color) -> Self {
        self.selection = Some(c);
        self
    }

    /// Every colour the diff paints, from a theme's roles.
    pub fn colors(mut self, c: DiffColors) -> Self {
        self.colors = Some(c);
        self
    }

    /// Build each [`Annotation`] under its last line. Rows holding annotations are built every
    /// commit, so a widget can read app state (a composer's text) freely.
    pub fn annotation(mut self, f: impl Fn(&S, &Annotation) -> El + 'static) -> Self {
        self.annotation = Some(Rc::new(f));
        self
    }

    /// What the marks column shows by a line that carries annotations (a folded thread's badge),
    /// given the annotations under that line; `None` leaves it to the hover's buttons.
    pub fn gutter_mark(mut self, f: impl Fn(&S, &[&Annotation]) -> Option<El> + 'static) -> Self {
        self.gutter_mark = Some(Rc::new(f));
        self
    }

    /// Extra controls at the end of each file's header (a "viewed" toggle, a menu), by file.
    pub fn header_actions(mut self, f: impl Fn(&S, usize) -> El + 'static) -> Self {
        self.header = Some(Rc::new(f));
        self
    }

    /// Controls right after a file's name (a copy-path button, its group's tag), by file.
    pub fn header_lead(mut self, f: impl Fn(&S, usize) -> Option<El> + 'static) -> Self {
        self.header_lead = Some(Rc::new(f));
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

    /// Show a `+` in the marks column of the hovered line, and call `f` with the lines it stands
    /// for when pressed: the selection when the line is in it, else that line.
    pub fn on_comment(mut self, f: impl Fn(&mut S, LineRange, &mut Tx) + 'static) -> Self {
        self.on_comment = Some(Rc::new(f));
        self
    }

    /// Show a `?` beside the hover's `+`, and call `f` with the lines it stands for, as
    /// [`DiffView::on_comment`] does.
    pub fn on_ask(mut self, f: impl Fn(&mut S, LineRange, &mut Tx) + 'static) -> Self {
        self.on_ask = Some(Rc::new(f));
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
        let look = Look::new(k, self.fills, self.selection, self.colors, st.metrics());
        let wrap = st.wraps();
        let key = self.key.clone();
        if let Some(r) = rune::memo::layout(&format!("{key}/scroll")) {
            st.set_viewport(r);
        }
        st.set_scroll_key(&key);
        let vis = st.visual_span();
        let v = Rc::new(View { look, d: self, vis });
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
            .virtual_rows(count, v.look.m.line, keys(v.d.get.clone()), move |s: &S, i| build.row(s, i))
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
            at_cell.set([at.offset[1], at.viewport[1], at.offset[0]]);
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
            let (adv, gutter) = (v.look.adv, MARKS + v.look.number_w(digits) + SIGN.1 + CODE_PAD.0 + CODE_PAD.1);
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

/// Colours and type, from the kit's palette (or a theme's [`DiffColors`]) so themes restyle them.
struct Look {
    m: Metrics,
    font: TextStyle,
    number: TextStyle,
    label: TextStyle,
    strong: TextStyle,
    small: TextStyle,
    small_sans: TextStyle,
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
    cursor_line: Color,
    selected: Color,
    selection: Color,
    accent: Color,
    on_accent: Color,
    comment: Color,
    ask: Color,
    range_bar: Color,
    span_mark: Color,
    find_hit: Color,
    find_current: Color,
    syntax: SyntaxTheme,
    /// Changes when anything above does, for memoised rows.
    version: u64,
}

#[derive(Clone, Copy)]
struct Tints {
    line: Color,
    word: Color,
    sign: Color,
    count: Color,
}

impl Look {
    fn new(k: &Kit, fills: Option<ChangeFills>, selection: Option<Color>, colors: Option<DiffColors>, m: Metrics) -> Look {
        let c = &k.c;
        let bg = c.bg_base;
        let dark = k.dark;
        let tints = |hue: Color, fg: Color, fill: Option<Color>, word: Option<Color>| {
            let (line, derived) = match fill {
                Some(line) => (line, srgb_mix(line, hue, 0.25)),
                None => (
                    tint(bg, hue, if dark { 0.13 } else { 0.1 }),
                    tint(bg, hue, if dark { 0.32 } else { 0.27 }),
                ),
            };
            Tints {
                line,
                word: word.unwrap_or(derived),
                sign: fg,
                count: fg,
            }
        };
        let font = TextStyle {
            size: CODE_SIZE,
            line_height: m.line,
            ..k.ty.mono
        };
        let info = c.info.icon;
        let warn = c.warning.icon;
        let (add_fill, del_fill) = match (colors, fills) {
            (Some(d), _) => (Some(d.add_line), Some(d.del_line)),
            (None, Some(f)) => (Some(f.add), Some(f.del)),
            (None, None) => (None, None),
        };
        let mut look = Look {
            m,
            font,
            number: TextStyle {
                size: NUMBER_SIZE,
                line_height: m.line,
                ..k.ty.mono
            },
            label: TextStyle {
                size: 13.0,
                line_height: 20.0,
                ..k.ty.ui
            },
            strong: TextStyle {
                size: 13.0,
                line_height: 20.0,
                weight: 600.0,
                ..k.ty.ui
            },
            small: TextStyle {
                size: 12.0,
                line_height: 18.0,
                ..k.ty.mono
            },
            small_sans: TextStyle {
                size: 12.0,
                line_height: 18.0,
                ..k.ty.ui
            },
            adv: ADV,
            bg,
            text: c.text_base,
            muted: c.text_muted,
            faint: c.text_faint,
            border: colors.map_or(c.border_weak, |d| d.hairline),
            header_bg: colors.map_or(c.layer_01, |d| d.header),
            hover: k.alpha(c.text_base, if dark { 0.06 } else { 0.035 }),
            add: tints(c.success.icon, c.success.fg, add_fill, colors.map(|d| d.add_word)),
            del: tints(c.danger.icon, c.danger.fg, del_fill, colors.map(|d| d.del_word)),
            gap_bg: colors.map_or_else(|| tint(bg, info, if dark { 0.1 } else { 0.06 }), |d| d.gap),
            empty_bg: colors.map_or_else(|| tint(bg, c.text_base, if dark { 0.03 } else { 0.025 }), |d| d.filler),
            cursor_line: colors.map_or_else(|| tint(bg, c.text_base, if dark { 0.05 } else { 0.04 }), |d| d.cursor_line),
            selected: tint(bg, info, if dark { 0.2 } else { 0.14 }),
            selection: colors.map(|d| d.selection).or(selection).unwrap_or_else(|| k.selection()),
            accent: colors.map_or(info, |d| d.cursor),
            on_accent: colors.map_or(c.white, |d| d.on_button),
            comment: colors.map_or(info, |d| d.comment),
            ask: colors.map_or(info, |d| d.ask),
            range_bar: warn,
            span_mark: k.alpha(warn, if dark { 0.3 } else { 0.25 }),
            find_hit: colors.map_or_else(|| k.alpha(warn, 0.35), |d| d.find_hit),
            find_current: colors.map_or_else(|| k.alpha(warn, 0.7), |d| d.find_current),
            syntax: SyntaxTheme::from_kit(k),
            version: 0,
        };
        let mut h = std::hash::DefaultHasher::new();
        for c in [
            look.bg,
            look.text,
            look.add.line,
            look.del.line,
            look.add.word,
            look.del.word,
            look.selection,
            look.accent,
            look.header_bg,
            look.range_bar,
            look.empty_bg,
            look.gap_bg,
        ] {
            [c.r, c.g, c.b, c.a].map(f32::to_bits).hash(&mut h);
        }
        look.font.size.to_bits().hash(&mut h);
        m.line.to_bits().hash(&mut h);
        m.header.to_bits().hash(&mut h);
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

    fn line_fill(&self, kind: LineKind, selected: bool, calm: bool, cursor: bool) -> Color {
        match (selected, self.tints(kind)) {
            (true, _) => self.selected,
            (false, Some(t)) if !calm => t.line,
            _ if cursor => self.cursor_line,
            _ => self.bg,
        }
    }

    /// One line-number column for numbers of `digits` digits.
    fn number_w(&self, digits: u32) -> f32 {
        number_w(digits)
    }
}

/// One side's line in a row: what a cell draws.
#[derive(Clone)]
struct Cell<'a> {
    /// Its index in the file's lines (none for unchanged lines between hunks).
    idx: Option<u32>,
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

    /// The line a comment or mark on this cell anchors to: the new side unless removed.
    fn anchor(&self) -> Option<LineRef> {
        match (self.kind, self.old, self.new) {
            (LineKind::Removed, Some(n), _) => Some(LineRef::new(Side::Old, n)),
            (_, _, Some(n)) => Some(LineRef::new(Side::New, n)),
            (_, Some(n), None) => Some(LineRef::new(Side::Old, n)),
            _ => None,
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
    /// The block's side and bytes, on the cursor's row.
    block: Option<(Side, u32, u32)>,
    /// Visual mode's side and bytes on this row (charwise).
    vis: Option<(Side, u32, u32)>,
    /// The viewport's height on the cursor's row, which sizes its margin box.
    room: u32,
    /// The viewport's width on the cursor's row while lines wrap, which places its block.
    wide: u32,
    range: [bool; 2],
    spans: u64,
}

impl Marks {
    /// The block on `side`'s cell (any cell in one column).
    fn block_on(&self, side: Option<Side>) -> Option<Range<usize>> {
        self.block
            .filter(|b| side.is_none_or(|s| s == b.0))
            .map(|(_, a, b)| a as usize..b as usize)
    }

    fn vis_on(&self, side: Option<Side>) -> Option<Range<usize>> {
        self.vis
            .filter(|v| side.is_none_or(|s| s == v.0))
            .map(|(_, a, b)| a as usize..b as usize)
    }
}

struct View<S> {
    d: DiffView<S>,
    look: Look,
    /// Visual mode's extent, worked out once a build.
    vis: Option<VisualSpan>,
}

impl<S: 'static> View<S> {
    /// How wide unwrapped (unified) content is, so the list doesn't change width as rows are built.
    fn content_width(&self, st: &DiffState) -> f32 {
        let l = &self.look;
        let digits = (0..st.files().len()).map(|f| st.digits(f)).max().unwrap_or(3);
        MARKS + 2.0 * l.number_w(digits) + SIGN.0 + st.columns() as f32 * l.adv + CODE_PAD.0 + CODE_PAD.1
    }

    /// The most a full-width part of a row (a header, an annotation) spans: the viewport, not
    /// the unwrapped content.
    fn view_w(&self, st: &DiffState) -> Option<f32> {
        let w = st.view_width();
        (w > 0.0 && !st.wraps()).then_some(w)
    }

    fn calm(&self, st: &DiffState, f: usize) -> bool {
        let file = &st.files()[f];
        self.d.calm && file.additions >= CALM_LINES && file.deletions == 0 && file.lines.len() == file.additions as usize
    }

    fn marks(&self, st: &DiffState, i: usize, r: Row) -> Marks {
        let f = r.file();
        let cursor = st.cursor_row() == Some(i as u32);
        let mut m = Marks {
            cursor,
            block: st
                .block()
                .filter(|_| cursor && self.d.block)
                .map(|(s, b)| (s, b.start as u32, b.end as u32)),
            vis: self
                .vis
                .as_ref()
                .filter(|v| (v.rows.0..=v.rows.1).contains(&(i as u32)))
                .and_then(|v| {
                    let side = st.visual_side(v, i as u32);
                    let len = st.row_text(i as u32, side)?.len();
                    v.bytes(i as u32, len).map(|b| (side, b.start as u32, b.end as u32))
                }),
            room: if cursor { st.viewport_height().to_bits() } else { 0 },
            wide: if cursor && st.wraps() { st.view_width().to_bits() } else { 0 },
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
        let lit = |e: El| if dim { e.child(self.dim_scrim()) } else { e };
        match r {
            Row::File(f) => lit(self.file_row(s, st, f as usize)),
            Row::Note(f) => memo(&key, (self.look.version, st.rev, self.view_w(st).map(f32::to_bits), dim), || {
                lit(self.note(st, f as usize))
            }),
            Row::Gap { f, gap } => memo(&key, (self.look.version, st.rev, self.view_w(st).map(f32::to_bits), dim), || {
                lit(self.gap(st, f as usize, gap as usize))
            }),
            _ if !st.annotations().is_empty() && self.d.annotation.is_some() && self.annotated(st, r) => {
                lit(self.line(s, st, i, &key, true))
            }
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
                    st.hovered_line(&key),
                );
                memo(&key, version, || lit(self.line(s, st, i, &key, false)))
            }
        }
    }

    /// A row outside the spotlight under the reader's ground at `1 - DIM`: the look of the row at
    /// `DIM` opacity, without a layer per row. Sticky, so it paints over the sticky gutter too.
    fn dim_scrim(&self) -> El {
        el().key("dim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(self.look.bg.alpha(1.0 - DIM))
            .sticky_left(0.0)
            .inert()
            .a11y_hidden()
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
                idx: Some(i),
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
                    idx: None,
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

    /// A line's mark from the app, only for lines that carry annotations.
    fn mark(&self, s: &S, f: usize, at: Option<LineRef>, annotated: bool) -> Option<El> {
        let build = self.d.gutter_mark.as_ref().filter(|_| annotated)?;
        let st = (self.d.get)(s);
        let here: Vec<&Annotation> = st.annotations_at(f, at?).collect();
        if here.is_empty() {
            return None;
        }
        build(s, &here)
    }

    fn line(&self, s: &S, st: &DiffState, i: usize, key: &str, annotated: bool) -> El {
        LINE_BUILDS.with(|n| n.set(n.get() + 1));
        let r = st.rows[i];
        let f = r.file();
        let (a, b) = self.cells(st, r);
        let Some(a) = a else { return col() };
        let marks = self.marks(st, i, r);
        let reveal = marks.cursor.then(|| self.reveal_box(st));
        let draw = st.draw(f);
        let line = match draw {
            Draw::Unified => {
                let mark = self.mark(s, f, a.anchor(), annotated);
                self.unified(st, i, f, a, marks, key, None, mark)
            }
            Draw::One(side) => {
                let mark = self.mark(s, f, a.anchor(), annotated);
                self.unified(st, i, f, a, marks, key, Some(side), mark)
            }
            Draw::Split => {
                let (old, new) = match (b, a.kind) {
                    (Some(b), _) => (Some(a), Some(b)),
                    (None, LineKind::Removed) => (Some(a), None),
                    (None, LineKind::Added) => (None, Some(a)),
                    (None, LineKind::Context) => (Some(a.clone()), Some(a)),
                };
                let old_mark = old
                    .as_ref()
                    .and_then(|c| c.old)
                    .and_then(|n| self.mark(s, f, Some(LineRef::new(Side::Old, n)), annotated));
                let new_mark = new
                    .as_ref()
                    .and_then(|c| c.new)
                    .and_then(|n| self.mark(s, f, Some(LineRef::new(Side::New, n)), annotated));
                self.split(st, i, f, old, new, marks, key, [old_mark, new_mark])
            }
        };
        let Some(build) = self.d.annotation.as_ref().filter(|_| annotated) else {
            return line.children(reveal);
        };
        // Annotations run the reader's full width under the line, in unified and split alike.
        let mut c = col().key("notes").w_full();
        if let Some(w) = self.view_w(st) {
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
        col()
            .w_full()
            .child(line.key("line"))
            .child(c.children(all.map(|a| note(build(s, a), a))))
            .children(reveal)
    }

    /// An invisible box around the cursor's line, three lines taller each way (and a sticky
    /// header's height above), that a cursor move scrolls just into view: Vim's `scrolloff`.
    fn reveal_box(&self, st: &DiffState) -> El {
        let m = &self.look.m;
        let (mut above, mut below) = (SCROLLOFF * m.line + m.header, SCROLLOFF * m.line);
        let room = st.viewport_height() - m.line;
        if room > 0.0 && above + below > room {
            let k = room / (above + below);
            (above, below) = (above * k, below * k);
        }
        el().key("vim-reveal")
            .absolute(0.0, -above)
            .w_full()
            .h(above + m.line + below)
            .inert()
            .a11y_hidden()
    }

    /// Whether lines carry the hover's `+` and `?`.
    fn adds(&self) -> bool {
        self.d.on_comment.is_some() || self.d.on_ask.is_some()
    }

    /// Tracks the pointer over a line (`side` for a split half), so only that line and the
    /// cursor's mount the `+` and `?`.
    fn track_hover(&self, e: El, key: &str, side: Option<Side>) -> El {
        if !self.adds() {
            return e;
        }
        let (get, key) = (self.d.get_mut.clone(), key.to_owned());
        e.on_hover_change(move |s: &mut S, inside, tx: &mut Tx| {
            if !get(s).hover_line(&key, side, inside) {
                tx.live_only();
            }
        })
    }

    /// The marks column: the app's mark, else the hover's `+` and `?`, mounted only on the
    /// hovered and cursor lines (`show`), so assistive tech meets one pair, on the line in hand.
    /// An empty cell stays hidden from it.
    fn marks_cell(&self, f: usize, at: Option<LineRef>, mark: Option<El>, show: bool) -> El {
        let l = &self.look;
        let cell = row().key("mk").w(MARKS).h(l.m.line).center();
        if let Some(m) = mark {
            return cell.child(m);
        }
        if let Some(at) = at
            && show
            && self.adds()
        {
            // Each button presses over the line's full height and half the pair's width.
            let mut add = row().key("hover-add").absolute(1.0, 0.0).h(l.m.line).group_opacity(0.0, 1.0);
            if let Some(cb) = self.d.on_comment.clone() {
                add = add.child(self.hover_button(f, at, "plus", &PLUS, "Comment on this line  c", l.comment, cb));
            }
            if let Some(cb) = self.d.on_ask.clone() {
                add = add.child(self.hover_button(f, at, "ask", &QUESTION, "Ask about this line  ?", l.ask, cb));
            }
            return cell.child(add);
        }
        cell.a11y_hidden()
    }

    #[allow(clippy::too_many_arguments)]
    fn hover_button(&self, f: usize, at: LineRef, key: &str, glyph: &'static Icon, label: &str, fill: Color, cb: LinesFn<S>) -> El {
        let l = &self.look;
        let get = self.d.get_mut.clone();
        row()
            .key(key.to_owned())
            .w(20.0)
            .h_full()
            .center()
            .cursor(Cursor::Pointer)
            .role("button")
            .label(label.to_owned())
            .help(label.to_owned())
            .child(
                row()
                    .size(18.0, 16.0)
                    .center()
                    .radius(4.0)
                    // Solid, so the code under the gutter never shows through.
                    .fill(fill.alpha(1.0))
                    // A drawn glyph, centred in its box (text glyphs sit on a baseline, not the middle).
                    .child(icon(glyph, 12.0).color(l.on_accent).a11y_hidden()),
            )
            .on_press_tx(move |s: &mut S, tx| {
                let st = get(s);
                let lines = match st.selection() {
                    Some(sel) if sel.file == f && st.contains(&sel, at) => sel,
                    _ => LineRange::line(f, at),
                };
                cb(s, lines, tx);
            })
    }

    /// The `+` or `−` before the code.
    fn sign(&self, kind: LineKind, w: f32) -> El {
        let l = &self.look;
        let (glyph, color) = match (kind, self.look.tints(kind)) {
            (LineKind::Added, Some(t)) => ("+", t.sign),
            (LineKind::Removed, Some(t)) => ("−", t.sign),
            _ => (" ", l.faint),
        };
        row()
            .key("sg")
            .w(w)
            .h(l.m.line)
            .justify_center()
            .a11y_hidden()
            .child(text(glyph).font(l.font).color(color))
    }

    /// A line in one column: both numbers, or with `one` that side's only (a one-sided file in
    /// split mode, across the whole width).
    #[allow(clippy::too_many_arguments)]
    fn unified(&self, st: &DiffState, i: usize, f: usize, c: Cell<'_>, marks: Marks, key: &str, one: Option<Side>, mark: Option<El>) -> El {
        let l = &self.look;
        let selected = st.row_selected(i, None);
        let calm = self.calm(st, f);
        let fill = l.line_fill(c.kind, selected, calm, marks.cursor);
        let digits = st.digits(f);
        let copy = match c.kind {
            LineKind::Context => true,
            LineKind::Added => st.copy_side() == Side::New,
            LineKind::Removed => st.copy_side() == Side::Old,
        };
        let hover_side = if c.kind == LineKind::Removed { Side::Old } else { Side::New };
        // The marks, numbers and sign hold to the viewport's left edge while the code scrolls
        // sideways under them, on the line's own opaque fill.
        let show = marks.cursor || st.hovered_line(key).is_some();
        let mut gutter = row()
            .key("gutter")
            .self_stretch()
            .items_start()
            .fill(fill)
            .child(self.marks_cell(f, c.anchor(), mark, show));
        let numbers = match one {
            Some(side) => {
                let n = if side == Side::Old { c.old } else { c.new };
                gutter = gutter.child(self.number(st, i, f, n, side, selected, marks.cursor, digits).key("n"));
                1.0
            }
            None => {
                gutter = gutter
                    .child(self.number(st, i, f, c.old, Side::Old, selected, marks.cursor, digits).key("no"))
                    .child(self.number(st, i, f, c.new, Side::New, selected, marks.cursor, digits).key("nn"));
                2.0
            }
        };
        gutter = gutter.child(self.sign(if calm { LineKind::Context } else { c.kind }, SIGN.0));
        if marks.range[0] || marks.range[1] {
            gutter = gutter.child(self.range_bar().absolute(MARKS + numbers * l.number_w(digits) - RANGE_BAR, 0.0));
        }
        if !st.wraps() {
            gutter = gutter.sticky_left(0.0);
        }
        let label = c.label();
        let mut r = row()
            .w_full()
            .min_h(l.m.line)
            .items_start()
            .fill(fill)
            .hover(l.hover)
            .hover_group()
            .role("listitem")
            .label(label)
            .child(gutter)
            .child(
                self.code(st, i, f, &c, copy, hover_side, key, marks.block_on(None), marks.vis_on(None))
                    .key("code"),
            );
        if marks.cursor {
            r = r.child(self.cursor_bar());
        }
        self.track_hover(r, key, None)
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
        side_marks: [Option<El>; 2],
    ) -> El {
        let digits = st.digits(f);
        let calm = self.calm(st, f);
        let [old_mark, new_mark] = side_marks;
        let half = |side: Side, c: Option<Cell<'a>>, mark: Option<El>| -> El {
            let l = &self.look;
            let hk = if side == Side::Old { "old" } else { "new" };
            let mut h = row().key(hk).w_frac(0.5).min_w(0.0).min_h(l.m.line).items_start().self_stretch();
            let Some(c) = c else {
                return h.fill(l.empty_bg);
            };
            let kind = c.kind;
            let selected = st.row_selected(i, Some(side));
            let fill = l.line_fill(kind, selected, calm, marks.cursor);
            let n = if side == Side::Old { c.old } else { c.new };
            let mut cell = c;
            if kind == LineKind::Context {
                cell.text = st.side_line(f, side, n.unwrap_or(1)).unwrap_or(cell.text);
            }
            let copy = st.copy_side() == side;
            let label = cell.label();
            let at = n.map(|n| LineRef::new(side, n));
            let show = marks.cursor || st.hovered_line(key) == Some(Some(side));
            h = h
                .fill(fill)
                .hover(l.hover)
                .hover_group()
                .role("listitem")
                .label(label)
                .child(self.marks_cell(f, at, mark, show))
                .child(self.number(st, i, f, n, side, selected, marks.cursor, digits).key("n"))
                .child(self.sign(if calm { LineKind::Context } else { kind }, SIGN.1))
                .child(
                    self.code(
                        st,
                        i,
                        f,
                        &cell,
                        copy,
                        side,
                        key,
                        marks.block_on(Some(side)),
                        marks.vis_on(Some(side)),
                    )
                    .key("code"),
                );
            if marks.range[side as usize] {
                h = h.child(self.range_bar().absolute(MARKS + l.number_w(digits) - RANGE_BAR, 0.0));
            }
            if marks.cursor && (side == Side::New || kind == LineKind::Removed) {
                h = h.child(self.cursor_bar());
            }
            self.track_hover(h, key, Some(side))
        };
        let divider = col().key("divider").w(1.0).self_stretch().fill(self.look.border);
        row()
            .w_full()
            .min_h(self.look.m.line)
            .items_stretch()
            .child(half(Side::Old, old, old_mark))
            .child(divider)
            .child(half(Side::New, new, new_mark))
    }

    /// The cursor's bar at the line's left edge.
    fn cursor_bar(&self) -> El {
        el().key("cursor")
            .pin(Some(0.0), Some(0.0), None, Some(0.0))
            .w(CURSOR_BAR)
            .fill(self.look.accent)
            .a11y_hidden()
    }

    /// The bar beside the numbers of lines a range or span comment covers.
    fn range_bar(&self) -> El {
        col().key("range").w(RANGE_BAR).h_full().fill(self.look.range_bar).a11y_hidden()
    }

    /// A line-number cell: pressing it selects the line, shift-pressing extends the selection to
    /// it, and dragging down the numbers selects the lines passed.
    #[allow(clippy::too_many_arguments)]
    fn number(&self, st: &DiffState, i: usize, f: usize, n: Option<u32>, side: Side, selected: bool, cursor: bool, digits: u32) -> El {
        let l = &self.look;
        let unified = st.draw(f) != Draw::Split;
        let mut c = row()
            .w(l.number_w(digits))
            .min_h(l.m.line)
            .self_stretch()
            .pr(8.0)
            .justify_end()
            .items_start()
            .a11y_hidden();
        if let Some(n) = n {
            let color = if selected || cursor { l.text } else { l.faint };
            c = c.child(text(n.to_string()).font(l.number).color(color));
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

    /// The code of a line: syntax colours under word changes, tabs padded to four columns, span
    /// comments' text tinted without moving a glyph; visual mode's bytes in the selection colour
    /// and the cursor's `block` inverted, the text's colour behind the ground's.
    #[allow(clippy::too_many_arguments)]
    fn code(
        &self,
        st: &DiffState,
        i: usize,
        f: usize,
        c: &Cell<'_>,
        copy: bool,
        side: Side,
        row_key: &str,
        block: Option<Range<usize>>,
        vis: Option<Range<usize>>,
    ) -> El {
        let l = &self.look;
        let (src, n) = c.source();
        let syntax: Vec<(Range<usize>, Color)> = st.highlights(f, src, n).into_iter().map(|(r, p)| (r, l.syntax.paint(p))).collect();
        let word = l.tints(c.kind).map(|t| t.word);
        let spans = layer(c.text.len(), &syntax, word.map_or(&[][..], |_| &c.changes[..]), word);
        let (found, current) = st.find_marks(c.text, c.idx.map(|i| (f, i)));
        let paint = MarkPaint {
            span: l.span_mark,
            find: if current { l.find_current } else { l.find_hit },
            selection: l.selection,
            text: l.text,
            bg: l.bg,
        };
        let has_block = block.is_some();
        let m = LineMarks {
            spans: st.span_marks(f, LineRef::new(src, n)),
            found,
            vis,
            block,
        };
        let wrap = st.wraps();
        let (shown, spans, pads, mut lit) = drawn(c.text, spans, m, wrap, &paint);
        let cell_w = rune::text::advance("0", l.font);
        // Highlights have round corners and paint under later ones: the block is a square over
        // the text with its glyph drawn again inverted, on whichever wrapped line it sits.
        let block = if has_block { lit.pop() } else { None };
        let square = block.and_then(|h| {
            let r = h.range();
            let at = if wrap {
                wrap_width(st, f).map(|w| wrapped_line(&shown, r.start, w, cell_w))
            } else {
                Some((0, 0))
            };
            if at.is_none() {
                lit.push(h);
            }
            at.map(|(line, start)| {
                let x = x_in(&shown[start..r.start], l.font, cell_w);
                let y = line as f32 * l.font.line_height;
                let w = rune::text::advance(&shown[r.clone()], l.font).max(cell_w);
                el().key("block")
                    .absolute(x, y)
                    .w(w)
                    .h(l.font.line_height)
                    .fill(l.text)
                    .inert()
                    .a11y_hidden()
                    .child(text(shown[r].to_owned()).font(l.font).color(l.bg).single_line())
            })
        });
        let mut t = rich(shown, spans).font(l.font).color(l.text).key("text");
        t = if wrap { t.w_full() } else { t.single_line() };
        if !lit.is_empty() {
            t = t.text_highlights(lit);
        }
        if copy {
            t = t.selectable(true).selection_color(l.selection);
            t = pads.into_iter().fold(t, |t, r| t.uncopied(r));
        }
        let (get, cell) = (self.d.get_mut.clone(), format!("{row_key}.{}", side as u8));
        let (aim, click) = (self.d.get_mut.clone(), self.d.get_mut.clone());
        let (row_at, line_h, font) = (i as u32, l.font.line_height, l.font);
        let mut code = col()
            .grow(1.0)
            .min_w(0.0)
            .pl(CODE_PAD.0)
            .pr(CODE_PAD.1)
            .child(t)
            .on_pointer_move_local(move |s: &mut S, p, tx: &mut Tx| {
                aim(s).pointer = [p.x - CODE_PAD.0, p.y];
                tx.live_only();
            })
            .on_press_tx(move |s: &mut S, tx: &mut Tx| {
                let st = click(s);
                let [x, y] = st.pointer;
                let line = (y / line_h).max(0.0) as usize;
                let byte = clicked_byte(st, row_at, side, line, x, |s| x_in(s, font, cell_w), cell_w);
                st.click_code(row_at, side, byte, tx);
            })
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
        match square {
            Some(b) => code.child(b),
            None => code,
        }
    }

    fn gutter_w(&self, st: &DiffState, f: usize) -> f32 {
        let n = self.look.number_w(st.digits(f));
        if st.draw(f) == Draw::Unified {
            MARKS + 2.0 * n + SIGN.0
        } else {
            MARKS + n + SIGN.1
        }
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
            .items_start()
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
                    .corners([4.0, 4.0, 0.0, 0.0])
                    .fill(l.accent)
                    .offset(14.0, -18.0)
                    .child(text(c).font(l.small_sans).color(l.on_accent).single_line()),
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

    /// One of the hunk separator's show-more buttons.
    fn expand_button(&self, key: String, glyph: &'static Icon, label: String, press: impl Fn(&mut S, &mut Tx) + 'static) -> El {
        let l = &self.look;
        row()
            .key(key)
            .size(24.0, 22.0)
            .center()
            .radius(4.0)
            .hover(l.hover)
            .cursor(Cursor::Pointer)
            .role("button")
            .label(label.clone())
            .help(label)
            .child(icon(glyph, 16.0).color(l.muted))
            .on_press_tx(press)
    }

    /// The unchanged lines between two hunks, folded: a band with buttons to show more, the
    /// count and the next hunk's header.
    fn gap(&self, st: &DiffState, f: usize, gap: usize) -> El {
        FOLD_BUILDS.with(|n| n.set(n.get() + 1));
        let l = &self.look;
        let file = &st.files()[f];
        let hidden = st.hidden(f, gap).unwrap_or(0);
        let last = gap == file.hunks.len();
        let expandable = st.expandable(f);
        let mut buttons = Vec::new();
        if expandable {
            let step = hidden.min(EXPAND_STEP);
            let mut add = |how: Expand, glyph: &'static Icon, label: String| {
                let get = self.d.get_mut.clone();
                buttons.push(self.expand_button(format!("{how:?}"), glyph, label, move |s: &mut S, _| get(s).expand(f, gap, how)));
            };
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
            buttons.push(self.expand_button(
                "load".into(),
                if gap == 0 { &EXPAND_UP } else { &EXPAND_DOWN },
                "Load the whole file to show unchanged lines".into(),
                move |s: &mut S, _| {
                    get(s).request_expand(f, gap, how);
                    need(s, f);
                },
            ));
        }
        let buttons = row().key("expand").h(l.m.gap).items_center().gap(2.0).children(buttons);
        let mut inner = row().key("inner").h(l.m.gap).pl(4.0).items_center().gap(10.0).child(buttons);
        if let Some(w) = self.view_w(st) {
            inner = inner.max_w(w).sticky_left(0.0);
        }
        if hidden > 0 {
            let count = text(if hidden == 1 {
                "1 unchanged line".to_owned()
            } else {
                format!("{hidden} unchanged lines")
            })
            .font(l.small_sans)
            .color(l.muted)
            .key("count");
            let count = if expandable && hidden > EXPAND_STEP {
                let get = self.d.get_mut.clone();
                row()
                    .key("all")
                    .px(4.0)
                    .h(22.0)
                    .items_center()
                    .radius(4.0)
                    .hover(l.hover)
                    .cursor(Cursor::Pointer)
                    .role("button")
                    .label(format!("Show all {hidden} lines"))
                    .on_press_tx(move |s: &mut S, _| get(s).expand(f, gap, Expand::All))
                    .child(count)
            } else {
                row().key("all").child(count)
            };
            inner = inner.child(count);
        }
        let header = file.hunks.get(gap).map(|h| model::hunk_header(file, h));
        if let Some(h) = &header {
            inner = inner.child(text(h.clone()).font(l.small).color(l.faint).single_line().key("header"));
        }
        let label = match &header {
            Some(h) => format!("{hidden} unchanged lines, then {h}"),
            None if st.hidden(f, gap).is_none() => "Unchanged lines to the end of the file".to_owned(),
            None => format!("{hidden} unchanged lines"),
        };
        let line = |key: &'static str, top: bool| {
            el().key(key)
                .pin(
                    Some(0.0),
                    if top { Some(0.0) } else { None },
                    Some(0.0),
                    if top { None } else { Some(0.0) },
                )
                .h(1.0)
                .fill(l.border)
                .a11y_hidden()
        };
        row()
            .w_full()
            .h(l.m.gap)
            .items_center()
            .fill(l.gap_bg)
            .role("group")
            .label(label)
            .child(inner)
            .child(line("line-top", true))
            .child(line("line-bottom", false))
    }

    /// A file with no lines to show.
    fn note(&self, st: &DiffState, f: usize) -> El {
        FOLD_BUILDS.with(|n| n.set(n.get() + 1));
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
            .h(l.m.note)
            .items_center()
            .pl(self.gutter_w(st, f) + CODE_PAD.0)
            .child(text(msg).font(l.small_sans).color(l.muted));
        if let Some(w) = self.view_w(st) {
            inner = inner.max_w(w).sticky_left(0.0);
        }
        row().w_full().h(l.m.note).fill(l.bg).role("note").label(msg).child(inner)
    }

    /// A file's row: its header, and the file-level widget under it when the app gives one.
    fn file_row(&self, s: &S, st: &DiffState, f: usize) -> El {
        let header = self.file_header(s, st, f, false);
        let Some(slot) = self.d.header_annotation.as_ref().and_then(|b| b(s, f)) else {
            return header;
        };
        let mut c = col().key("file-note").w_full();
        if let Some(w) = self.view_w(st) {
            c = c.w(w).sticky_left(0.0);
        }
        col().w_full().child(header.key("header")).child(c.child(slot))
    }

    /// `▾ server · src/Api/VouchersController.cs  +14 −6  [the app's actions]`, sticky while its
    /// file scrolls.
    fn file_header(&self, s: &S, st: &DiffState, f: usize, sticky: bool) -> El {
        let l = &self.look;
        let file = &st.files()[f];
        let collapsed = st.collapsed(f);
        let get = self.d.get_mut.clone();
        let toggle = row()
            .key("toggle")
            .size(22.0, 22.0)
            .center()
            .radius(4.0)
            .hover(l.hover)
            .cursor(Cursor::Pointer)
            .role("button")
            .label(if collapsed { "Expand file" } else { "Collapse file" })
            .help(if collapsed { "Expand file" } else { "Collapse file" })
            .expanded(!collapsed)
            .child(icon(if collapsed { &CHEVRON_RIGHT } else { &CHEVRON_DOWN }, 16.0).color(l.faint))
            .on_press_tx(move |s: &mut S, tx| {
                let st = get(s);
                st.set_collapsed(f, !st.collapsed(f));
                if sticky {
                    st.scroll_to_file(f, tx);
                }
            });
        let repo = st.repo_id(f);
        // `server · src/Api/` muted, then the name strong.
        let split_path = |p: &str| -> (String, String) {
            let (dir, name) = p.rsplit_once('/').map_or(("", p), |(d, n)| (d, n));
            let mut lead = String::new();
            if let Some(r) = repo {
                lead.push_str(r);
                lead.push_str(" · ");
            }
            if !dir.is_empty() {
                lead.push_str(dir);
                lead.push('/');
            }
            (lead, name.to_owned())
        };
        let path = |p: &str, key: &'static str, strong: bool| {
            let (lead, name) = split_path(p);
            row()
                .key(key)
                .items_center()
                .min_w(0.0)
                .shrink()
                .child(
                    text(lead)
                        .font(l.label)
                        .color(l.muted)
                        .truncate_head()
                        .min_w(0.0)
                        .shrink()
                        .key("dir"),
                )
                .child(
                    text(name)
                        .font(if strong { l.strong } else { l.label })
                        .color(if strong { l.text } else { l.muted })
                        .truncate()
                        .key("name"),
                )
        };
        let mut name = row().key("path").items_center().gap(6.0).min_w(0.0).shrink();
        name = match (file.status, file.old_path.as_deref()) {
            (FileStatus::Renamed | FileStatus::Copied, Some(a)) => name
                .child(path(a, "old", false))
                .child(text("→").font(l.label).color(l.faint).key("arrow"))
                .child(path(&file.path, "new", true)),
            _ => name.child(path(&file.path, "new", true)),
        };
        let status = match file.status {
            FileStatus::Added => Some(("Added", l.add.count)),
            FileStatus::Deleted => Some(("Deleted", l.del.count)),
            FileStatus::Renamed => Some(("Renamed", l.muted)),
            FileStatus::Copied => Some(("Copied", l.muted)),
            FileStatus::Modified => None,
        };
        let (adds, dels) = (file.additions, file.deletions);
        let mut inner = row()
            .key("inner")
            .w_full()
            .grow(1.0)
            .pl(6.0)
            .pr(8.0)
            .gap(8.0)
            .items_center()
            .child(toggle)
            .child(name);
        if let Some(lead) = self.d.header_lead.as_ref().and_then(|b| b(s, f)) {
            inner = inner.child(lead.key("lead"));
        }
        inner = inner.child(
            row()
                .key("stat")
                .gap(4.0)
                .child(text(format!("+{adds}")).font(l.small).color(l.add.count).key("adds"))
                .child(text(format!("−{dels}")).font(l.small).color(l.del.count).key("dels")),
        );
        if let Some((word, color)) = status {
            inner = inner.child(text(word).font(l.small_sans).color(color).key("status"));
        }
        if file.generated {
            inner = inner.child(text("Generated").font(l.small_sans).color(l.faint).key("generated"));
        }
        inner = match &self.d.header {
            Some(h) => inner.child(h(s, f).key("actions").grow(1.0).min_w(0.0)),
            None => inner.child(row().key("space").grow(1.0).a11y_hidden()),
        };
        let mut label = st.display_path(f);
        if let (FileStatus::Renamed, Some(a)) = (file.status, file.old_path.as_deref()) {
            label.push_str(&format!(", renamed from {a}"));
        }
        label.push_str(&format!(", {adds} added, {dels} removed"));
        let line = |key: &'static str, c: Color| col().key(key).w_full().h(1.0).fill(c).a11y_hidden();
        let mut h = col().w_full().h(l.m.header).fill(l.header_bg).role("heading").label(label);
        // In the list, as wide as the view and held to its left edge while the code scrolls
        // sideways, so the counts and actions stay in sight.
        if let (false, Some(w)) = (sticky, self.view_w(st)) {
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

/// The width `file`'s code wraps at, from the view's width: `None` until it is laid out.
fn wrap_width(st: &DiffState, file: usize) -> Option<f32> {
    let (w, n) = (st.view_width(), number_w(st.digits(file)));
    let gutter = match st.draw(file) {
        Draw::Split => return Some((w - 1.0) / 2.0 - MARKS - n - SIGN.1 - CODE_PAD.0 - CODE_PAD.1).filter(|_| w > 0.0),
        Draw::One(_) => MARKS + n + SIGN.0,
        Draw::Unified => MARKS + 2.0 * n + SIGN.0,
    };
    Some(w - gutter - CODE_PAD.0 - CODE_PAD.1).filter(|_| w > 0.0)
}

/// How far into a line `head` reaches, trailing spaces included.
fn x_in(head: &str, font: TextStyle, cell_w: f32) -> f32 {
    if head.len() <= 4096 {
        // A line's measured width leaves out trailing spaces, so measure past a mark.
        rune::text::advance(&format!("{head}|"), font) - rune::text::advance("|", font)
    } else {
        cell_w * head.chars().filter(|&c| c != '\u{200B}').count() as f32
    }
}

/// The wrapped line (wrapped at `w`) that byte `at` of `shown` sits on, and the byte that line
/// starts at, by counting columns as [`wrap_starts`] does rather than shaping the text.
fn wrapped_line(shown: &str, at: usize, w: f32, cell_w: f32) -> (usize, usize) {
    let cols = (w / cell_w + 0.01).max(1.0) as u32;
    let col = shown[..at].chars().filter(|&c| c != '\u{200B}').count() as u32;
    // A word's later characters decide whether it wraps, so scan the whole line.
    let starts = wrap_starts(shown, cols, usize::MAX);
    let line = starts.iter().rposition(|&s| s <= col).unwrap_or(0);
    let start = starts[line];
    let byte = shown
        .char_indices()
        .filter(|&(_, c)| c != '\u{200B}')
        .nth(start as usize)
        .map_or(at, |(b, _)| b.min(at));
    (line, byte)
}

/// The byte of row `r`'s code on `side` that a click `x` into its `line`th drawn line lands on,
/// by the drawn text's measured width (`measure`), so wide characters and tabs land where the
/// cursor is drawn.
fn clicked_byte(st: &DiffState, r: u32, side: Side, line: usize, x: f32, measure: impl Fn(&str) -> f32, cell_w: f32) -> usize {
    let Some(code) = st.row_text(r, side) else { return 0 };
    let wrap = st.wraps();
    let (shown, ..) = tabbed(code, Vec::new(), Vec::new(), wrap);
    let width = st.rows.get(r as usize).and_then(|row| wrap_width(st, row.file()));
    let (start, end) = match width.filter(|_| wrap) {
        Some(w) => {
            let starts = wrap_starts(&shown, (w / cell_w + 0.01).max(1.0) as u32, line + 2);
            let at = |col: u32| {
                shown
                    .char_indices()
                    .filter(|&(_, c)| c != '\u{200B}')
                    .nth(col as usize)
                    .map_or(shown.len(), |(b, _)| b)
            };
            let start = at(*starts.get(line).or(starts.last()).unwrap_or(&0));
            (start, starts.get(line + 1).map_or(shown.len(), |&n| at(n)))
        }
        None => (0, shown.len()),
    };
    let b = byte_at_x(&shown[start..end], x, measure) + start;
    // The char drawn at `b`: a tab's padding belongs to the tab.
    let col = crate::state::shown_to_col(code, wrap, b + 1).saturating_sub(1);
    crate::state::byte_of_col(code, col)
}

/// The start of the grapheme of `s` whose drawn width (`measure` of the text before it and
/// through it) covers `x`; past the end, the last grapheme. A binary search: `measure` grows with
/// the prefix.
fn byte_at_x(s: &str, x: f32, measure: impl Fn(&str) -> f32) -> usize {
    let mut bounds = vec![0];
    while let Some(&b) = bounds.last()
        && b < s.len()
    {
        bounds.push(crate::motion::next_grapheme(s, b));
    }
    if bounds.len() < 2 {
        return 0;
    }
    // The last grapheme start whose left edge is at or before `x`.
    let n = bounds[..bounds.len() - 1].partition_point(|&b| measure(&s[..b]) <= x);
    bounds[n.saturating_sub(1)]
}

/// Where the first `lines` lines of `shown` start, in columns, wrapped at `cols` as text layout
/// breaks it: after a space or a zero-width space, a word longer than a line left whole.
fn wrap_starts(shown: &str, cols: u32, lines: usize) -> Vec<u32> {
    let mut starts = vec![0];
    let (mut col, mut line, mut brk) = (0u32, 0u32, None::<u32>);
    for ch in shown.chars() {
        if starts.len() >= lines {
            break;
        }
        let zw = ch == '\u{200B}';
        if !zw
            && !ch.is_whitespace()
            && col - line >= cols
            && let Some(b) = brk.take().filter(|&b| b > line)
        {
            starts.push(b);
            line = b;
        }
        if !zw {
            col += 1;
        }
        if zw || ch.is_whitespace() {
            brk = Some(col);
        }
    }
    starts
}

/// A line as drawn: text, spans, inserted (uncopied) ranges and span marks, from `tabbed`.
type Drawn = (String, Vec<Span>, Vec<Range<usize>>, Vec<Range<usize>>);

/// A line as drawn with its marks lit: text, spans, uncopied ranges and highlights.
type Lit = (String, Vec<Span>, Vec<Range<usize>>, Vec<TextHighlight>);

/// The colours a line's marks paint in.
struct MarkPaint {
    span: Color,
    find: Color,
    selection: Color,
    text: Color,
    bg: Color,
}

/// Byte ranges of a line's code to mark: span comments, find hits, visual mode, the block.
struct LineMarks {
    spans: Vec<Range<usize>>,
    found: Vec<Range<usize>>,
    vis: Option<Range<usize>>,
    block: Option<Range<usize>>,
}

/// `code` as drawn (see [`tabbed`]) with its marks as highlights, painted in that order so the
/// block is on top: the text's colour behind the glyph, the glyph in the ground's. An empty
/// line's block sits on a space nothing copies.
fn drawn(code: &str, spans: Vec<Span>, m: LineMarks, wrap: bool, p: &MarkPaint) -> Lit {
    let n_spans = m.spans.len();
    let mut all: Vec<Range<usize>> = m.spans.into_iter().chain(m.found).collect();
    let n_found = all.len();
    let vis_at = m.vis.is_some().then_some(n_found);
    all.extend(m.vis);
    let block_at = m.block.is_some().then_some(all.len());
    all.extend(m.block);
    let (mut shown, spans, mut pads, mut all) = tabbed(code, spans, all, wrap);
    if code.is_empty()
        && let Some(b) = block_at.and_then(|i| all.get_mut(i))
    {
        shown.push(' ');
        pads.push(0..1);
        *b = 0..1;
    }
    let lit = all
        .into_iter()
        .enumerate()
        .map(|(i, r)| match i {
            _ if Some(i) == block_at => TextHighlight::new(r, p.text).color(p.bg).pad(0.0),
            _ if Some(i) == vis_at => TextHighlight::new(r, p.selection),
            _ if i < n_spans => TextHighlight::new(r, p.span),
            _ => TextHighlight::new(r, p.find),
        })
        .collect();
    (shown, spans, pads, lit)
}

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
    fn a_click_lands_on_the_grapheme_drawn_under_it() {
        let st = DiffState::new(vec![crate::testing::from_texts("a.txt", "", "a界界b\n\tx\n")]);
        // One cell a character, two for a wide one; a tab draws as one space before its padding.
        let measure = |s: &str| s.chars().map(|c| if c > '\u{2E80}' { 2.0 } else { 1.0 }).sum::<f32>();
        let click = |line: u32, x: f32| {
            let r = st.row_of(0, Side::New, line).unwrap();
            clicked_byte(&st, r, Side::New, 0, x, measure, 1.0)
        };
        assert_eq!(click(1, 0.5), 0);
        assert_eq!(click(1, 3.5), 4, "cell 3 is the second wide character");
        assert_eq!(click(1, 5.5), 7);
        assert_eq!(click(1, 40.0), 7, "past the end, the last character");
        assert_eq!(click(1, -3.0), 0);
        assert_eq!(click(2, 2.5), 0, "inside the tab's padding");
        assert_eq!(click(2, 4.2), 1);
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
    #[allow(clippy::single_range_in_vec_init)]
    fn the_block_inverts_one_grapheme_a_tabs_width_or_an_empty_lines_column() {
        let hue = |r, g, b| Color { r, g, b, a: 1.0 };
        let p = MarkPaint {
            span: hue(1.0, 0.0, 0.0),
            find: hue(0.0, 1.0, 0.0),
            selection: hue(0.0, 0.0, 1.0),
            text: Color::WHITE,
            bg: Color::BLACK,
        };
        let block = |r: Range<usize>| TextHighlight::new(r, Color::WHITE).color(Color::BLACK).pad(0.0);
        let marks = |vis: Option<Range<usize>>, b: Range<usize>| LineMarks {
            spans: Vec::new(),
            found: vec![0..1],
            vis,
            block: Some(b),
        };
        let (shown, _, _, lit) = drawn("ab\tc", Vec::new(), marks(None, 2..3), false, &p);
        assert_eq!(shown, "ab\t c");
        assert_eq!(lit.last(), Some(&block(2..4)), "the tab and its padding: one cell as drawn");
        let (_, _, _, lit) = drawn("xe\u{301}y", Vec::new(), marks(Some(0..4), 1..4), false, &p);
        assert_eq!(lit[0], TextHighlight::new(0..1, p.find));
        assert_eq!(lit[1], TextHighlight::new(0..4, p.selection));
        assert_eq!(lit[2], block(1..4), "the whole grapheme, on top of the selection");
        let (shown, _, pads, lit) = drawn("", Vec::new(), marks(None, 0..0), false, &p);
        assert_eq!((shown.as_str(), pads), (" ", vec![0..1]), "a space nothing copies");
        assert_eq!(lit.last(), Some(&block(0..1)));
        let (shown, _, pads, lit) = drawn(
            "",
            Vec::new(),
            LineMarks {
                spans: Vec::new(),
                found: Vec::new(),
                vis: None,
                block: None,
            },
            false,
            &p,
        );
        assert!(shown.is_empty() && pads.is_empty() && lit.is_empty(), "no block, nothing added");
    }

    #[test]
    fn wrapped_lines_start_after_spaces_and_breaks_and_long_words_stay_whole() {
        let all = usize::MAX;
        assert_eq!(wrap_starts("aaa bbb ccc", 5, all), vec![0, 4, 8]);
        assert_eq!(wrap_starts("aaa bbb ccc", 5, 2), vec![0, 4], "stops at the lines asked for");
        assert_eq!(wrap_starts("aaa bbb ccc", 8, all), vec![0, 8]);
        assert_eq!(wrap_starts("abcdefghij kl", 4, all), vec![0, 11]);
        assert_eq!(
            wrap_starts("a.\u{200B}bcde", 3, all),
            vec![0, 2],
            "a zero-width space breaks and takes no column"
        );
        assert_eq!(wrap_starts("short", 80, all), vec![0]);
    }

    #[test]
    fn the_block_finds_its_wrapped_line_and_where_that_line_starts() {
        let (w, cell) = (5.0 * 7.0, 7.0);
        assert_eq!(wrapped_line("aaa bbb ccc", 0, w, cell), (0, 0));
        assert_eq!(wrapped_line("aaa bbb ccc", 2, w, cell), (0, 0), "the word's last character");
        assert_eq!(wrapped_line("aaa bbb ccc", 4, w, cell), (1, 4), "bbb wrapped to the second line");
        assert_eq!(wrapped_line("aaa bbb ccc", 9, w, cell), (2, 8), "ccc on the third");
        assert_eq!(wrapped_line("short", 4, w * 4.0, cell), (0, 0), "no wrap at all");
        assert_eq!(
            wrapped_line("a.\u{200B}bcde", 6, 3.0 * 7.0, cell),
            (1, 5),
            "after a zero-width break"
        );
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
