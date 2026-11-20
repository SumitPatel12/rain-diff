use std::borrow::Cow;
use std::rc::Rc;

use rain_core::{FileStatus, FileTag};
use rune::{Color, El, GrowthSpec, Icon, Key, KeyEvent, ScrollSpec, Tx, col, el, icon, row, text};
use rune_kit::{Checkbox, Kit, TextInput, TreeItem, TreeKey, TreeStyle, glyphs};

use crate::file_icons::{FileIcons, REPO, REPO_OPEN};
use crate::model::{FileEntry, NodeKind, TREE_KEY, TreeFilter, TreeState};

pub static COMMENT: Icon = Icon {
    name: "rain-comment",
    view: 16.0,
    body: r#"<path d="M2.5 3.5H13.5V10.5H6.5L3.5 13V10.5H2.5V3.5Z" stroke="currentColor" stroke-linejoin="round"/>"#,
};

/// Keeps a row's icon place for the full-colour icon drawn over it.
static SLOT: Icon = Icon {
    name: "rain-icon-slot",
    view: 16.0,
    body: "",
};

const ROW_HEIGHT: f32 = 24.0;
const ICON: f32 = 16.0;

type Get<S> = fn(&S) -> &TreeState;
type GetMut<S> = fn(&mut S) -> &mut TreeState;
type GetIcons<S> = fn(&S) -> &FileIcons;
type KeyFn<S> = Rc<dyn Fn(&mut S, &str, &mut Tx)>;
type ViewedFn<S> = Rc<dyn Fn(&mut S, &str, bool, &mut Tx)>;
type TagFn<S> = Rc<dyn Fn(&mut S, FileTag, &mut Tx)>;
type FilterFn<S> = Rc<dyn Fn(&mut S, &TreeFilter, &mut Tx)>;

/// The file tree panel over a [`TreeState`] in the app's state `S`: progress, a filter box,
/// repo chips (with more than one repo), tag chips, a group bar when one tag is chosen, and the virtualised rows, each with a viewed
/// checkbox first.
///
/// Callbacks run after the [`TreeState`] has changed: `on_viewed` once per file whose mark
/// changed (also for folder and group marks), so the app persists from that one place.
pub struct TreeView<S> {
    get: Get<S>,
    get_mut: GetMut<S>,
    on_open: Option<KeyFn<S>>,
    on_select: Option<KeyFn<S>>,
    on_viewed: Option<ViewedFn<S>>,
    on_mark_group_viewed: Option<TagFn<S>>,
    on_filter_change: Option<FilterFn<S>>,
    icons: Option<GetIcons<S>>,
    header: bool,
    rows: bool,
    progress: bool,
    filter_box: bool,
    chips: bool,
}

/// The callbacks, shared by the header, the rows and the key handler.
struct Hooks<S> {
    get: Get<S>,
    get_mut: GetMut<S>,
    on_open: Option<KeyFn<S>>,
    on_select: Option<KeyFn<S>>,
    on_viewed: Option<ViewedFn<S>>,
    on_mark_group_viewed: Option<TagFn<S>>,
    on_filter_change: Option<FilterFn<S>>,
}

impl<S: 'static> Hooks<S> {
    fn viewed(&self, s: &mut S, changed: Vec<String>, on: bool, tx: &mut Tx) {
        if let Some(f) = &self.on_viewed {
            for key in changed {
                f(s, &key, on, tx);
            }
        }
    }

    /// Toggles the viewed mark of a file, or of every shown file under a folder or repo.
    fn toggle_node(&self, s: &mut S, id: &str, tx: &mut Tx) {
        let st = (self.get_mut)(s);
        let Some(node) = st.model.get(id) else { return };
        let (changed, on) = match node.kind {
            NodeKind::File(i) => {
                let on = !st.files[i as usize].viewed;
                (if st.set_viewed(id, on) { vec![id.to_owned()] } else { Vec::new() }, on)
            }
            NodeKind::Dir | NodeKind::Repo(_) => {
                let (v, t) = st.folder_counts(id);
                let on = v < t;
                (st.set_viewed_under(id, on), on)
            }
        };
        self.viewed(s, changed, on, tx);
    }

    fn open(&self, s: &mut S, id: &str, tx: &mut Tx) {
        if let Some(f) = &self.on_open {
            f(s, id, tx);
        }
    }

    fn filter_changed(&self, s: &mut S, tx: &mut Tx) {
        if let Some(f) = &self.on_filter_change {
            let filter = (self.get)(s).filter.clone();
            f(s, &filter, tx);
        }
    }

    fn set_filter(&self, s: &mut S, tx: &mut Tx, change: impl FnOnce(&mut TreeFilter)) {
        let st = (self.get_mut)(s);
        let mut filter = st.filter.clone();
        change(&mut filter);
        st.apply(&filter);
        self.filter_changed(s, tx);
    }

    fn mark_group(&self, s: &mut S, tag: FileTag, tx: &mut Tx) {
        let changed = (self.get_mut)(s).mark_tag_viewed(tag);
        self.viewed(s, changed, true, tx);
        if let Some(f) = &self.on_mark_group_viewed {
            f(s, tag, tx);
        }
    }

    fn key(&self, s: &mut S, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let m = ev.mods;
        if m.ctrl || m.alt || m.cmd || m.logo {
            return false;
        }
        let st = (self.get_mut)(s);
        let plain = |k: Key| KeyEvent {
            key: k,
            mods: Default::default(),
        };
        let before = st.model.cursor().map(str::to_owned);
        let out = match &ev.key {
            Key::Text(t) if t == " " || t == "v" => {
                if tx.key_repeat() {
                    return true;
                }
                if let Some(id) = before {
                    self.toggle_node(s, &id, tx);
                }
                return true;
            }
            Key::Text(t) if t == "f" => {
                st.filter_edit.focused = true;
                return true;
            }
            Key::Text(t) if t == "j" => st.model.handle_key(&plain(Key::Down), |n| n.label(), tx),
            Key::Text(t) if t == "k" => st.model.handle_key(&plain(Key::Up), |n| n.label(), tx),
            // Letters stay with the app's keymap rather than type-selecting rows.
            Key::Text(_) => return false,
            Key::PageUp | Key::PageDown => {
                let step = if ev.key == Key::PageUp { Key::Up } else { Key::Down };
                let page = ((st.viewport / ROW_HEIGHT).floor() as usize).max(2) - 1;
                let mut out = TreeKey::Ignored;
                for _ in 0..page {
                    out = st.model.handle_key(&plain(step.clone()), |n| n.label(), tx);
                }
                out
            }
            _ => st.model.handle_key(ev, |n| n.label(), tx),
        };
        match out {
            TreeKey::Ignored => false,
            TreeKey::Handled => {
                let after = (self.get)(s).model.cursor().map(str::to_owned);
                if after != before
                    && let (Some(f), Some(id)) = (&self.on_select, after)
                {
                    f(s, &id, tx);
                }
                true
            }
            TreeKey::Open(id) => {
                self.open(s, &id, tx);
                true
            }
        }
    }
}

impl<S: 'static> TreeView<S> {
    pub fn new(get: Get<S>, get_mut: GetMut<S>) -> Self {
        TreeView {
            get,
            get_mut,
            on_open: None,
            on_select: None,
            on_viewed: None,
            on_mark_group_viewed: None,
            on_filter_change: None,
            icons: None,
            header: true,
            rows: true,
            progress: true,
            filter_box: true,
            chips: true,
        }
    }

    /// The header without its chips, for a list that offers them elsewhere (a popover).
    pub fn without_chips(mut self) -> Self {
        self.chips = false;
        self
    }

    /// Only the repo and tag chips and the toggles, without the filter box or rows.
    pub fn chips_only(mut self) -> Self {
        self.rows = false;
        self.filter_box = false;
        self
    }

    /// Only the filter box and chips, for a list that draws its own rows.
    pub fn header_only(mut self) -> Self {
        self.rows = false;
        self
    }

    /// Only the rows, under a filter drawn elsewhere.
    pub fn rows_only(mut self) -> Self {
        self.header = false;
        self
    }

    /// Hides the viewed counts above the filter box.
    pub fn without_progress(mut self) -> Self {
        self.progress = false;
        self
    }

    /// A file row was clicked, or Return pressed on it: show that file. Gets its `repoId/path`.
    pub fn on_open(mut self, f: impl Fn(&mut S, &str, &mut Tx) + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    /// The keyboard cursor moved to a row (file, folder or repo id).
    pub fn on_select(mut self, f: impl Fn(&mut S, &str, &mut Tx) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// A file's viewed mark changed (checkbox, Space, a folder's checkbox or a group mark).
    pub fn on_viewed(mut self, f: impl Fn(&mut S, &str, bool, &mut Tx) + 'static) -> Self {
        self.on_viewed = Some(Rc::new(f));
        self
    }

    /// "Mark all viewed" on the group bar, after `on_viewed` has run for each file it changed.
    pub fn on_mark_group_viewed(mut self, f: impl Fn(&mut S, FileTag, &mut Tx) + 'static) -> Self {
        self.on_mark_group_viewed = Some(Rc::new(f));
        self
    }

    /// The filter changed (chip, toggle or text), after it was applied.
    pub fn on_filter_change(mut self, f: impl Fn(&mut S, &TreeFilter, &mut Tx) + 'static) -> Self {
        self.on_filter_change = Some(Rc::new(f));
        self
    }

    /// Draws file and folder rows with the window's [`FileIcons`]; without them the place is blank.
    pub fn file_icons(mut self, f: GetIcons<S>) -> Self {
        self.icons = Some(f);
        self
    }

    /// The panel, filling the height its parent gives it.
    pub fn build(self, s: &S, k: &Kit) -> El {
        let hooks = Rc::new(Hooks {
            get: self.get,
            get_mut: self.get_mut,
            on_open: self.on_open,
            on_select: self.on_select,
            on_viewed: self.on_viewed,
            on_mark_group_viewed: self.on_mark_group_viewed,
            on_filter_change: self.on_filter_change,
        });
        let st = (hooks.get)(s);
        let mut panel = col().key("tree-panel").w_full().min_h(0.0);
        if self.rows {
            panel = panel.h_full();
        }
        if self.header {
            if self.filter_box {
                panel = panel.child(header(st, k, &hooks, self.progress));
            }
            if self.chips {
                // Alone (a popover), the toggles read as a checklist.
                let checklist = !self.filter_box && !self.rows;
                if st.repos().len() > 1 {
                    panel = panel.child(repo_chips(st, k, &hooks, checklist));
                }
                panel = panel.child(chips(st, k, &hooks, checklist));
            }
            if self.rows
                && let Some(tag) = st.filter.single_tag()
            {
                panel = panel.child(group_bar(st, tag, k, &hooks));
            }
        }
        if self.rows {
            panel = panel.child(rows(st, k, &hooks, self.icons));
        }
        panel
    }
}

/// A tag's short row label, its colour, and its chip key.
pub fn tag_short(t: FileTag) -> &'static str {
    match t {
        FileTag::MustReview => "Must",
        FileTag::Review => "Review",
        FileTag::Skim => "Skim",
        FileTag::FormattingOnly => "Format",
        FileTag::Generated => "Gen",
        FileTag::Ignored => "Ignored",
        FileTag::Uncovered => "Uncovered",
    }
}

pub fn tag_slug(t: FileTag) -> &'static str {
    match t {
        FileTag::MustReview => "must-review",
        FileTag::Review => "review",
        FileTag::Skim => "skim",
        FileTag::FormattingOnly => "formatting-only",
        FileTag::Generated => "generated",
        FileTag::Ignored => "ignored",
        FileTag::Uncovered => "uncovered",
    }
}

fn tag_color(t: FileTag, k: &Kit) -> Color {
    match t {
        FileTag::MustReview => k.c.danger.icon,
        FileTag::Review => k.c.warning.icon,
        FileTag::Skim => k.c.info.icon,
        FileTag::Uncovered => k.c.text_muted,
        FileTag::FormattingOnly | FileTag::Generated | FileTag::Ignored => k.c.text_faint,
    }
}

fn status_letter(s: FileStatus) -> (&'static str, &'static str) {
    match s {
        FileStatus::Modified => ("M", "Modified"),
        FileStatus::Added => ("A", "Added"),
        FileStatus::Deleted => ("D", "Deleted"),
        FileStatus::Renamed => ("R", "Renamed"),
        FileStatus::Copied => ("C", "Copied"),
    }
}

fn status_color(s: FileStatus, k: &Kit) -> Color {
    match s {
        FileStatus::Added => k.c.success.icon,
        FileStatus::Deleted => k.c.danger.icon,
        FileStatus::Renamed | FileStatus::Copied => k.c.info.icon,
        FileStatus::Modified => k.c.text_muted,
    }
}

fn repo_color(r: u16, k: &Kit) -> Color {
    match r % 4 {
        0 => k.c.icon_accent,
        1 => k.c.success.icon,
        2 => k.c.warning.icon,
        _ => k.c.info.icon,
    }
}

fn header<S: 'static>(st: &TreeState, k: &Kit, hooks: &Rc<Hooks<S>>, with_progress: bool) -> El {
    let p = st.progress();
    let summary = format!("{}/{} viewed", p.viewed, p.total);
    let must = format!("Must review {}/{}", p.must_viewed, p.must_total);
    let progress = row()
        .key("progress")
        .role("status")
        .label(format!("{summary}, {must}"))
        .w_full()
        .gap(12.0)
        .center_y()
        .child(text(summary).font(k.ty.text_small_medium).color(k.c.text_base).a11y_hidden())
        .child(
            text(must)
                .font(k.ty.text_small)
                .color(if p.must_viewed < p.must_total {
                    k.c.danger.fg
                } else {
                    k.c.text_muted
                })
                .a11y_hidden(),
        );
    let (h1, h2, h3) = (hooks.clone(), hooks.clone(), hooks.clone());
    let get_mut = hooks.get_mut;
    let input = TextInput::new(&st.filter_edit)
        .label("Filter files")
        .placeholder("Filter files")
        .leading_icon(&glyphs::SEARCH)
        .width(None)
        .clear_button()
        .on_edit(move |s: &mut S| &mut get_mut(s).filter_edit)
        .on_change(move |s: &mut S, tx: &mut Tx| {
            let text = (h1.get)(s).filter_edit.text.clone();
            h1.set_filter(s, tx, |f| f.text = text);
        })
        .on_action(move |s: &mut S, tx: &mut Tx| h2.set_filter(s, tx, |f| f.text.clear()))
        .on_key(move |s: &mut S, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key != Key::Escape {
                return false;
            }
            h3.set_filter(s, tx, |f| f.text.clear());
            tx.blur();
            true
        })
        .build(k)
        .key("filter");
    let mut c = col().key("tree-header").w_full().pl(10.0).pr(6.0).pt(10.0).gap(6.0);
    if with_progress {
        c = c.child(progress);
    }
    c.child(input)
}

/// One filter toggle: a chip in the tree's wrap, or in a checklist a row with a box, the label,
/// then (the caller's) count at the end.
#[allow(clippy::too_many_arguments)]
fn chip<S: 'static>(
    key: String,
    label: String,
    said: String,
    on: bool,
    k: &Kit,
    press: impl Fn(&mut S, &mut Tx) + 'static,
    parts: Vec<El>,
    checklist: bool,
) -> El {
    let c = &k.c;
    if !checklist {
        let e = row()
            .key(key)
            .role("toggle")
            .label(said)
            .checked(on)
            .focusable(true)
            .h(22.0)
            .px(6.0)
            .gap(4.0)
            .center_y()
            .radius(k.radius.sm)
            .border(1.0, if on { c.border_focus } else { c.border_weak })
            .fill(if on { k.alpha(c.text_accent, 0.18) } else { Color::TRANSPARENT })
            .focus_ring(1.0, 1.0, c.border_active)
            .children(parts)
            .child(text(label).font(k.ty.text_xsmall_medium).color(c.text_base).a11y_hidden())
            .on_press_tx(press);
        return k.interactive(e);
    }
    let mut boxed = row().key("box").size(14.0, 14.0).center().radius(3.0).a11y_hidden();
    boxed = if on {
        boxed.fill(c.bg_accent).child(icon(&CHECK, 12.0).color(c.submit_fg))
    } else {
        boxed.border(1.5, c.text_muted)
    };
    let e = row()
        .key(key)
        .role("toggle")
        .label(said)
        .checked(on)
        .focusable(true)
        .w_full()
        .h(26.0)
        .px(8.0)
        .gap(10.0)
        .center_y()
        .radius(5.0)
        .focus_ring(1.0, 1.0, c.border_active)
        .child(boxed)
        .child(text(label).font(k.ty.ui).color(c.text_base).a11y_hidden())
        .child(el().grow(1.0).a11y_hidden())
        .on_press_tx(press);
    k.interactive(e)
}

static CHECK: rune::Icon = rune::Icon {
    name: "tree-check",
    view: 16.0,
    body: r#"<path d="m3.5 8.5 3 3 6-7" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// A small label over a group of toggles.
fn group_label(t: &str, k: &Kit) -> El {
    text(t.to_owned()).font(k.ty.small).color(k.c.text_faint).px(8.0).pt(6.0).pb(2.0)
}

fn repo_chips<S: 'static>(st: &TreeState, k: &Kit, hooks: &Rc<Hooks<S>>, checklist: bool) -> El {
    let mut wrap = if checklist {
        col().key("repo-chips").w_full().px(4.0).child(group_label("Repo", k))
    } else {
        row().key("repo-chips").w_full().px(8.0).pt(6.0).flex_wrap().gaps(4.0, 4.0)
    };
    for (r, name) in st.repos() {
        let r = *r;
        let n = st.repo_count(r);
        let on = st.filter.repos.contains(&r);
        if n == 0 && !on {
            continue;
        }
        let h = hooks.clone();
        let dot = el().size(8.0, 8.0).radius(4.0).fill(repo_color(r, k)).self_center().a11y_hidden();
        let count = text(n.to_string()).font(k.ty.small.mono()).color(k.c.text_faint).a11y_hidden();
        let press = move |s: &mut S, tx: &mut Tx| {
            h.set_filter(s, tx, |f| {
                if !f.repos.remove(&r) {
                    f.repos.insert(r);
                }
            })
        };
        let said = format!("{name}, {n} files");
        let e = chip(format!("chip-repo-{r}"), name.clone(), said, on, k, press, vec![dot], checklist);
        wrap = wrap.child(e.child(count));
    }
    wrap
}

fn chips<S: 'static>(st: &TreeState, k: &Kit, hooks: &Rc<Hooks<S>>, checklist: bool) -> El {
    let mut wrap = if checklist {
        col().key("chips").w_full().px(4.0).pb(4.0).child(group_label("Tag", k))
    } else {
        row().key("chips").w_full().px(8.0).py(6.0).flex_wrap().gaps(4.0, 4.0)
    };
    for t in FileTag::ALL {
        let n = st.tag_count(t);
        let on = st.filter.tags.contains(&t);
        if n == 0 && !on {
            continue;
        }
        let h = hooks.clone();
        let glyph = text(t.glyph())
            .font(k.ty.text_xsmall_medium.mono())
            .color(tag_color(t, k))
            .a11y_hidden();
        let count = text(n.to_string()).font(k.ty.small.mono()).color(k.c.text_faint).a11y_hidden();
        let press = move |s: &mut S, tx: &mut Tx| {
            h.set_filter(s, tx, |f| {
                if !f.tags.remove(&t) {
                    f.tags.insert(t);
                }
            })
        };
        let said = format!("{}, {n} files", t.label());
        let label = t.label().to_owned();
        let e = chip(format!("chip-{}", tag_slug(t)), label, said, on, k, press, vec![glyph], checklist);
        wrap = wrap.child(e.child(count));
    }
    let (h1, h2) = (hooks.clone(), hooks.clone());
    let f = &st.filter;
    if checklist {
        wrap = wrap.child(el().w_full().h(1.0).my(4.0).fill(k.c.border_weak).a11y_hidden());
    }
    wrap = wrap.child(chip(
        "chip-unviewed".into(),
        "Unviewed".into(),
        "Unviewed only".into(),
        f.unviewed_only,
        k,
        move |s: &mut S, tx: &mut Tx| h1.set_filter(s, tx, |f| f.unviewed_only = !f.unviewed_only),
        vec![icon(&glyphs::EYE_OFF, 14.0).color(k.c.icon_muted).a11y_hidden()],
        checklist,
    ));
    wrap.child(chip(
        "chip-commented".into(),
        "Comments".into(),
        "Has comments only".into(),
        f.commented_only,
        k,
        move |s: &mut S, tx: &mut Tx| h2.set_filter(s, tx, |f| f.commented_only = !f.commented_only),
        vec![icon(&COMMENT, 14.0).color(k.c.icon_muted).a11y_hidden()],
        checklist,
    ))
}

fn group_bar<S: 'static>(st: &TreeState, tag: FileTag, k: &Kit, hooks: &Rc<Hooks<S>>) -> El {
    let (v, t) = st.tag_progress(tag);
    let h = hooks.clone();
    let done = v == t;
    let mut button = row()
        .key("mark-group")
        .role("button")
        .label(format!("Mark all {} viewed", t))
        .h(22.0)
        .px(8.0)
        .center_y()
        .radius(k.radius.sm)
        .border(1.0, k.c.border_base)
        .focusable(!done)
        .disabled(done)
        .child(
            text("Mark all viewed")
                .font(k.ty.text_xsmall_medium)
                .color(if done { k.c.text_faint } else { k.c.text_base })
                .a11y_hidden(),
        );
    if !done {
        button = k.interactive(button.on_press_tx(move |s: &mut S, tx: &mut Tx| h.mark_group(s, tag, tx)));
    }
    row()
        .key("group-bar")
        .w_full()
        .px(8.0)
        .py(4.0)
        .gap(8.0)
        .center_y()
        .fill(k.c.surface_weak)
        .child(text(tag.glyph()).font(k.ty.text_small_medium.mono()).color(tag_color(tag, k)))
        .child(
            text(format!("{}  {v}/{t} viewed", tag.label()))
                .font(k.ty.text_small_medium)
                .color(k.c.text_base)
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .truncate(),
        )
        .child(button)
}

fn file_item(e: &FileEntry, highlight: &str, k: &Kit) -> (TreeItem, String) {
    let c = &k.c;
    let (_, status) = status_letter(e.status);
    let mut said = vec![status.to_owned()];
    if let Some(old) = &e.old_path {
        said.push(format!("from {old}"));
    }
    if e.additions > 0 {
        said.push(format!("{} added", e.additions));
    }
    if e.deletions > 0 {
        said.push(format!("{} removed", e.deletions));
    }
    if e.comments > 0 {
        said.push(format!("{} comments", e.comments));
    }
    said.push(e.tag.label().to_owned());
    if e.viewed {
        said.push("viewed".into());
    }
    let dim = e.generated || e.viewed || matches!(e.tag, FileTag::Ignored | FileTag::Generated);
    let mut item = TreeItem::new(e.name.clone())
        .icon(&SLOT)
        .highlight(highlight)
        .changes(e.additions, e.deletions);
    if dim {
        item = item.label_color(c.text_faint);
    }
    if e.comments > 0 {
        item = item.trailing(
            row()
                .key("comments")
                .gap(2.0)
                .center_y()
                .a11y_hidden()
                .child(icon(&COMMENT, 14.0).color(c.icon_accent))
                .child(text(e.comments.to_string()).font(k.ty.text_xsmall_medium).color(c.text_accent)),
        );
    }
    (item, said.join(", "))
}

/// One letter in a small tinted box, its meaning on hover.
fn letter_tag(key: &'static str, letter: &'static str, ink: Color, help: &str, k: &Kit) -> El {
    row()
        .key(key)
        .size(16.0, 16.0)
        .center()
        .radius(3.0)
        .fill(k.alpha(ink, 0.16))
        .border(1.0, k.alpha(ink, 0.45))
        .help(help.to_owned())
        .child(
            text(letter)
                .font(rune::tok::TextStyle::new(10.0, 700.0).lh(14.0))
                .color(ink)
                .a11y_hidden(),
        )
}

/// The review tag's letter and colour, as the grouped list marks the same files.
fn review_letter(t: FileTag, k: &Kit) -> (&'static str, Color) {
    match t {
        FileTag::MustReview => ("M", k.c.danger.fg),
        FileTag::Review => ("R", k.c.info.fg),
        FileTag::Skim => ("S", k.c.text_muted),
        FileTag::Uncovered => ("U", k.c.warning.fg),
        FileTag::FormattingOnly => ("F", k.c.warning.fg),
        FileTag::Generated => ("G", k.c.text_faint),
        FileTag::Ignored => ("I", k.c.text_faint),
    }
}

/// A file row's two tags at its far left: how to review it, then what git did to it. A folder
/// row gets the same width empty, so every name starts in one column.
fn lead_tags(e: Option<&FileEntry>, k: &Kit) -> El {
    let mut r = row().key("tags").w(36.0).gap(4.0).center_y().a11y_hidden();
    if let Some(e) = e {
        let (letter, ink) = review_letter(e.tag, k);
        let (sl, status) = status_letter(e.status);
        r = r
            .child(letter_tag("tag", letter, ink, e.tag.label(), k))
            .child(letter_tag("status", sl, status_color(e.status, k), status, k));
    }
    r
}

fn rows<S: 'static>(st: &TreeState, k: &Kit, hooks: &Rc<Hooks<S>>, icons: Option<GetIcons<S>>) -> El {
    let style = TreeStyle {
        guides: false,
        row_height: ROW_HEIGHT,
        ..TreeStyle::from_kit(k)
    };
    let get = hooks.get;
    let kit = Rc::new(k.clone());
    let h = hooks.clone();
    let build_row = move |s: &S, i: usize| -> El {
        let st = get(s);
        let Some((node, r)) = st.model.row(i) else { return el() };
        let id = r.id.to_owned();
        let highlight = st.filter.text.split_whitespace().next().unwrap_or("");
        let (checkbox, item, said) = match node.kind {
            NodeKind::File(fi) => {
                let e = &st.files[fi as usize];
                let (item, said) = file_item(e, highlight, &kit);
                (Checkbox::new(e.viewed).label(format!("Viewed {}", e.name)), item, said)
            }
            NodeKind::Dir | NodeKind::Repo(_) => {
                let (v, t) = st.folder_counts(&id);
                let mut item = TreeItem::new(node.name.clone());
                item = match node.kind {
                    NodeKind::Repo(ri) => item
                        .icon(if r.expanded == Some(true) { &REPO_OPEN } else { &REPO })
                        .icon_color(repo_color(ri, &kit)),
                    _ => item.icon(&SLOT),
                };
                item = item.trailing(
                    text(format!("{v}/{t}"))
                        .key("count")
                        .font(kit.ty.text_xsmall.features("'tnum' 1"))
                        .color(kit.c.text_faint)
                        .a11y_hidden(),
                );
                let kind = if matches!(node.kind, NodeKind::Repo(_)) {
                    "repository"
                } else {
                    "folder"
                };
                let partial = v > 0 && v < t;
                let mut cb = Checkbox::new(t > 0 && v == t)
                    .indeterminate(partial)
                    .label(format!("Viewed all in {}", node.name));
                if partial {
                    cb = cb.icon(kit.glyph.dash);
                }
                (cb, item, format!("{kind}, {v} of {t} files viewed"))
            }
        };
        let (h1, h2) = (h.clone(), h.clone());
        let (toggle_id, press_id) = (id.clone(), id.clone());
        let file = matches!(node.kind, NodeKind::File(_));
        let openable = r.expanded.is_some();
        let picture = icons.and_then(|f| {
            let icons = f(s);
            match node.kind {
                NodeKind::File(fi) => Some(icons.file(&st.files[fi as usize].name, ICON)),
                NodeKind::Dir => Some(icons.folder(r.expanded == Some(true), ICON)),
                NodeKind::Repo(_) => None,
            }
        });
        // Over the slot TreeItem keeps after the indent and the 16 point chevron, 4 points apart.
        let icon_at = [
            style.padding + r.depth as f32 * style.indent + 16.0 + 4.0,
            (style.row_height - ICON) / 2.0,
        ];
        let checkbox = checkbox
            .hide_label()
            .on_toggle_tx(move |s: &mut S, tx: &mut Tx| h1.toggle_node(s, &toggle_id, tx))
            .build(&kit)
            .key("viewed")
            .focusable(false)
            .self_center();
        let mut item = item.build(&kit, &r, &style);
        if let Some(p) = picture {
            item = item.child(p.absolute(icon_at[0], icon_at[1]));
        }
        let item = item
            .key("item")
            .description(said)
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .on_press_tx(move |s: &mut S, tx: &mut Tx| {
                let m = &mut (h2.get_mut)(s).model;
                let plain = m.click(&press_id, tx.mods());
                if openable && plain {
                    m.toggle(&press_id);
                }
                if let Some(f) = &h2.on_select {
                    f(s, &press_id, tx);
                }
                if file {
                    h2.open(s, &press_id, tx);
                }
            });
        let entry = match node.kind {
            NodeKind::File(fi) => st.files.get(fi as usize),
            _ => None,
        };
        row()
            .w_full()
            .h(style.row_height)
            .pl(6.0)
            .gap(4.0)
            .center_y()
            .child(checkbox)
            .child(lead_tags(entry, &kit))
            .child(item)
    };
    let count = st.model.visible_len();
    let rows = col().key("rows").w_full().growth(GrowthSpec::default()).virtual_rows(
        count,
        style.row_height,
        move |s: &S, i| Cow::Borrowed(get(s).model.visible_id(i).unwrap_or_default()),
        build_row,
    );
    let get_mut = hooks.get_mut;
    let viewport = col()
        .key("viewport")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .on_layout_tx(move |s: &mut S, r: [f32; 4], tx: &mut Tx| {
            let st = get_mut(s);
            if st.viewport == r[3] {
                tx.live_only();
            }
            st.viewport = r[3];
        })
        .child(rows);
    let mut root = col()
        .key(TREE_KEY)
        .role("tree")
        .label("Changed files")
        .focusable(true)
        .w_full()
        .grow(1.0)
        .min_h(0.0);
    if let Some(id) = st.model.cursor() {
        root = root.active_descendant(id.to_owned());
    }
    let h = hooks.clone();
    root.on_key(move |s: &mut S, ev: &KeyEvent, tx: &mut Tx| h.key(s, ev, tx))
        .child(viewport)
}
