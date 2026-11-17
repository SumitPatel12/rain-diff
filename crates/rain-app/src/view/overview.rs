//! The Overview tab: the Brief as a page of Markdown under real headings (Intent, How it works,
//! Invariants, Not changed, Not covered, Verify by hand), the facts triage proves and the Map
//! beside it, cite chips that peek and jump, and comments or questions on every section. Without
//! a brief the story notes and the facts stand in.

use rain_comments::{Composer, ThreadCard};
use rain_core::brief::{BriefDoc, Cite};
use rain_core::{Side, Thread, short_sha};
use rune::{El, ScrollSpec, TextStyle, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Markdown, SegmentedControl};

use super::ui::{outline, sr};
use super::{icons, messages, muted, notes};
use crate::app::{App, ArtifactDraft, ComposeMode, PeekTarget};
use crate::theme::{TYPE, radius};

/// The scheme of a cite link the page rewrites a code span into.
pub(crate) const CITE_SCHEME: &str = "rain-cite:";
/// Windows at least this wide put Facts and Map in a column beside the text.
const SIDE_COLUMN_MIN: f32 = 1400.0;
const READING_MAX: f32 = 680.0;
const SIDE_W: f32 = 300.0;

fn heading(t: impl Into<String>, style: TextStyle, k: &Kit) -> El {
    text(t.into()).font(style).color(k.c.text_base).wrap().role("heading")
}

fn h1(t: impl Into<String>, k: &Kit) -> El {
    heading(t, TYPE.h1, k)
}

fn h2(t: impl Into<String>, k: &Kit) -> El {
    heading(t, TYPE.h2, k)
}

fn h3(t: impl Into<String>, k: &Kit) -> El {
    heading(t, TextStyle::new(14.0, 600.0).lh(22.0), k)
}

/// The Overview page, scrolling on its own.
pub(super) fn page(s: &App, k: &Kit) -> El {
    let doc = s.brief_doc();
    let wide = s.window_width >= SIDE_COLUMN_MIN;
    let mut reading = col()
        .key("overview-text")
        .w_full()
        .max_w(READING_MAX)
        .gap(12.0)
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .child(title_row(s, doc, k));
    if let Some(d) = doc
        && let Some(stale) = stale_line(s, d)
    {
        reading = reading.child(messages::inset_banner(
            "brief-stale",
            stale,
            messages::Tone::Warning,
            &[("Refresh brief", |s: &mut App, tx: &mut Tx| s.refresh_brief(tx))],
            k,
        ));
    }
    if !wide {
        let mut cards = row()
            .key("overview-cards")
            .w_full()
            .gap(12.0)
            .items_start()
            .flex_wrap()
            .child(facts(s, k));
        if let Some(m) = map_card(s, k) {
            cards = cards.child(m);
        }
        reading = reading.child(cards);
    }
    reading = reading.child(match doc {
        Some(d) => sections(s, d, k),
        None => fallback(s, k),
    });
    if let Some(o) = orphans(s, k) {
        reading = reading.child(o);
    }
    let mut body = row()
        .key("overview-body")
        .w_full()
        .pl(36.0)
        .pr(28.0)
        .pt(24.0)
        .pb(48.0)
        .gap(28.0)
        .items_start()
        .on_layout(|s: &mut App, r: [f32; 4]| s.peek_page_moved(r[1]))
        .child(reading);
    if wide {
        let mut side = col().key("overview-side").w(SIDE_W).gap(12.0).child(facts(s, k));
        if let Some(m) = map_card(s, k) {
            side = side.child(m);
        }
        body = body.child(side);
    }
    col()
        .key("overview")
        .role("region")
        .label("Overview")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(k.c.bg_base)
        .on_layout(|s: &mut App, r: [f32; 4]| s.peek_page_top(r[1]))
        .scroll(ScrollSpec { stick_bottom: false })
        .child(body)
}

/// The title, then the tour as the page's one primary action.
fn title_row(s: &App, doc: Option<&BriefDoc>, k: &Kit) -> El {
    let named = s.overview.as_ref().map(|l| l.document.title.trim()).filter(|t| !t.is_empty());
    let title = doc
        .map(|d| d.title.trim())
        .filter(|t| !t.is_empty())
        .or(named)
        .unwrap_or("Overview");
    let mut c = row()
        .key("overview-title")
        .w_full()
        .gap(12.0)
        .items_center()
        .pb(2.0)
        .child(h1(title.to_owned(), k).grow(1.0).shrink().min_w(0.0));
    {
        let steps = s.tour_steps().len();
        let label = match steps {
            1 => "Start tour, 1 step".to_owned(),
            n => format!("Start tour, {n} steps"),
        };
        let (surf, r) = sr(k);
        let b = k
            .interactive_with(
                row()
                    .key("brief-tour")
                    .role("button")
                    .label(label.clone())
                    .help(format!("A guided pass over {steps} steps, the code spotlit  Shift+T"))
                    .h(28.0)
                    .px(14.0)
                    .gap(8.0)
                    .center()
                    .radius(radius::BUTTON)
                    .fill(r.accent)
                    .cursor(rune::Cursor::Pointer)
                    .focusable(true)
                    .child(icons::draw(&icons::PLAY, 14.0, r.on_accent))
                    .child(text("Start tour").font(TYPE.strong).color(r.on_accent).a11y_hidden()),
                k.c.contrast_hover,
                k.c.contrast_pressed,
            )
            .press_scale(0.97);
        let b = if steps == 0 {
            b.opacity(0.45).disabled(true)
        } else {
            b.on_press_tx(|s: &mut App, tx: &mut Tx| s.start_tour(tx)).child(
                row()
                    .key("brief-tour-count")
                    .min_w(18.0)
                    .h(18.0)
                    .px(5.0)
                    .center()
                    .radius(9.0)
                    .fill(surf.card_head)
                    .border(1.5, r.accent)
                    .pin(None, Some(-8.0), Some(-8.0), None)
                    .a11y_hidden()
                    .child(text(steps.to_string()).font(TYPE.micro.features("'tnum' 1")).color(k.c.text_base)),
            )
        };
        c = c.child(b);
    }
    c
}

/// "Written against server a1b2c3d; loaded server 9f8e7d6" when the diff moved on.
fn stale_line(s: &App, d: &BriefDoc) -> Option<String> {
    let targets: Vec<(String, String)> = s.mirror.repos.iter().map(|r| (r.id.clone(), r.target.clone())).collect();
    let st = rain_overview::brief::stale(d, &targets);
    if st.is_empty() {
        return None;
    }
    let parts: Vec<String> = st
        .iter()
        .map(|x| format!("{} {} → {}", x.repo, short_sha(&x.written), short_sha(&x.loaded)))
        .collect();
    Some(format!(
        "The brief was written against other commits ({}); its cites may be off",
        parts.join(", ")
    ))
}

/// A card on the well: `Title · subtitle`, then its body.
fn card(key: &str, title: &str, sub: &str, body: El, k: &Kit) -> El {
    let (surf, _) = sr(k);
    col()
        .key(key.to_owned())
        .role("group")
        .label(title.to_owned())
        .w_full()
        .max_w(SIDE_W)
        .pl(12.0)
        .pr(12.0)
        .pt(10.0)
        .pb(12.0)
        .gap(6.0)
        .radius(radius::CARD)
        .fill(surf.well)
        .border(1.0, surf.hairline)
        .child(
            row()
                .gap(4.0)
                .child(text(title.to_owned()).font(TYPE.label).color(k.c.text_base))
                .child(text(sub.to_owned()).font(TYPE.small).color(k.c.text_faint)),
        )
        .child(body.w_full())
}

/// The facts the diff itself proves; its own look, never the agent's.
fn facts(s: &App, k: &Kit) -> El {
    let f = &s.brief.facts;
    let group = |g: rain_triage::Group| s.brief.groups.iter().find(|c| c.group == g).map_or(0, |c| c.count);
    let mut items: Vec<(&str, String, bool)> = vec![
        ("Files", f.files.to_string(), false),
        ("Real changes", f.real.to_string(), false),
        ("Tests", format!("{} (+{} −{})", f.tests, f.tests_added, f.tests_removed), false),
    ];
    if f.migrations > 0 {
        items.push(("Migrations", f.migrations.to_string(), true));
    }
    for (g, name) in [
        (rain_triage::Group::Generated, "Generated"),
        (rain_triage::Group::Lockfile, "Lockfiles"),
        (rain_triage::Group::FormattingOnly, "Formatting-only"),
        (rain_triage::Group::Moves, "Moves"),
    ] {
        let n = group(g);
        if n > 0 {
            items.push((name, n.to_string(), false));
        }
    }
    items.push(("Tests off", f.tests_off.len().to_string(), !f.tests_off.is_empty()));
    if let Some(c) = s.brief_check()
        && !c.files_not_cited.is_empty()
    {
        items.push(("Not in the brief", c.files_not_cited.len().to_string(), true));
    }
    let mut list = col();
    for (i, (name, value, warn)) in items.into_iter().enumerate() {
        let ink = if warn { k.c.warning.fg } else { k.c.text_muted };
        list = list.child(
            row()
                .key(format!("fact-{i}"))
                .label(format!("{value} {name}"))
                .h(24.0)
                .items_center()
                .child(text(name).font(TYPE.small).color(ink).grow(1.0).a11y_hidden())
                .child(
                    text(value)
                        .font(TextStyle {
                            weight: 600.0,
                            ..TYPE.small.features("'tnum' 1")
                        })
                        .color(if warn { k.c.warning.fg } else { k.c.text_base })
                        .a11y_hidden(),
                ),
        );
    }
    card("brief-facts", "Facts", "from the diff", list, k)
}

fn map_card(s: &App, k: &Kit) -> Option<El> {
    let n = s.overview.as_ref()?.document.graph.nodes.len();
    let body = row().pt(4.0).child(
        outline("open-map-card", "Open map", Some("m"), 24.0, k)
            .help("Open the Map  m")
            .on_press(|s: &mut App| s.toggle_map()),
    );
    let sub = if n == 1 { "· 1 node".to_owned() } else { format!("· {n} nodes") };
    Some(card("overview-map", "Map", &sub, body, k))
}

/// The space above an H2 (none above the first).
fn rule(key: &str, first: bool, _: &Kit) -> El {
    let c = col().key(key.to_owned()).w_full();
    if first { c } else { c.pt(8.0) }
}

/// A rule above an H2 (except the first), then the heading.
fn h2_block(key: &str, title: &str, first: bool, k: &Kit) -> El {
    rule(key, first, k).gap(8.0).child(h2(title.to_owned(), k))
}

fn sections(s: &App, d: &BriefDoc, k: &Kit) -> El {
    let mut c = col().key("brief-sections").w_full().gap(10.0);
    let mut first = true;
    let mut h = |key: &str, title: Option<&str>| {
        let e = match title {
            Some(t) => h2_block(key, t, first, k),
            None => rule(key, first, k),
        };
        first = false;
        e
    };
    if let Some(sec) = &d.intent {
        c = c.child(h("h2-intent", None));
        c = c.child(section(
            s,
            k,
            &sec.id,
            Some(("Intent", true)),
            "Intent",
            &sec.text,
            &sec.cites,
            None,
        ));
    }
    if !d.how.is_empty() {
        c = c.child(h("h2-how", Some("How it works")));
        for (i, how) in d.how.iter().enumerate() {
            let title = format!("{}. {}", i + 1, how.title);
            c = c.child(section(s, k, &how.id, Some((&title, false)), &title, &how.text, &how.cites, None));
        }
    }
    if !d.invariants.is_empty() {
        c = c.child(h("h2-invariants", Some("Invariants")));
        for (i, v) in d.invariants.iter().enumerate() {
            let mut extra = col().gap(2.0);
            if !v.enforced.is_empty() {
                extra = extra.child(labelled_chips(s, k, &format!("{}-enforced", v.id), "Enforced", &v.enforced));
            }
            if !v.tested.is_empty() {
                extra = extra.child(labelled_chips(s, k, &format!("{}-tested", v.id), "Tested", &v.tested));
            }
            c = c.child(invariant(s, k, i + 1, &v.id, &v.text, extra));
        }
    }
    if let Some(sec) = &d.not_changed {
        c = c.child(h("h2-not-changed", None));
        c = c.child(section(
            s,
            k,
            &sec.id,
            Some(("Not changed", true)),
            "Not changed",
            &sec.text,
            &sec.cites,
            None,
        ));
    }
    if let Some(n) = &d.not_covered {
        c = c.child(h("h2-not-covered", None));
        c = c.child(section(
            s,
            k,
            &n.id,
            Some(("Not covered", true)),
            "Not covered",
            &n.text,
            &[],
            Some(not_covered_files(s, k)),
        ));
    }
    if let Some(sec) = &d.verify {
        c = c.child(h("h2-verify", None));
        c = c.child(section(
            s,
            k,
            &sec.id,
            Some(("Verify by hand", true)),
            "Verify by hand",
            &sec.text,
            &sec.cites,
            None,
        ));
    }
    c
}

fn not_covered_files(s: &App, k: &Kit) -> El {
    let files = s.not_covered_files();
    let toggle = row()
        .gap(8.0)
        .items_center()
        .child(muted(format!("{} file(s)", files.len()), k))
        .child(
            Button::new(if s.brief.show_not_covered { "Hide files" } else { "Show files" })
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .disabled(files.is_empty())
                .on_press(|s: &mut App| s.brief.show_not_covered = !s.brief.show_not_covered)
                .build(k)
                .key("not-covered-toggle"),
        );
    if !s.brief.show_not_covered {
        return toggle;
    }
    let mut list = col().key("not-covered-files").gap(1.0);
    for f in files.iter().take(60) {
        let key = f.clone();
        list = list.child(
            text(f.clone())
                .font(k.ty.mono)
                .color(k.c.text_muted)
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_key(&key, tx)),
        );
    }
    if files.len() > 60 {
        list = list.child(muted(format!("… {} more", files.len() - 60), k));
    }
    col().gap(4.0).child(toggle).child(list)
}

/// Comments whose section is gone, at the end of the page.
fn orphans(s: &App, k: &Kit) -> Option<El> {
    s.brief_doc()?;
    let orphans = s.orphaned_brief_threads();
    if orphans.is_empty() {
        return None;
    }
    let mut o = col()
        .key("brief-orphans")
        .role("group")
        .label("Comments whose section is gone")
        .w_full()
        .gap(6.0)
        .child(h2_block(
            "h2-orphans",
            &format!("Comments whose section is gone ({})", orphans.len()),
            false,
            k,
        ));
    for t in orphans {
        o = o.child(orphan_card(s, t, k));
    }
    Some(o)
}

/// The head of a commentable section: its H3 (How items), a thread dot, and Comment / Ask while
/// hovered or focused.
/// `title` is `(text, is_h2)`: a section that is the whole of its H2 carries it in its head.
fn section_head(s: &App, k: &Kit, id: &str, title: Option<(&str, bool)>, threads: usize) -> El {
    let mut head = row()
        .key(format!("sec-head-{id}"))
        .w_full()
        .min_h(24.0)
        .gap(8.0)
        .items_center()
        .on_press({
            let id = id.to_owned();
            move |s: &mut App| s.focus_section(&id)
        });
    if let Some((t, top)) = title {
        head = head.child(if top { h2(t.to_owned(), k) } else { h3(t.to_owned(), k) }.shrink());
    }
    if threads > 0 {
        head = head.child(dot(threads, k).key("sec-dot"));
    }
    head = head.child(el().grow(1.0));
    let shown = s.brief.focus.as_deref() == Some(id) || s.brief.hover_section.as_deref() == Some(id);
    if shown {
        head = head.child(mode_buttons(k, id.to_owned()));
    }
    head
}

/// Hover shows the section's Comment / Ask; a right-click opens its menu.
fn hoverable(c: El, id: &str) -> El {
    let (id, menu) = (id.to_owned(), id.to_owned());
    c.on_hover_change(move |s: &mut App, inside, _: &mut Tx| {
        if inside {
            s.brief.hover_section = Some(id.clone());
        } else if s.brief.hover_section.as_deref() == Some(id.as_str()) {
            s.brief.hover_section = None;
        }
    })
    .on_secondary_press(move |s: &mut App, p: rune::PointerPress, _: &mut Tx| s.open_section_menu(&menu, (p.x, p.y)))
}

#[allow(clippy::too_many_arguments)]
fn section(s: &App, k: &Kit, id: &str, title: Option<(&str, bool)>, label: &str, body: &str, cites: &[Cite], extra: Option<El>) -> El {
    let threads = s.section_threads(id);
    let mut c = col()
        .key(format!("brief-sec-{id}"))
        .role("article")
        .label(label.to_owned())
        .w_full()
        .gap(6.0)
        .pl(12.0)
        .pr(12.0)
        .pt(8.0)
        .pb(4.0)
        .mx(-12.0)
        .radius(radius::CARD)
        .child(section_head(s, k, id, title, threads.len()));
    if s.brief.hover_section.as_deref() == Some(id) || s.brief.focus.as_deref() == Some(id) {
        c = c.fill(k.c.hover);
    }
    if !body.trim().is_empty() {
        c = c.child(md_body(s, id, body, k));
    }
    let inline = inline_cites(body, &s.keys);
    let shown: Vec<(usize, &Cite)> = cites
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            let key = rain_overview::brief::resolve(&c.path, &s.keys);
            !key.is_some_and(|key| inline.iter().any(|(k, l)| k == key && *l == c.lines))
        })
        .collect();
    if !shown.is_empty() {
        c = c.child(chips_at(s, k, id, cites, &shown));
    }
    if let Some(e) = extra {
        c = c.child(e);
    }
    for e in tail(s, id, &threads, k) {
        c = c.child(e);
    }
    hoverable(c, id)
}

/// One invariant as an ordered list item: its number, its text and its cites.
fn invariant(s: &App, k: &Kit, n: usize, id: &str, body: &str, extra: El) -> El {
    let threads = s.section_threads(id);
    let mut item = col()
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(section_head(s, k, id, None, threads.len()))
        .child(md_body(s, id, body, k))
        .child(extra);
    for e in tail(s, id, &threads, k) {
        item = item.child(e);
    }
    let mut c = row()
        .key(format!("brief-sec-{id}"))
        .role("listitem")
        .label(format!("Invariant {n}"))
        .w_full()
        .gap(8.0)
        .pl(12.0)
        .pr(12.0)
        .pt(6.0)
        .pb(4.0)
        .mx(-12.0)
        .radius(radius::CARD)
        .items_start()
        // The head row is 24 points, then a 4 point gap: the number sits on the body's first line.
        .child(text(format!("{n}.")).font(TYPE.body).color(k.c.text_muted).w(20.0).pt(28.0))
        .child(item);
    if s.brief.hover_section.as_deref() == Some(id) || s.brief.focus.as_deref() == Some(id) {
        c = c.fill(k.c.hover);
    }
    hoverable(c, id)
}

/// The threads and the composer under section `id`.
fn tail(s: &App, id: &str, threads: &[&Thread], k: &Kit) -> Vec<El> {
    let mut out: Vec<El> = threads.iter().map(|t| thread_card(s, t, k)).collect();
    if let Some(d) = &s.brief.composer
        && d.target.section.as_deref() == Some(id)
        && d.target.kind == rain_core::TargetKind::Brief
    {
        out.push(artifact_composer(s, d, k));
    }
    out
}

/// Comment / Ask on a hovered or focused section.
fn mode_buttons(k: &Kit, id: String) -> El {
    let ask = id.clone();
    row()
        .key("sec-actions")
        .gap(4.0)
        .child(
            outline("sec-comment", "Comment", Some("c"), 22.0, k)
                .on_press(move |s: &mut App| s.comment_on_section(&id, None, ComposeMode::Comment)),
        )
        .child(
            outline("sec-ask", "Ask", Some("?"), 22.0, k)
                .on_press(move |s: &mut App| s.comment_on_section(&ask, None, ComposeMode::Question)),
        )
}

/// A thread-count chip on a section head.
pub(super) fn dot(n: usize, k: &Kit) -> El {
    row()
        .h(18.0)
        .px(6.0)
        .radius(radius::CHIP)
        .fill(k.c.info.bg)
        .items_center()
        .label(format!("{n} thread(s)"))
        .child(text(n.to_string()).font(TYPE.micro).color(k.c.info.fg))
}

/// Byte ranges of `src` inside fenced code blocks (``` or ~~~), fences included.
fn fenced(src: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut open: Option<(usize, char, usize)> = None;
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start_matches(' ');
        let indent = line.len() - t.len();
        let run = |c: char| t.chars().take_while(|&x| x == c).count();
        match open {
            None if indent <= 3 => {
                if let Some(c) = ['`', '~'].into_iter().find(|&c| run(c) >= 3) {
                    open = Some((at, c, run(c)));
                }
            }
            Some((start, c, n)) if indent <= 3 && run(c) >= n && t[run(c)..].trim().is_empty() => {
                out.push(start..at + line.len());
                open = None;
            }
            _ => {}
        }
        at += line.len();
    }
    if let Some((start, ..)) = open {
        out.push(start..src.len());
    }
    out
}

/// Whether `t` (a line without its indent) starts a list item or another block a setext
/// underline can't turn into a heading.
fn starts_block(t: &str) -> bool {
    t.starts_with(['-', '*', '+', '>', '|', '#'])
        || t.split_once(['.', ')'])
            .is_some_and(|(n, _)| !n.is_empty() && n.len() <= 9 && n.chars().all(|c| c.is_ascii_digit()))
}

/// Headings inside a section body become H4 (`####`), so a body never outranks the page's own
/// headings; fenced code is left alone.
pub(crate) fn demote_headings(src: &str) -> String {
    let code = fenced(src);
    let in_code = |i: usize| code.iter().any(|r| r.contains(&i));
    let lines: Vec<(usize, &str)> = src
        .split_inclusive('\n')
        .scan(0, |at, l| {
            let start = *at;
            *at += l.len();
            Some((start, l))
        })
        .collect();
    let mut out = String::with_capacity(src.len() + 16);
    let mut i = 0;
    while i < lines.len() {
        let (at, line) = lines[i];
        i += 1;
        if in_code(at) {
            out.push_str(line);
            continue;
        }
        let body = line.trim_end_matches(['\n', '\r']);
        let t = body.trim_start_matches(' ');
        let indent = body.len() - t.len();
        let hashes = t.chars().take_while(|&c| c == '#').count();
        if indent <= 3 && (1..=6).contains(&hashes) && (t.len() == hashes || t[hashes..].starts_with([' ', '\t'])) {
            out.push_str("####");
            out.push_str(&t[hashes..]);
            out.push_str(&line[body.len()..]);
            continue;
        }
        if let Some(&(next_at, next)) = lines.get(i) {
            let u = next.trim();
            let underline = !u.is_empty() && (u.chars().all(|c| c == '=') || (u.len() >= 2 && u.chars().all(|c| c == '-')));
            if underline && indent <= 3 && !t.trim().is_empty() && !starts_block(t) && !in_code(next_at) {
                out.push_str("#### ");
                out.push_str(t.trim());
                out.push_str(&next[next.trim_end_matches(['\n', '\r']).len()..]);
                i += 1;
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

/// `rain-cite:<key>#a-b` (or without lines).
pub(crate) fn cite_url(key: &str, lines: Option<(u32, u32)>) -> String {
    match lines {
        Some((a, b)) => format!("{CITE_SCHEME}{key}#{a}-{b}"),
        None => format!("{CITE_SCHEME}{key}"),
    }
}

/// The key and lines of a `rain-cite:` URL.
pub(crate) fn parse_cite(url: &str) -> Option<(String, Option<(u32, u32)>)> {
    let rest = url.strip_prefix(CITE_SCHEME)?;
    match rest.rsplit_once('#') {
        Some((key, l)) => {
            let (a, b) = l.split_once('-').unwrap_or((l, l));
            Some((key.to_owned(), a.parse().ok().zip(b.parse().ok())))
        }
        None => Some((rest.to_owned(), None)),
    }
}

/// Code spans naming a diff file (`` `VoucherService.cs:27-31` ``) become `rain-cite:` links around
/// the same code span; fenced code and spans already inside a link are left alone.
pub(crate) fn link_cites(src: &str, keys: &[String]) -> String {
    let code = fenced(src);
    let mut out = String::with_capacity(src.len() + 64);
    let mut i = 0;
    while i < src.len() {
        if let Some(r) = code.iter().find(|r| r.start == i) {
            out.push_str(&src[r.clone()]);
            i = r.end;
            continue;
        }
        if !src[i..].starts_with('`') {
            let tick = src[i..].find('`').map_or(src.len(), |n| i + n);
            let next = code.iter().map(|r| r.start).filter(|&s| s > i && s < tick).min().unwrap_or(tick);
            out.push_str(&src[i..next]);
            i = next;
            continue;
        }
        let n = src[i..].bytes().take_while(|&c| c == b'`').count();
        let ticks = &src[i..i + n];
        let close = src[i + n..]
            .match_indices(ticks)
            .map(|(x, _)| i + n + x)
            .find(|&c| !src[c + n..].starts_with('`') && !src[..c].ends_with('`'));
        let Some(close) = close.filter(|&c| !code.iter().any(|r| r.contains(&c))) else {
            out.push_str(ticks);
            i += n;
            continue;
        };
        let span = &src[i..close + n];
        let inner = src[i + n..close].trim();
        let linked = src[..i].ends_with('[');
        let (path, lines) = rain_overview::brief::split_lines(inner);
        let key = (!linked && path.contains('.') && !path.contains(' '))
            .then(|| rain_overview::brief::resolve(path, keys))
            .flatten();
        match key {
            Some(k) => out.push_str(&format!("[{span}](<{}>)", cite_url(k, lines))),
            None => out.push_str(span),
        }
        i = close + n;
    }
    out
}

/// A section body as Markdown: headings demoted, cites linked. A cite link opens the peek under
/// the body (Ctrl+click jumps); other links open in the browser. Selecting text in it focuses
/// the section and keeps the text for a comment.
pub(super) fn md_body(s: &App, id: &str, body: &str, k: &Kit) -> El {
    let src = link_cites(&demote_headings(body), &s.keys);
    let anchor = format!("md-{id}");
    let (link_anchor, sel_id) = (anchor.clone(), id.to_owned());
    let md = Markdown::new(src)
        .style(
            super::ui::md_style(k)
                .inline_code_bg(rune::Color::TRANSPARENT)
                .inline_code_pad(1.0)
                .syntax_theme(s.seen_scheme.get().code_kinds(k)),
        )
        .selectable(true)
        .on_link_tx(move |s: &mut App, url: &str, tx: &mut Tx| {
            let Some((key, lines)) = parse_cite(url) else {
                let _ = tx.open_url(url);
                return;
            };
            let m = tx.mods();
            if m.ctrl || m.cmd {
                s.close_peek();
                s.goto_cite(&key, lines, Side::New, tx);
                return;
            }
            s.open_peek(cite_target(s, key, lines, &link_anchor), true);
        })
        .build(k);
    col()
        .key(anchor)
        .w_full()
        .child(md)
        .on_selection_change(move |s: &mut App, change: &rune::SelectionChange, _: &mut Tx| {
            match change.selection().map(|t| t.text().trim().to_owned()).filter(|t| !t.is_empty()) {
                Some(t) if !change.dragging() => {
                    s.focus_section(&sel_id);
                    s.brief.selection = Some((sel_id.clone(), t));
                }
                Some(_) => {}
                None => {
                    if s.brief.selection.as_ref().is_some_and(|(x, _)| *x == sel_id) {
                        s.brief.selection = None;
                    }
                }
            }
        })
}

/// The peek for a cite link of a body keyed `anchor`.
pub(crate) fn cite_target(s: &App, key: String, lines: Option<(u32, u32)>, anchor: &str) -> PeekTarget {
    let spec = match lines {
        Some((a, b)) => format!("{key}:{a}-{b}"),
        None => key.clone(),
    };
    let label = rain_overview::brief::cite_labels(&[spec.as_str()], &s.keys).remove(0);
    PeekTarget {
        key,
        lines,
        side: Side::New,
        anchor: anchor.to_owned(),
        label,
    }
}

/// One cite chip keyed `key`: a hover dwell or Space peeks, a press or Enter jumps; `None` for
/// the key when the cite names no diff file (a greyed chip).
pub(super) fn cite_chip(label: String, help: String, file: Option<String>, c: &Cite, key: String, k: &Kit) -> El {
    let (path, lines, side) = (c.path.clone(), c.lines, c.side.unwrap_or(Side::New));
    let chip = Button::new(label.clone())
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .disabled(file.is_none())
        .help(help)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| {
            s.close_peek();
            s.goto_cite(&path, lines, side, tx)
        })
        .build(k)
        .key(key.clone());
    let Some(file) = file else { return chip };
    let target = PeekTarget {
        key: file,
        lines,
        side,
        anchor: key,
        label,
    };
    let pressed = target.clone();
    chip.on_hover_change(move |s: &mut App, inside, tx: &mut Tx| s.peek_hover(target.clone(), inside, tx))
        .on_key(move |s: &mut App, ev: &rune::KeyEvent, tx: &mut Tx| match &ev.key {
            rune::Key::Text(t) if t == " " => {
                s.open_peek(pressed.clone(), true);
                true
            }
            rune::Key::Enter => {
                s.close_peek();
                s.goto_cite(&pressed.key, pressed.lines, pressed.side, tx);
                true
            }
            rune::Key::Escape => s.close_peek(),
            _ => false,
        })
}

/// The diff files and lines a body already links inline.
pub(crate) fn inline_cites(body: &str, keys: &[String]) -> Vec<(String, Option<(u32, u32)>)> {
    let src = link_cites(&demote_headings(body), keys);
    src.match_indices(&format!("(<{CITE_SCHEME}"))
        .filter_map(|(at, _)| {
            let url = &src[at + 2..];
            parse_cite(&url[..url.find('>')?])
        })
        .collect()
}

/// Cite chips: disambiguated labels, the full path as help.
fn chips(s: &App, k: &Kit, id: &str, cites: &[Cite]) -> El {
    let all: Vec<(usize, &Cite)> = cites.iter().enumerate().collect();
    chips_at(s, k, id, cites, &all)
}

/// The chips of `shown` (index into `cites`, cite), labelled against every cite of the section.
fn chips_at(s: &App, k: &Kit, id: &str, cites: &[Cite], shown: &[(usize, &Cite)]) -> El {
    let specs: Vec<String> = cites
        .iter()
        .map(|c| match c.lines {
            Some((a, b)) => format!("{}:{a}-{b}", c.path),
            None => c.path.clone(),
        })
        .collect();
    let refs: Vec<&str> = specs.iter().map(String::as_str).collect();
    let labels = rain_overview::brief::cite_labels(&refs, &s.keys);
    let mut r = row().key(format!("cites-{id}")).gap(4.0).flex_wrap();
    for &(i, c) in shown {
        let label = labels[i].clone();
        let file = rain_overview::brief::resolve(&c.path, &s.keys).map(str::to_owned);
        let help = match &file {
            Some(f) => f.clone(),
            None => format!("{} is not in this diff", c.path),
        };
        r = r.child(cite_chip(label, help, file, c, format!("cite-{id}-{i}"), k));
    }
    r
}

fn labelled_chips(s: &App, k: &Kit, key: &str, label: &str, cites: &[Cite]) -> El {
    row()
        .key(key.to_owned())
        .gap(6.0)
        .items_center()
        .child(muted(label, k).w(64.0))
        .child(chips(s, k, key, cites))
}

/// A thread on an artifact: the comment card, located by its artifact's title.
pub(super) fn thread_card(s: &App, t: &Thread, k: &Kit) -> El {
    let mut c = ThreadCard::new(t)
        .collapsed(!s.expanded.contains(&t.id))
        .focused(s.focused_thread.as_deref() == Some(t.id.as_str()))
        .location(s.artifact_title(t))
        .on_action(|s: &mut App, id: &str, a, tx: &mut Tx| s.card_action(id, a, tx));
    if s.reply_to.as_deref() == Some(t.id.as_str()) {
        c = c.reply_composer(&s.reply_edit, |s: &mut App| &mut s.reply_edit);
    }
    col().key(format!("athread-{}", t.id)).w_full().child(c.build(k))
}

/// An orphaned Brief comment: why it lost its section and the text it was posted on.
fn orphan_card(s: &App, t: &Thread, k: &Kit) -> El {
    let snap = t
        .orphan
        .as_ref()
        .map(|o| o.snapshot.content.clone())
        .or_else(|| t.code_snapshot.as_ref().map(|c| c.content.clone()))
        .unwrap_or_default();
    let mut c = col().key(format!("orphan-{}", t.id)).w_full().gap(4.0).child(
        row()
            .gap(6.0)
            .items_center()
            .child(
                row()
                    .px(6.0)
                    .radius(k.radius.sm)
                    .fill(k.c.warning.bg)
                    .child(text("orphaned · section removed").font(k.ty.small_medium).color(k.c.warning.fg)),
            )
            .child(muted("last seen:", k)),
    );
    if !snap.is_empty() {
        c = c.child(
            row()
                .w_full()
                .gap(8.0)
                .padding(6.0)
                .radius(k.radius.sm)
                .fill(k.c.layer_02)
                .child(el().w(3.0).h_full().fill(k.c.border_strong))
                .child(
                    text(snap)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap()
                        .max_lines(6)
                        .grow(1.0)
                        .shrink(),
                ),
        );
    }
    c.child(thread_card(s, t, k))
}

/// The artifact composer: Comment or Question, then the text area.
pub(super) fn artifact_composer(_s: &App, d: &ArtifactDraft, k: &Kit) -> El {
    let i = match d.mode {
        ComposeMode::Comment => 0,
        ComposeMode::Question => 1,
    };
    let modes = SegmentedControl::new(&["Comment", "Question"], Some(i))
        .label("Mode")
        .on_pick(|s: &mut App, i, _: &mut Tx| s.set_artifact_mode(if i == 0 { ComposeMode::Comment } else { ComposeMode::Question }))
        .build(k);
    let hint = match d.mode {
        ComposeMode::Comment => "A review comment on this part of the brief",
        ComposeMode::Question => "Claude answers here, as a reply",
    };
    let placeholder = match d.mode {
        ComposeMode::Comment => "Comment… (Ctrl+Enter to send, Esc to cancel)",
        ComposeMode::Question => "Ask Claude… (Ctrl+Enter to send, Esc to cancel)",
    };
    let edit = &_s.brief.edit;
    col()
        .key("artifact-composer")
        .w_full()
        .gap(4.0)
        .child(row().gap(8.0).items_center().child(modes).child(muted(hint, k)))
        .child(
            Composer::new(edit, |s: &mut App| &mut s.brief.edit)
                .key("artifact-field")
                .anchor_label(d.title.clone())
                .placeholder(placeholder)
                .on_submit(|s: &mut App, body: String, _: &mut Tx| s.submit_artifact(body))
                .on_cancel(|s: &mut App, _: &mut Tx| {
                    s.cancel_artifact();
                })
                .build(k),
        )
}

/// The no-brief card's text, naming only what this page really shows.
fn fallback_copy(s: &App) -> String {
    let from_diff = if s.overview.is_some() {
        "Facts and the Map come from the diff"
    } else {
        "Facts come from the diff"
    };
    let until = if s.story_notes().is_empty() {
        format!("Until then, {from_diff}.")
    } else {
        format!("Until then, the agent's story notes stand in below, and {from_diff}.")
    };
    format!("**No brief yet.** Ask the agent to run `rain brief set` (see docs/agent.md).\n\n{until}")
}

/// No brief yet: the story notes with Comment / Ask per note, and how to get a brief.
fn fallback(s: &App, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let mut c = col().key("brief-fallback").w_full().gap(16.0).child(
        col()
            .key("overview-empty")
            .w_full()
            .px(20.0)
            .py(16.0)
            .gap(8.0)
            .radius(radius::CARD)
            .fill(surf.panel)
            .border(1.0, surf.card_border)
            .child(Markdown::new(fallback_copy(s)).selectable(true).build(k)),
    );
    if let Some(story) = notes::story_card(s, k) {
        c = c.child(story);
    }
    for n in s.story_notes() {
        let (id, ask) = (n.id.clone(), n.id.clone());
        let name = n
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| "Story note".into());
        let mut note = col().key(format!("story-actions-{}", n.id)).w_full().gap(4.0).child(
            row()
                .gap(4.0)
                .items_center()
                .child(muted(name, k))
                .child(
                    Button::new("Comment")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(move |s: &mut App| s.comment_on_note(&id, ComposeMode::Comment))
                        .build(k)
                        .key("note-comment"),
                )
                .child(
                    Button::new("Ask")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(move |s: &mut App| s.comment_on_note(&ask, ComposeMode::Question))
                        .build(k)
                        .key("note-ask"),
                ),
        );
        for t in s.note_threads(&n.id) {
            note = note.child(thread_card(s, t, k));
        }
        if let Some(d) = &s.brief.composer
            && d.target.note.as_deref() == Some(n.id.as_str())
        {
            note = note.child(artifact_composer(s, d, k));
        }
        c = c.child(note);
    }
    c
}
