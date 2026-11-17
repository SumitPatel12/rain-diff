//! The list island: the filter field and its toggles, then one virtualised column of
//! collapsible sections in triage order, and at the bottom a tray on the well holding the folded
//! groups with their Accept, the accept snackbar and Waiting on you. Also file-at-a-time's
//! cards (landing and group) for the reader.

use std::borrow::Cow;
use std::rc::Rc;

use rain_comments::{anchor_label, ops};
use rain_core::{Thread, ThreadKind};
use rain_tree::{TreeFilter, TreeView};
use rain_triage::Group;
use rune::{Color, Cursor, El, GrowthSpec, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit};

use super::icons;
use super::ui::{Ghost, outline, sr};
use super::{messages, muted};
use crate::app::board::{CHANGE_GROUPS, CHANGES, Listing, Pick, Row, WAITING, change_title, section_group, section_of, section_title};
use crate::app::{App, ME};
use crate::settings::Density;
use crate::theme::{Look, Roles, Surfaces, TYPE, radius};

/// A file row's height: 26 compact, 32 comfortable.
pub(crate) fn row_height(s: &App) -> f32 {
    if s.settings.density == Density::Compact { 26.0 } else { 32.0 }
}

/// A group row's height in the tray: 30 compact, 36 comfortable.
fn group_height(s: &App) -> f32 {
    if s.settings.density == Density::Compact { 30.0 } else { 36.0 }
}

/// `CHANGES` as `Changes`, `WAITING ON YOU` as `Waiting on you`.
fn sentence(title: &str) -> String {
    let lower = title.to_lowercase();
    let mut c = lower.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// What a list row needs of the look, shared by the virtual rows.
struct RowLook {
    kit: Kit,
    s: Surfaces,
    r: Roles,
}

fn slug(g: Group) -> String {
    g.label().to_lowercase()
}

pub(super) fn board(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let c = col()
        .key("board")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(filter_header(s, k))
        .child(body(s, look))
        .children(super::filter::hidden_foot(s, k));
    match tray(s, look) {
        Some(t) => c.child(t),
        None => c,
    }
}

/// The filter field, with the toggles (repo, tag, unviewed, comments) behind its funnel; they
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
        .pr(10.0)
        .pb(6.0)
        .gap(0.0)
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
        s: look.s,
        r: look.r,
    });
    let n = listing.rows.len();
    let rows = col()
        .key("list-files")
        .w_full()
        .pb(6.0)
        .growth(GrowthSpec::default())
        .virtual_rows(
            n,
            row_height(s),
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
        Some(Row::Sub(g)) => sub_header(s, g, &listing, key, k),
        Some(Row::File(g)) => file_row(s, g, key, look),
        Some(Row::Accepted(a)) => accepted_row(s, a, key, k),
        None => row().key(key).h(row_height(s)),
    }
}

/// `▾ M Must review  1 of 4 viewed` over the change group's files; a press folds the group.
fn sub_header(s: &App, g: Group, l: &Listing, key: String, k: &Kit) -> El {
    let folded = s.sub_folded(g);
    let n = CHANGE_GROUPS.iter().position(|&x| x == g).map_or(0, |i| l.changes[i]);
    let files: Vec<usize> = (0..s.keys.len()).filter(|&f| s.group_of(f) == g).collect();
    let viewed = files.iter().filter(|&&f| s.is_viewed(f)).count();
    let name = change_title(g);
    let color = if g == Group::Must { k.c.danger.fg } else { k.c.text_muted };
    let count = if g == Group::Skim {
        n.to_string()
    } else {
        format!("{viewed} of {} viewed", files.len())
    };
    let r = row()
        .key("sub")
        .role("button")
        .label(format!("{name}, {n} files, {count}"))
        .expanded(!folded)
        .w_full()
        .h(26.0)
        .pl(10.0)
        .pr(12.0)
        .gap(6.0)
        .items_center()
        .radius(radius::BUTTON)
        .cursor(Cursor::Pointer)
        .on_press(move |s: &mut App| s.toggle_sub(g))
        .child(icons::draw(
            if folded { &icons::CHEV_R } else { &icons::CHEV_D },
            14.0,
            k.c.text_faint,
        ))
        .child(text(name).font(TYPE.small).color(color).a11y_hidden())
        .child(
            text(count)
                .font(TYPE.small.features("'tnum' 1"))
                .color(k.c.text_faint)
                .a11y_hidden(),
        );
    row().key(key).w_full().px(6.0).pt(4.0).child(k.interactive(r))
}

/// A section's head: `▾ Name count` (a press folds it), and its action: a group's Accept, or
/// Real changes' grouped/folders switch.
fn header(s: &App, sec: u8, l: &Listing, k: &Kit) -> El {
    let folded = s.section_folded(sec);
    let title = sentence(section_title(sec));
    let mut h = row()
        .key(format!("list-section-{sec}"))
        .role("button")
        .label(format!("{title}, {} files", l.shown[sec as usize]))
        .expanded(!folded)
        .w_full()
        .h(28.0)
        .pl(8.0)
        .pr(10.0)
        .gap(6.0)
        .items_center()
        .hover_group()
        .cursor(Cursor::Pointer)
        .on_press_tx(move |s: &mut App, _: &mut Tx| s.toggle_list_section(sec))
        .child(icons::draw(
            if folded { &icons::CHEV_R } else { &icons::CHEV_D },
            16.0,
            k.c.text_faint,
        ))
        .child(text(title).font(TYPE.label).color(k.c.text_base).a11y_hidden())
        .child(
            text(l.shown[sec as usize].to_string())
                .font(TYPE.small.features("'tnum' 1"))
                .color(k.c.text_faint)
                .a11y_hidden(),
        )
        .child(el().grow(1.0).a11y_hidden());
    if let Some(g) = section_group(sec)
        && l.acceptable[sec as usize] > 0
    {
        h = h.child(accept_button(g, l.acceptable[sec as usize], k));
    }
    if sec == CHANGES {
        h = h.child(
            Ghost::new("changes-mode")
                .icon(&icons::TREE)
                .help(if s.board.folders {
                    "Changes by folder · press for by group"
                } else {
                    "Changes by group · press for by folder"
                })
                .height(22.0)
                .on(s.board.folders)
                .build(k)
                .on_press(|s: &mut App| s.toggle_changes_mode()),
        );
    }
    h
}

/// `Accept N`: marks the group viewed without reading (Shift+A; u undoes).
fn accept_button(g: Group, n: usize, k: &Kit) -> El {
    outline(format!("accept-{}", slug(g)), format!("Accept {n}"), None, 22.0, k)
        .help(format!(
            "Mark {n} {} files viewed without reading  Shift+A on a file in the group",
            g.label().to_lowercase()
        ))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.accept(g, tx))
}

/// Group tag · name · dim folder · `+n −m` · thread count · ✓ once viewed.
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
    let dir = dir.replacen('/', " · ", 1);
    let h = row_height(s);
    let name_color = if viewed || group == Group::Skim {
        k.c.text_muted
    } else {
        k.c.text_base
    };
    let mut r = row()
        .key("row")
        .role("button")
        .hover_group()
        .label(said.join(", "))
        .help(path.clone())
        .w_full()
        .h(h)
        .pl(10.0)
        .pr(8.0)
        .gap(6.0)
        .items_center()
        .radius(radius::BUTTON)
        .cursor(Cursor::Pointer)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_file(g, tx))
        .child(
            text(name.to_owned())
                // One weight whether current or not, so selecting a row moves nothing in it.
                .font(TYPE.ui)
                .color(name_color)
                .truncate_middle()
                .shrink()
                .min_w(60.0)
                .a11y_hidden(),
        )
        .child(
            text(dir)
                .font(TYPE.small)
                .color(k.c.text_faint)
                .truncate_head()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        )
        .child(
            row()
                .gap(4.0)
                .a11y_hidden()
                .child(text(format!("+{}", f.additions)).font(TYPE.micro_mono).color(k.c.success.fg))
                .child(text(format!("−{}", f.deletions)).font(TYPE.micro_mono).color(k.c.danger.fg)),
        );
    if threads > 0 {
        r = r.child(
            row()
                .key("threads")
                .gap(2.0)
                .items_center()
                .a11y_hidden()
                .child(icons::draw(&icons::CHAT, 12.0, k.c.text_muted))
                .child(text(threads.to_string()).font(TYPE.micro).color(k.c.text_muted)),
        );
    }
    // The group's colour as a thin bar at the row's left; the heading above names it.
    r = r.child(
        el().key("group-bar")
            .absolute(3.0, 7.0)
            .w(2.0)
            .h(h - 14.0)
            .radius(1.0)
            .fill(super::ui::group_mark(group, k).1)
            .a11y_hidden(),
    );
    // The viewed toggle sits in a fixed slot, so marking a file moves nothing beside it: a check
    // once viewed, an empty ring on hover before; a press flips it without opening the file.
    let mark = if viewed {
        icons::draw(&icons::CHECK, 16.0, look.r.accent)
    } else {
        el().size(12.0, 12.0)
            .radius(6.0)
            .border(1.5, k.c.icon_faint)
            .group_opacity(0.0, 1.0)
            .a11y_hidden()
    };
    r = r.child(
        row()
            .key(if viewed { "viewed" } else { "viewed-slot" })
            .role("checkbox")
            .label(if viewed { "Viewed" } else { "Mark viewed" })
            .checked(viewed)
            .help(if viewed {
                "Viewed · press to unmark  Shift+Space"
            } else {
                "Mark viewed  Shift+Space"
            })
            .size(20.0, 20.0)
            .center()
            .radius(radius::KEY)
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut App| s.set_viewed(g, !viewed))
            .child(mark),
    );
    let r = if current {
        r.fill(look.s.selected_row)
            .checked(true)
            .child(el().key("current-bar").size(0.0, 0.0).a11y_hidden())
    } else {
        k.interactive(r)
    };
    row().key(key).w_full().px(6.0).child(r)
}

fn accepted_row(s: &App, a: usize, key: String, k: &Kit) -> El {
    let Some(acc) = s.mirror.accepted.get(a) else {
        return row().key(key).h(row_height(s));
    };
    let label = format!("{} · {} accepted", acc.group.label(), acc.files.len());
    let reason = acc.reason.clone();
    let (_, r) = sr(k);
    k.interactive(
        row()
            .key(key)
            .role("button")
            .label(reason.clone())
            .w_full()
            .h(24.0)
            .pl(28.0)
            .pr(8.0)
            .gap(6.0)
            .items_center()
            .radius(radius::BUTTON)
            .on_press(move |s: &mut App| s.status = reason.clone()),
    )
    .child(icons::draw(&icons::CHECK, 14.0, r.accent))
    .child(text(label).font(TYPE.small).color(k.c.text_muted).truncate().grow(1.0).shrink())
    .help("Undo accept  u")
}

/// Whether the list's filter shows thread `t`'s file (a thread on no file shows unfiltered).
fn filter_shows(s: &App, t: &Thread) -> bool {
    match s.file_of_thread(t) {
        Some(g) => s.tree.is_shown(&s.keys[g]),
        None => s.tree.filter.is_empty(),
    }
}

/// A row in the tray: `▸ Tests 2  [Accept 2]`.
fn tray_group(s: &App, sec: u8, l: &Listing, k: &Kit) -> El {
    let folded = s.section_folded(sec);
    let title = sentence(section_title(sec));
    let mut h = k
        .interactive(
            row()
                .key(format!("list-section-{sec}"))
                .role("button")
                .label(format!("{title}, {} files", l.shown[sec as usize]))
                .expanded(!folded)
                .w_full()
                .h(group_height(s))
                .pl(8.0)
                .pr(6.0)
                .gap(6.0)
                .items_center()
                .radius(radius::BUTTON)
                .cursor(Cursor::Pointer)
                .on_press_tx(move |s: &mut App, _: &mut Tx| s.toggle_list_section(sec)),
        )
        .child(icons::draw(
            if folded { &icons::CHEV_R } else { &icons::CHEV_D },
            16.0,
            k.c.text_muted,
        ))
        .child(text(title).font(TYPE.small).color(k.c.text_base).a11y_hidden())
        .child(
            text(l.shown[sec as usize].to_string())
                .font(TYPE.small)
                .color(k.c.text_faint)
                .a11y_hidden(),
        )
        .child(el().grow(1.0).a11y_hidden());
    if let Some(g) = section_group(sec)
        && l.acceptable[sec as usize] > 0
    {
        h = h.child(accept_button(g, l.acceptable[sec as usize], k));
    }
    h
}

fn divider(s: &Surfaces) -> El {
    row()
        .w_full()
        .px(8.0)
        .py(4.0)
        .a11y_hidden()
        .child(el().w_full().h(1.0).fill(s.hairline))
}

/// The tray at the list's foot: the folded groups and their Accept, the accept snackbar, and
/// Waiting on you; `None` when it would be empty.
fn tray(s: &App, look: &Look) -> Option<El> {
    let k = &look.kit;
    let listing = s.listing();
    let mut waiting: Vec<&Thread> = s
        .mirror
        .threads
        .iter()
        .filter(|t| ops::waiting_on_user(t, ME) && filter_shows(s, t))
        .collect();
    waiting.sort_by_key(|t| (s.file_of_thread(t).unwrap_or(usize::MAX), t.anchor.lines().0));
    let snack = s.accept_snack();
    if listing.footer.is_empty() && waiting.is_empty() && snack.is_none() {
        return None;
    }
    let mut c = col()
        .key("list-tray")
        .w_full()
        .py(4.0)
        .radius(radius::CARD)
        .fill(look.s.well)
        .border(1.0, look.s.hairline);
    if !listing.footer.is_empty() {
        c = c.child(
            col()
                .key("list-groups")
                .role("group")
                .label("Folded groups")
                .w_full()
                .children(listing.footer.iter().map(|&sec| tray_group(s, sec, &listing, k))),
        );
    }
    if let Some(msg) = snack {
        c = c.child(row().w_full().px(6.0).py(4.0).child(messages::snackbar(
            "accept-snack",
            msg.to_owned(),
            Some(("Undo (u)", |s: &mut App, tx: &mut Tx| s.undo_accept(tx))),
            k,
        )));
    }
    if !waiting.is_empty() {
        if !listing.footer.is_empty() || snack.is_some() {
            c = c.child(divider(&look.s));
        }
        let folded = s.section_folded(WAITING);
        c = c.child(
            k.interactive(
                row()
                    .key(format!("list-section-{WAITING}"))
                    .role("button")
                    .label(format!("Waiting on you, {} threads", waiting.len()))
                    .expanded(!folded)
                    .w_full()
                    .h(group_height(s))
                    .pl(8.0)
                    .pr(6.0)
                    .gap(6.0)
                    .items_center()
                    .radius(radius::BUTTON)
                    .cursor(Cursor::Pointer)
                    .on_press_tx(move |s: &mut App, _: &mut Tx| s.toggle_list_section(WAITING)),
            )
            .child(icons::draw(
                if folded { &icons::CHEV_R } else { &icons::CHEV_D },
                16.0,
                k.c.text_muted,
            ))
            .child(
                text(sentence(section_title(WAITING)))
                    .font(TYPE.small)
                    .color(k.c.warning.fg)
                    .a11y_hidden(),
            )
            .child(text(waiting.len().to_string()).font(TYPE.small).color(k.c.text_faint).a11y_hidden()),
        );
        if !folded {
            let rows: Vec<El> = waiting.into_iter().map(|t| waiting_row(s, t, k)).collect();
            c = c.child(
                col()
                    .key("list-waiting")
                    .w_full()
                    .max_h(200.0)
                    .scroll(ScrollSpec { stick_bottom: false })
                    .child(col().w_full().gap(1.0).children(rows)),
            );
        }
    }
    Some(row().w_full().padding(8.0).child(c))
}

/// `claude · issue  VouchersController.cs:23` over the last message.
fn waiting_row(s: &App, t: &Thread, k: &Kit) -> El {
    let key = s.file_of_thread(t).map_or_else(|| t.file_path.clone(), |i| s.keys[i].clone());
    let last = t.messages.last();
    let who = last
        .and_then(|m| m.author.clone())
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| "you".into());
    let body = last.map_or("", |m| m.body.as_str());
    let id = t.id.clone();
    let place = if t.file_path.starts_with('@') {
        crate::app::composer::artifact_title(&t.file_path, t.target.as_ref())
    } else {
        format!("{}:{}", key.rsplit('/').next().unwrap_or(&key), t.anchor.lines().0)
    };
    let kind = match t.kind() {
        ThreadKind::Comment => "comment",
        k => k.label(),
    };
    k.interactive(
        col()
            .key(format!("thread-{}", t.id))
            .role("button")
            .label(format!("Waiting on you: {}", anchor_label(&key, &t.anchor)))
            .w_full()
            .pl(28.0)
            .pr(8.0)
            .pt(4.0)
            .pb(6.0)
            .radius(radius::BUTTON)
            .cursor(Cursor::Pointer)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_thread(&id, tx)),
    )
    .child(
        row()
            .gap(6.0)
            .items_center()
            .child(text(format!("{who} · {kind}")).font(TYPE.small).color(k.c.text_muted))
            .child(text(place).font(TYPE.micro_mono).color(k.c.text_accent).truncate().shrink()),
    )
    .child(
        text(rain_comments::label::first_sentence(body, 80))
            .font(TYPE.small)
            .color(k.c.text_base)
            .truncate(),
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
    let (surf, _) = sr(k);
    col()
        .key(key.to_owned())
        .role("region")
        .w_full()
        .max_w(640.0)
        .gap(12.0)
        .padding(22.0)
        .radius(radius::ISLAND)
        .fill(surf.panel)
        .border(1.0, surf.card_border)
}

fn heading(t: String, k: &Kit) -> El {
    text(t).font(rune::tok::TextStyle::new(18.0, 600.0).lh(24.0)).color(k.c.text_base)
}

fn wrap_card(e: El) -> El {
    col()
        .key("single-card")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .pt(28.0)
        .px(24.0)
        .items_center()
        .child(e)
}

/// A count tile on the well: the number large, what it counts under it.
fn tile(key: &str, n: usize, what: &str, ink: Color, k: &Kit) -> El {
    let (surf, _) = sr(k);
    col()
        .key(key.to_owned())
        .label(format!("{n} {what}"))
        .w(0.0)
        .grow(1.0)
        .h(64.0)
        .px(12.0)
        .justify_center()
        .radius(radius::CARD)
        .fill(surf.well)
        .child(
            text(n.to_string())
                .font(rune::tok::TextStyle::new(20.0, 600.0).lh(26.0))
                .color(ink)
                .a11y_hidden(),
        )
        .child(text(what.to_owned()).font(TYPE.small).color(k.c.text_muted).a11y_hidden())
}

/// Counts, a row per group with its Accept, and where to start.
fn landing(s: &App, k: &Kit) -> El {
    let (surf, r) = sr(k);
    let order = s.list_order();
    let count = |g: Group| order.iter().filter(|&&f| s.group_of(f) == g).count();
    let real = order.iter().filter(|&&g| s.group_of(g).is_change()).count();
    let grouped = order.len() - real;
    let groups = Group::ALL.into_iter().filter(|g| !g.is_change() && count(*g) > 0).count();
    let mut c = card("landing", k)
        .child(heading(format!("{} files in this review", order.len()), k))
        .child(
            text("Start with Must review. Groups can be accepted without reading them; you can undo with u.")
                .key("landing-counts")
                .label(format!(
                    "{real} real changes · {grouped} files in groups · {} waiting on you",
                    s.waiting_count()
                ))
                .font(TYPE.ui)
                .color(k.c.text_muted)
                .wrap(),
        )
        .child(
            row()
                .key("landing-tiles")
                .w_full()
                .gap(10.0)
                .child(tile("tile-must", count(Group::Must), "Must review", r.must, k))
                .child(tile("tile-review", count(Group::Review), "Review", k.c.text_base, k))
                .child(tile("tile-skim", count(Group::Skim), "Skim", k.c.text_base, k))
                .child(tile("tile-groups", grouped, &format!("in {groups} groups"), k.c.text_base, k)),
        );
    let l = s.listing();
    let mut list = col().key("landing-accepts").w_full();
    for g in Group::ALL.into_iter().filter(|g| !g.is_change()) {
        let total = count(g);
        if total == 0 {
            continue;
        }
        let n = l.acceptable[section_of(g) as usize];
        let mut line = row()
            .key(format!("landing-group-{}", slug(g)))
            .w_full()
            .h(36.0)
            .px(4.0)
            .gap(8.0)
            .items_center()
            .child(if n > 0 {
                icons::draw(&icons::CHEV_R, 16.0, k.c.text_muted)
            } else {
                icons::draw(&icons::CHECK, 16.0, r.accent)
            })
            .child(text(g.label().to_owned()).font(TYPE.ui).color(k.c.text_base))
            .child(
                text(if n > 0 {
                    format!("{total} file{}", if total == 1 { "" } else { "s" })
                } else {
                    format!("{total} accepted")
                })
                .font(TYPE.small)
                .color(k.c.text_muted),
            )
            .child(el().grow(1.0).a11y_hidden());
        if n > 0 {
            line = line.child(
                outline(format!("landing-accept-{}", slug(g)), format!("Accept {n}"), None, 24.0, k)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| s.accept(g, tx)),
            );
        }
        list = list.child(
            line.child(
                el().pin(Some(0.0), None, Some(0.0), Some(0.0))
                    .h(1.0)
                    .fill(surf.hairline)
                    .a11y_hidden(),
            ),
        );
    }
    c = c.child(list);
    c = c.child(
        row().w_full().justify_end().child(
            k.interactive_with(
                row()
                    .key("landing-start")
                    .role("button")
                    .label("Start with the first item")
                    .h(30.0)
                    .px(14.0)
                    .gap(8.0)
                    .center()
                    .radius(radius::BUTTON)
                    .fill(r.accent)
                    .focusable(true)
                    .cursor(Cursor::Pointer)
                    .child(text("Start with the first item").font(TYPE.strong).color(r.on_accent).a11y_hidden())
                    .child(text("]").font(TYPE.micro_mono).color(r.on_accent).a11y_hidden()),
                k.c.contrast_hover,
                k.c.contrast_pressed,
            )
            .press_scale(0.97)
            .on_press_tx(|s: &mut App, tx: &mut Tx| s.step_item(true, tx)),
        ),
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
