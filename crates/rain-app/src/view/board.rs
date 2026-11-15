//! The list: the filter box and chips, then one virtualised column of collapsible sections in
//! triage order, the folded groups with their Accept pinned under it, the accept snackbar, and
//! Waiting on you pinned last. Also file-at-a-time's
//! cards (landing and group) for the reader.

use std::borrow::Cow;
use std::rc::Rc;

use rain_comments::{anchor_label, ops};
use rain_core::Thread;
use rain_tree::{TreeFilter, TreeView};
use rain_triage::Group;
use rune::{Color, El, GrowthSpec, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, SidebarHeader};

use super::{lines_label, list_row, messages, muted};
use crate::app::board::{CHANGE_GROUPS, CHANGES, Listing, Pick, Row, WAITING, change_title, section_group, section_of, section_title};
use crate::app::{App, ME};
use crate::theme::Look;

/// One list row: section headers and files alike.
pub(crate) const ROW_HEIGHT: f32 = 28.0;

/// `CHANGES` as `Changes`, `WAITING ON YOU` as `Waiting on you`.
fn sentence(title: &str) -> String {
    let lower = title.to_lowercase();
    let mut c = lower.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// What a list row needs of the look, shared by the virtual rows.
struct RowLook {
    kit: Kit,
    selected: Color,
}

pub(crate) fn group_color(g: Group, k: &Kit) -> Color {
    match g {
        Group::Must => k.c.danger.icon,
        Group::Review => k.c.warning.icon,
        Group::Skim => k.c.info.icon,
        _ => k.c.text_faint,
    }
}

fn slug(g: Group) -> String {
    g.label().to_lowercase()
}

pub(super) fn board(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let mut c = col()
        .key("board")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(filter_header(s, k))
        .child(body(s, look));
    let listing = s.listing();
    if !listing.footer.is_empty() {
        c = c.child(
            col()
                .key("list-groups")
                .role("group")
                .label("Folded groups")
                .w_full()
                .child(super::hairline(look.s.hairline))
                .children(listing.footer.iter().map(|&sec| header(s, sec, &listing, k))),
        );
    }
    if let Some(msg) = s.accept_snack() {
        c = c.child(row().w_full().px(6.0).py(4.0).child(messages::snackbar(
            "accept-snack",
            msg.to_owned(),
            Some(("Undo (u)", |s: &mut App, tx: &mut Tx| s.undo_accept(tx))),
            k,
        )));
    }
    waiting(s, c, k)
}

/// The filter box, with the chips (repo, tag, unviewed, comments) behind its `⚲` button; they
/// narrow every section and the reader.
fn filter_header(s: &App, k: &Kit) -> El {
    let input = TreeView::new(|s: &App| &s.tree, |s: &mut App| &mut s.tree)
        .header_only()
        .without_progress()
        .without_chips()
        .on_filter_change(|s: &mut App, f: &TreeFilter, tx: &mut Tx| s.on_filter_change(f, tx))
        .build(s, k)
        .key("list-filter")
        .grow(1.0)
        .min_w(0.0);
    row()
        .key("list-filter-row")
        .w_full()
        .items_end()
        .pr(8.0)
        .pb(6.0)
        .child(input)
        .child(super::filter::chips_button(s, k))
}

fn body(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let listing = s.listing();
    if s.board.folders && !s.section_folded(CHANGES) {
        return col()
            .key("list-folders")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .child(header(s, CHANGES, &listing, k))
            .child(
                col().key("list-files").w_full().grow(1.0).min_h(0.0).child(
                    TreeView::new(|s: &App| &s.tree, |s: &mut App| &mut s.tree)
                        .rows_only()
                        .on_open(|s: &mut App, key: &str, tx: &mut Tx| s.goto_key(key, tx))
                        .on_viewed(|s: &mut App, key: &str, on, _: &mut Tx| s.persist_viewed_key(key, on))
                        .on_filter_change(|s: &mut App, f: &TreeFilter, tx: &mut Tx| s.on_filter_change(f, tx))
                        .build(s, k),
                ),
            );
    }
    let kit = Rc::new(RowLook {
        kit: k.clone(),
        selected: look.s.selected_row,
    });
    let n = listing.rows.len();
    let rows = col()
        .key("list-files")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            n,
            ROW_HEIGHT,
            |s: &App, i| Cow::Owned(row_key(s, i)),
            move |s: &App, i| build_row(s, i, &kit),
        )
        .virtual_rows_version(listing.version);
    col()
        .key("list-scroll")
        .role("list")
        .label("Changed files by group")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(rows)
}

fn row_key(s: &App, i: usize) -> String {
    match s.listing().rows.get(i) {
        Some(Row::Header(sec)) => format!("list-section-{sec}"),
        Some(Row::Sub(g)) => format!("list-sub-{}", slug(*g)),
        Some(Row::File(g)) => format!("file-{}", s.keys[*g]),
        Some(Row::Accepted(i)) => format!("done-{i}"),
        None => format!("row-{i}"),
    }
}

fn build_row(s: &App, i: usize, look: &RowLook) -> El {
    let listing = s.listing();
    let key = row_key(s, i);
    let k = &look.kit;
    match listing.rows.get(i).cloned() {
        Some(Row::Header(sec)) => header(s, sec, &listing, k),
        Some(Row::Sub(g)) => sub_header(g, &listing, key, k),
        Some(Row::File(g)) => file_row(s, g, key, look),
        Some(Row::Accepted(a)) => accepted_row(s, a, key, k),
        None => row().key(key).h(ROW_HEIGHT),
    }
}

/// `Must review  17` over the change group's files: a label, not a fold.
fn sub_header(g: Group, l: &Listing, key: String, k: &Kit) -> El {
    let n = CHANGE_GROUPS.iter().position(|&x| x == g).map_or(0, |i| l.changes[i]);
    let name = change_title(g);
    let color = if g == Group::Must { k.c.danger.fg } else { k.c.text_muted };
    row()
        .key(key)
        .role("heading")
        .label(format!("{name}, {n} files"))
        .w_full()
        .h(ROW_HEIGHT)
        .pl(12.0)
        .pr(12.0)
        .pt(6.0)
        .gap(6.0)
        .items_center()
        .child(text(name).font(k.ty.small_medium).color(color).a11y_hidden())
        .child(
            text(n.to_string())
                .font(k.ty.small.features("'tnum' 1"))
                .color(k.c.text_faint)
                .a11y_hidden(),
        )
}

/// `Name  count ▾  [action]`; a press on it folds the section. A group's Accept always shows;
/// Changes' grouped/folders switch shows on hover.
fn header(s: &App, sec: u8, l: &Listing, k: &Kit) -> El {
    let mut h = SidebarHeader::new(sentence(section_title(sec)))
        .count(l.shown[sec as usize])
        .collapsed(s.section_folded(sec))
        .on_toggle(move |s: &mut App, _: &mut Tx| s.toggle_list_section(sec));
    if let Some(g) = section_group(sec)
        && l.acceptable[sec as usize] > 0
    {
        let n = l.acceptable[sec as usize];
        h = h.action(
            Button::new(format!("Accept {n}"))
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Mini)
                .help(format!(
                    "Mark {n} {} files viewed without reading (Shift+V)",
                    g.label().to_lowercase()
                ))
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.accept(g, tx))
                .build(k)
                .key(format!("accept-{}", slug(g))),
        );
    }
    if sec == CHANGES {
        h = h.action(
            Button::new(if s.board.folders { "folders ▾" } else { "grouped ▾" })
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Mini)
                .help("Changes by group or by folder")
                .on_press(|s: &mut App| s.toggle_changes_mode())
                .build(k)
                .key("changes-mode")
                .group_opacity(0.0, 1.0),
        );
    }
    row()
        .key(format!("list-section-{sec}"))
        .w_full()
        .h(ROW_HEIGHT)
        .hover_group()
        .child(h.build(k))
}

/// Must bar · name · dim folder · `+n −m` · thread count · ✓ once viewed.
fn file_row(s: &App, g: usize, key: String, look: &RowLook) -> El {
    let k = &look.kit;
    let path = &s.keys[g];
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
    let group = s.group_of(g);
    let f = &s.diff.files()[g];
    let viewed = s.is_viewed(g);
    let threads = s.tree.file(path).map_or(0, |e| e.comments);
    let item = s.item(g);
    let mut said = vec![format!("{name}, {}", group.label())];
    if let Some(i) = item {
        said.push(i.reason.text.clone());
    }
    if viewed {
        said.push("viewed".into());
    }
    if threads > 0 {
        said.push(format!("{threads} comments"));
    }
    let current = s.reader_tab() == crate::app::ReaderTab::Changes && s.current() == Some(g);
    let must = el()
        .key("must")
        .w(3.0)
        .h(16.0)
        .radius(1.5)
        .fill(if group == Group::Must {
            k.c.danger.icon
        } else {
            Color::TRANSPARENT
        })
        .a11y_hidden();
    let mut r = list_row(key, k, s)
        .h(ROW_HEIGHT)
        .gap(6.0)
        .pl(0.0)
        .label(said.join(", "))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_file(g, tx))
        .child(must)
        .child(
            text(name.to_owned())
                .font(k.ty.ui)
                .color(if viewed { k.c.text_faint } else { k.c.text_base })
                .truncate()
                .shrink()
                .a11y_hidden(),
        )
        .child(
            text(dir.to_owned())
                .font(k.ty.small)
                .color(k.c.text_faint)
                .truncate()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        )
        .child(
            row()
                .gap(3.0)
                .a11y_hidden()
                .child(text(format!("+{}", f.additions)).font(k.ty.small.mono()).color(k.c.success.fg))
                .child(text(format!("−{}", f.deletions)).font(k.ty.small.mono()).color(k.c.danger.fg)),
        );
    if threads > 0 {
        r = r.child(
            text(format!("● {threads}"))
                .key("threads")
                .font(k.ty.small)
                .color(k.c.text_accent)
                .a11y_hidden(),
        );
    }
    r = r.child(
        text(if viewed { "✓" } else { "" })
            .key("viewed")
            .font(k.ty.small)
            .color(k.c.success.icon)
            .w(12.0)
            .a11y_hidden(),
    );
    if !current {
        return r;
    }
    r.fill(look.selected).checked(true).radius(0.0).child(
        el().key("current-bar")
            .absolute(0.0, 0.0)
            .w(2.0)
            .h(ROW_HEIGHT)
            .fill(k.c.border_focus)
            .a11y_hidden(),
    )
}

fn accepted_row(s: &App, a: usize, key: String, k: &Kit) -> El {
    let Some(acc) = s.mirror.accepted.get(a) else {
        return row().key(key).h(ROW_HEIGHT);
    };
    let label = format!("{} · {} accepted", acc.group.label(), acc.files.len());
    let reason = acc.reason.clone();
    list_row(key, k, s)
        .h(ROW_HEIGHT)
        .label(reason.clone())
        .on_press(move |s: &mut App| s.status = reason.clone())
        .child(text("✓").font(k.ty.small).color(k.c.success.icon).w(22.0))
        .child(text(label).font(k.ty.ui).color(k.c.text_muted).truncate().grow(1.0).shrink())
}

/// Whether the list's filter shows thread `t`'s file (a thread on no file shows unfiltered).
fn filter_shows(s: &App, t: &Thread) -> bool {
    match s.file_of_thread(t) {
        Some(g) => s.tree.is_shown(&s.keys[g]),
        None => s.tree.filter.is_empty(),
    }
}

fn waiting(s: &App, mut c: El, k: &Kit) -> El {
    let mut waiting: Vec<&Thread> = s
        .mirror
        .threads
        .iter()
        .filter(|t| ops::waiting_on_user(t, ME) && filter_shows(s, t))
        .collect();
    waiting.sort_by_key(|t| (s.file_of_thread(t).unwrap_or(usize::MAX), t.anchor.lines().0));
    if waiting.is_empty() {
        return c;
    }
    let header = SidebarHeader::new(sentence(section_title(WAITING)))
        .count(waiting.len())
        .collapsed(s.section_folded(WAITING))
        .on_toggle(move |s: &mut App, _: &mut Tx| s.toggle_list_section(WAITING))
        .build(k);
    c = c.child(
        row()
            .key(format!("list-section-{WAITING}"))
            .w_full()
            .h(ROW_HEIGHT)
            .fill(k.c.layer_01)
            .child(header),
    );
    if !s.section_folded(WAITING) {
        let rows: Vec<El> = if waiting.is_empty() {
            vec![muted("Nothing waiting on you.", k).px(8.0).py(4.0)]
        } else {
            waiting.into_iter().map(|t| waiting_row(s, t, k)).collect()
        };
        c = c.child(
            col()
                .key("list-waiting")
                .w_full()
                .max_h(200.0)
                .scroll(ScrollSpec { stick_bottom: false })
                .child(col().w_full().px(6.0).pb(8.0).gap(1.0).children(rows)),
        );
    }
    c
}

fn waiting_row(s: &App, t: &Thread, k: &Kit) -> El {
    let key = s.file_of_thread(t).map_or_else(|| t.file_path.clone(), |i| s.keys[i].clone());
    let last = t.messages.last();
    let who = last.and_then(|m| m.author.clone()).unwrap_or_default();
    let body = last.map_or("", |m| m.body.as_str());
    let id = t.id.clone();
    let place = if t.file_path.starts_with('@') {
        crate::app::composer::artifact_title(&t.file_path, t.target.as_ref())
    } else {
        format!("{} {}", key.rsplit('/').next().unwrap_or(&key), lines_label(&t.anchor))
    };
    list_row(format!("thread-{}", t.id), k, s)
        .label(format!("Waiting on you: {}", anchor_label(&key, &t.anchor)))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_thread(&id, tx))
        .child(text("●").font(k.ty.small).color(k.c.warning.icon).w(14.0))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .child(
                    text(format!("{who}: {}", rain_comments::label::first_sentence(body, 80)))
                        .font(k.ty.small)
                        .color(k.c.text_base)
                        .truncate(),
                )
                .child(text(place).font(k.ty.small).color(k.c.text_muted).truncate()),
        )
}

// ----- file-at-a-time cards -----

/// The reader's card in file-at-a-time when the item is not a file.
pub(super) fn single_card(s: &App, k: &Kit) -> Option<El> {
    if !s.single_mode() {
        return None;
    }
    match s.board.pick {
        Pick::File(_) => None,
        Pick::Group(g) => Some(group_card(s, g, k)),
        Pick::Landing | Pick::Brief => Some(landing(s, k)),
    }
}

fn card(key: &str, k: &Kit) -> El {
    col()
        .key(key.to_owned())
        .role("region")
        .w_full()
        .max_w(860.0)
        .gap(10.0)
        .padding(18.0)
        .radius(k.radius.lg)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
}

fn heading(t: String, k: &Kit) -> El {
    text(t).font(k.ty.ui_medium).color(k.c.text_base)
}

fn wrap_card(e: El) -> El {
    col()
        .key("single-card")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(24.0)
        .items_center()
        .child(e)
}

/// Counts, one Accept per group left, and where to start.
fn landing(s: &App, k: &Kit) -> El {
    let order = s.list_order();
    let real = order.iter().filter(|&&g| s.group_of(g).is_change()).count();
    let grouped = order.len() - real;
    let mut c = card("landing", k).child(heading("File at a time".into(), k)).child(
        text(format!(
            "{real} real changes · {grouped} files in groups · {} waiting on you",
            s.waiting_count()
        ))
        .key("landing-counts")
        .font(k.ty.ui)
        .color(k.c.text_muted),
    );
    let l = s.listing();
    let mut accepts = row().key("landing-accepts").gap(8.0).flex_wrap();
    for g in Group::ALL.into_iter().filter(|g| !g.is_change()) {
        let n = l.acceptable[section_of(g) as usize];
        if n > 0 {
            accepts = accepts.child(
                Button::new(format!("Accept {} {n}", g.label()))
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| s.accept(g, tx))
                    .build(k)
                    .key(format!("landing-accept-{}", slug(g))),
            );
        }
    }
    c = c.child(accepts);
    c = c.child(
        Button::new("Start with the first item (])")
            .variant(ButtonVariant::Neutral)
            .on_press_tx(|s: &mut App, tx: &mut Tx| s.step_item(true, tx))
            .build(k)
            .key("landing-start"),
    );
    if s.board.pick == Pick::Brief {
        c = c.child(muted("The Brief is above the reader; b folds it.", k));
    }
    wrap_card(c)
}

/// A group: its counts and reasons, the exemplar (first file), the outliers (files whose
/// reason differs from the exemplar's), the files, and Accept.
fn group_card(s: &App, group: Group, k: &Kit) -> El {
    let files: Vec<usize> = (0..s.keys.len()).filter(|&g| s.group_of(g) == group).collect();
    let lines: u32 = files
        .iter()
        .map(|&g| s.diff.files()[g].additions + s.diff.files()[g].deletions)
        .sum();
    let viewed = files.iter().filter(|&&g| s.is_viewed(g)).count();
    let reason_of = |g: usize| s.item(g).map(|i| i.reason.text.clone()).unwrap_or_default();
    let exemplar = files.first().copied();
    let first_reason = exemplar.map(reason_of).unwrap_or_default();
    let outliers: Vec<usize> = files.iter().copied().filter(|&g| reason_of(g) != first_reason).collect();
    let n = s.listing().acceptable[section_of(group) as usize];
    let mut c = card("group-card", k)
        .child(heading(format!("{} · {} files · {lines} lines", group.label(), files.len()), k))
        .child(
            text(format!("{viewed} of {} viewed · why: {first_reason}", files.len()))
                .key("group-reason")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    if let Some(g) = exemplar {
        c = c.child(
            Button::new(format!("Open the exemplar: {}", s.keys[g]))
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_file(g, tx))
                .build(k)
                .key("group-exemplar"),
        );
    }
    if !outliers.is_empty() {
        c = c.child(muted(
            format!("Outliers ({}): a different reason than the exemplar", outliers.len()),
            k,
        ));
        for &g in outliers.iter().take(8) {
            c = c.child(file_line(s, g, "outlier", k));
        }
    }
    c = c.child(muted("Files", k));
    for &g in files.iter().take(12) {
        c = c.child(file_line(s, g, "group-file", k));
    }
    if files.len() > 12 {
        c = c.child(muted(format!("+{} more", files.len() - 12), k));
    }
    let actions = row().gap(8.0).child(
        Button::new(if n > 0 {
            format!("Accept {n} (Space)")
        } else {
            "All viewed".into()
        })
        .variant(ButtonVariant::Outline)
        .disabled(n == 0)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.accept(group, tx))
        .build(k)
        .key("card-accept"),
    );
    wrap_card(c.child(actions))
}

fn file_line(s: &App, g: usize, prefix: &str, k: &Kit) -> El {
    let key = s.keys[g].clone();
    let why = s.item(g).map(|i| i.reason.text.clone()).unwrap_or_default();
    k.interactive(
        row()
            .key(format!("{prefix}-{key}"))
            .role("button")
            .label(key.clone())
            .w_full()
            .gap(8.0)
            .px(6.0)
            .py(2.0)
            .radius(k.radius.sm)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_file(g, tx)),
    )
    .child(text(key).font(k.ty.small.mono()).color(k.c.text_base).truncate().grow(1.0).shrink())
    .child(text(why).font(k.ty.small).color(k.c.text_faint))
}
