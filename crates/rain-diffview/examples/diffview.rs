//! The diff view on the committed fixtures: `fixtures/s.patch` by default, `m.patch` in scene
//! `m`, split and wrapped modes, line/range/span comments, and the line cursor.
//! `cargo run -p rain-diffview --example diffview -- --scene annotated --snap out.png`
//! writes a still; without `--snap` the window opens (`j`/`k` line, `n`/`p` hunk, `]`/`[` file,
//! `s` toggles split).

#[path = "../tests/common/patch.rs"]
mod patch;

use rain_core::comments::Anchor;
use rain_core::diff::Side;
use rain_diffview::{Annotation, DiffMode, DiffState, DiffView, LineRef, SpanSelection, Spot};
use rune::{App, AppSpec, Cx, El, KeyEvent, Tx, col, row, text};
use rune_kit::Kit;

const S: &str = include_str!("../../../fixtures/s.patch");
const M: &str = include_str!("../../../fixtures/m.patch");
const ONE_SIDED: &str = "diff --git a/src/vouchers.rs b/src/vouchers.rs
--- a/src/vouchers.rs
+++ b/src/vouchers.rs
@@ -1,4 +1,4 @@
 pub fn total(account: &Account) -> Money {
-    account.lines.iter().map(|l| l.amount).sum()
+    account.lines.iter().map(|l| l.amount).sum::<Money>() + account.voucher
 }

diff --git a/src/voucher_service.rs b/src/voucher_service.rs
new file mode 100644
--- /dev/null
+++ b/src/voucher_service.rs
@@ -0,0 +1,8 @@
+pub struct VoucherService { repo: Repo }
+
+impl VoucherService {
+    pub fn record(&self, account: u64, amount: Money) -> Result<(), Error> {
+        if amount <= Money::ZERO { return Err(Error::Invalid); }
+        self.repo.in_unit_of_work(|uow| uow.add_voucher(account, amount))
+    }
+}
diff --git a/src/legacy_totals.rs b/src/legacy_totals.rs
deleted file mode 100644
--- a/src/legacy_totals.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-pub fn legacy_total(lines: &[Line]) -> Money {
-    lines.iter().map(|l| l.amount).sum()
-}
";

struct Review {
    diff: DiffState,
    dark: Option<bool>,
    span: Option<SpanSelection>,
    status: String,
}

fn init() -> Review {
    Review {
        diff: DiffState::new(patch::parse(S)),
        dark: None,
        span: None,
        status: String::new(),
    }
}

/// The first file with at least `n` new-side lines in its first hunk, and that hunk's first new line.
fn hunk_with(d: &DiffState, n: u32) -> Option<(usize, u32)> {
    d.files()
        .iter()
        .enumerate()
        .find_map(|(i, f)| f.hunks.first().filter(|h| h.new_count >= n).map(|h| (i, h.new_start)))
}

fn annotate(s: &mut Review) {
    let Some((f, n)) = hunk_with(&s.diff, 6) else { return };
    s.diff.set_annotations(vec![
        Annotation::line(f, LineRef::new(Side::New, n), "line"),
        Annotation::new(
            f,
            Anchor::Range {
                side: Side::New,
                start: n + 1,
                end: n + 2,
            },
            "range",
        ),
        Annotation::new(
            f,
            Anchor::Span {
                side: Side::New,
                start_line: n + 3,
                start_col: 2,
                end_line: n + 4,
                end_col: 10,
            },
            "span",
        ),
    ]);
}

fn view(s: &Review, cx: &mut Cx<Review>) -> El {
    let k = Kit::neutral(s.dark.unwrap_or(cx.system_dark()));
    let (adds, dels) = s.diff.files().iter().fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
    let mode = match (s.diff.mode(), s.diff.layout_reason()) {
        (DiffMode::Split, Some(why)) => format!("split · narrow: {why}"),
        (DiffMode::Split, None) => "split".to_owned(),
        _ => "unified".to_owned(),
    };
    let span = s.span.as_ref().map_or(String::new(), |sp| {
        format!("  selection {}:{}–{}:{}", sp.start_line, sp.start_col, sp.end_line, sp.end_col)
    });
    let bar = row()
        .key("bar")
        .w_full()
        .h(40.0)
        .px(16.0)
        .gap(12.0)
        .items_center()
        .fill(k.c.layer_01)
        .child(text("Rain diffview").font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            text(format!(
                "{} files  +{adds} −{dels}  {mode}{span}  {}",
                s.diff.files().len(),
                s.status
            ))
            .font(k.ty.ui)
            .color(k.c.text_muted),
        );
    let kc = k.clone();
    let diff = DiffView::new(|s: &Review| &s.diff, |s: &mut Review| &mut s.diff)
        .calm_added_files(true)
        .annotation(move |_: &Review, a: &Annotation| card(&kc, a))
        .on_comment(|s: &mut Review, lines, _| s.status = format!("comment on {:?}", lines.anchor()))
        .on_span_select(|s: &mut Review, span, _| s.span = span)
        .build(s, &k);
    col().w_full().h_full().fill(k.c.bg_base).child(bar).child(diff)
}

fn card(k: &Kit, a: &Annotation) -> El {
    let what = match a.anchor {
        Anchor::Line { line, .. } => format!("on line {line}"),
        Anchor::Range { start, end, .. } => format!("on lines {start}–{end}"),
        Anchor::Span {
            start_line,
            start_col,
            end_line,
            end_col,
            ..
        } => format!("on {start_line}:{start_col}–{end_line}:{end_col}"),
    };
    let body = col()
        .w_full()
        .padding(10.0)
        .gap(4.0)
        .radius(6.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_weak)
        .child(
            row()
                .gap(8.0)
                .child(text("claude").font(k.ty.ui_medium).color(k.c.text_base))
                .child(text(what).font(k.ty.ui).color(k.c.text_faint)),
        )
        .child(text(format!("A {} comment.", a.id)).font(k.ty.body).color(k.c.text_base).wrap());
    col().w_full().px(12.0).py(6.0).child(body)
}

fn on_key(s: &mut Review, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if let rune::Key::Text(t) = &ev.key
        && t == "s"
        && tx.focused().is_none()
    {
        let next = if s.diff.mode() == DiffMode::Split {
            DiffMode::Unified
        } else {
            DiffMode::Split
        };
        s.diff.set_mode(next);
        return true;
    }
    s.diff.key(ev, tx)
}

fn main() {
    let spec = AppSpec {
        scenes: &[
            ("s", |_| {}),
            ("m", |s: &mut Review| s.diff = DiffState::new(patch::parse(M))),
            ("split", |s: &mut Review| s.diff.set_mode(DiffMode::Split)),
            ("wrap", |s: &mut Review| s.diff.set_wrap(true)),
            ("annotated", annotate),
            ("annotated-split", |s: &mut Review| {
                annotate(s);
                s.diff.set_mode(DiffMode::Split);
            }),
            ("cursor", |s: &mut Review| {
                if let Some((f, n)) = hunk_with(&s.diff, 4) {
                    s.diff.set_cursor(Some((f, LineRef::new(Side::New, n + 2))));
                }
            }),
            ("onesided", |s: &mut Review| {
                s.diff = DiffState::new(patch::parse(ONE_SIDED));
                s.diff.set_mode(DiffMode::Split);
            }),
            ("narrow", |s: &mut Review| {
                s.diff.set_mode(DiffMode::Split);
                s.diff.set_min_split_columns(90);
            }),
            ("spot", |s: &mut Review| {
                if let Some((f, n)) = hunk_with(&s.diff, 6) {
                    s.diff.set_spotlight(Some(Spot {
                        file: f,
                        side: Side::New,
                        lines: (n + 1, n + 4),
                        caption: Some("The endpoint validates the amount".into()),
                        step: Some((3, 9)),
                    }));
                }
            }),
            ("dark", |s: &mut Review| s.dark = Some(true)),
            ("light", |s: &mut Review| s.dark = Some(false)),
        ],
        on_key: Some(on_key),
        families: ("Inter", "ui-monospace, Inter"),
        ..AppSpec::new("Rain diffview", (1280.0, 800.0), init, view)
    };
    App::new(spec).modifier_keys().run();
}
