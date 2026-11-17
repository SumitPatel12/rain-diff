//! Thread cards and the composer on sample threads. `--snap out.png --scene expanded --theme light`
//! renders one headless; scenes: collapsed, expanded, composer, thread-3-replies.

use std::collections::HashSet;

use rain_comments::{CardAction, Composer, ThreadCard, anchor_label, apply, from_difit_json, ops};
use rain_core::{Anchor, CodeSnapshot, Session, Side};
use rune::{AppSpec, Cx, El, KeyEvent, TextEdit, Tx, col, text};
use rune_kit::Kit;

const IMPORT: &str = include_str!("../../../fixtures/difit/import.json");
const SPAN: Anchor = Anchor::Span {
    side: Side::New,
    start_line: 12,
    start_col: 4,
    end_line: 14,
    end_col: 19,
};

struct Demo {
    s: Session,
    expanded: HashSet<String>,
    focus: usize,
    composing: bool,
    edit: TextEdit,
    replying: Option<String>,
    reply: TextEdit,
}

fn init() -> Demo {
    let mut s = Session::default();
    apply(&mut s, from_difit_json(IMPORT).expect("fixture"), "app");
    let snap = CodeSnapshot {
        content: "decimal voucher = account.Voucher ?? 0;".into(),
        ..CodeSnapshot::default()
    };
    let id = ops::add_thread(
        &mut s,
        "server",
        "src/VoucherService.cs",
        SPAN,
        "question: should a missing voucher be zero or null here?",
        Some("claude"),
        Some(snap),
    );
    ops::set_disposition(&mut s, &id, Some(rain_core::Disposition::Ask));
    Demo {
        s,
        expanded: HashSet::new(),
        focus: 0,
        composing: false,
        edit: TextEdit::default(),
        replying: None,
        reply: TextEdit::default(),
    }
}

fn act(d: &mut Demo, id: &str, a: CardAction, _: &mut Tx) {
    match a {
        CardAction::Toggle => {
            if !d.expanded.remove(id) {
                d.expanded.insert(id.to_owned());
            }
        }
        CardAction::OpenReply | CardAction::AskReply => {
            d.expanded.insert(id.to_owned());
            d.replying = Some(id.to_owned());
        }
        CardAction::CancelReply => d.replying = None,
        CardAction::Reply(body) => {
            ops::reply(&mut d.s, id, &body, None);
            d.replying = None;
        }
        CardAction::Disposition(x) => {
            ops::toggle_disposition(&mut d.s, id, x);
        }
        CardAction::Delete => {
            ops::delete_thread(&mut d.s, id);
        }
        CardAction::Dismiss(r) => {
            ops::dismiss(&mut d.s, id, r);
        }
        CardAction::Resolve | CardAction::CloseOrphan => {
            ops::resolve(&mut d.s, id);
        }
        CardAction::Reopen => {
            ops::reopen(&mut d.s, id);
        }
        CardAction::Confirm => {
            ops::confirm_addressed(&mut d.s, id);
        }
        CardAction::EditMessage(_)
        | CardAction::OpenDismiss
        | CardAction::Retry
        | CardAction::CancelAnswer
        | CardAction::Attach
        | CardAction::EmptyReply
        | CardAction::Hover(_) => {}
    }
}

fn view(d: &Demo, cx: &mut Cx<Demo>) -> El {
    let k = Kit::neutral(cx.system_dark());
    let mut list = col().key("threads").w_full().gap(8.0);
    for (i, t) in d.s.threads.iter().enumerate() {
        let mut card = ThreadCard::new(t)
            .collapsed(!d.expanded.contains(&t.id))
            .focused(i == d.focus)
            .location(anchor_label(&format!("{}/{}", t.repo, t.file_path), &t.anchor))
            .on_action(act);
        if d.replying.as_deref() == Some(t.id.as_str()) {
            card = card.reply_composer(&d.reply, |d: &mut Demo| &mut d.reply);
        }
        list = list.child(card.build(&k));
    }
    let mut page = col().w_full().h_full().padding(16.0).gap(12.0).fill(k.c.bg_base).child(
        text(format!(
            "{} threads · waiting on you: {}",
            d.s.threads.len(),
            ops::waiting_count(&d.s, "sumit")
        ))
        .font(k.ty.ui_medium)
        .color(k.c.text_muted),
    );
    if d.composing {
        page = page.child(
            Composer::new(&d.edit, |d: &mut Demo| &mut d.edit)
                .anchor_label(anchor_label("server/src/VoucherService.cs", &SPAN))
                .preview("decimal voucher = account.Voucher ?? 0;\nreturn Round(voucher);\n_db.Save();")
                .on_submit(|d: &mut Demo, body: String, _: &mut Tx| {
                    ops::add_thread(&mut d.s, "server", "src/VoucherService.cs", SPAN, &body, None, None);
                    d.composing = false;
                })
                .on_cancel(|d: &mut Demo, _: &mut Tx| d.composing = false)
                .build(&k),
        );
    }
    page.child(list)
}

fn on_key(d: &mut Demo, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match &ev.key {
        rune::Key::Text(t) if t == "j" => d.focus = (d.focus + 1).min(d.s.threads.len().saturating_sub(1)),
        rune::Key::Text(t) if t == "k" => d.focus = d.focus.saturating_sub(1),
        rune::Key::Text(t) if t == "c" => d.composing = true,
        _ => {
            let Some(a) = rain_comments::card_key(ev) else { return false };
            let Some(id) = d.s.threads.get(d.focus).map(|t| t.id.clone()) else {
                return false;
            };
            act(d, &id, a, tx);
        }
    }
    true
}

fn main() {
    rune::run(AppSpec {
        scenes: &[
            ("collapsed", |_: &mut Demo| {}),
            ("expanded", |d| {
                d.expanded.insert("t-range".into());
                d.focus = 1;
            }),
            ("composer", |d| {
                d.composing = true;
                d.edit = TextEdit::new("Zero hides a missing voucher from the payout report.\nPrefer null and show “no voucher”.");
            }),
            ("thread-3-replies", |d| {
                ops::reply(&mut d.s, "t-line", "Guard added in the next round.", Some("claude"));
                d.expanded.insert("t-line".into());
                d.replying = Some("t-line".into());
                d.reply = TextEdit::new("Looks good, resolving.");
            }),
        ],
        on_key: Some(on_key),
        ..AppSpec::new("Rain comments", (900.0, 760.0), init, view)
    });
}
