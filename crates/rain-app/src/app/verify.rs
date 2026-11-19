//! Verify by hand as a checklist: the Brief's Verify items with the reviewer's edits, ticked in
//! the session so the ticks outlive the window, on a card the reviewer folds and drags about.

use std::cell::Cell;

use rain_core::verify::{ItemRef, VerifyItem, VerifyRefusal, brief_items};
use rune::{Drag, Live, Phase, TextEdit, Tx};

use super::App;

/// Space kept between the card and the window's edges.
const EDGE: f32 = 4.0;
/// Where the card sits by default: this far in from the reader's bottom-left corner.
const INSET: f32 = 14.0;
/// Pointer travel that makes a press on the head a drag instead of a click.
const SLOP: f32 = 4.0;

/// The floating card's geometry and its fields. Its position is a live value: a drag moves it
/// with no view run.
pub struct VerifyCard {
    /// The card's top-left in window points, as drawn.
    pub pos: Live<[f32; 2]>,
    size: Cell<[f32; 2]>,
    window: Cell<[f32; 2]>,
    reader: Cell<[f32; 4]>,
    /// Where a drag on the head started, and where it has got to.
    drag: Option<([f32; 2], [f32; 2])>,
    /// The press that ends a drag doesn't fold the card; it comes with the release, so the mark
    /// lapses quickly and a drag released off the head can't swallow a later click or Enter.
    swallow_press: Option<std::time::Instant>,
    pub editing: Option<ItemRef>,
    pub edit: TextEdit,
    pub add: TextEdit,
}

impl Default for VerifyCard {
    fn default() -> VerifyCard {
        VerifyCard {
            pos: Live::new([0.0; 2]),
            size: Cell::new([0.0; 2]),
            window: Cell::new([0.0; 2]),
            reader: Cell::new([0.0; 4]),
            drag: None,
            swallow_press: None,
            editing: None,
            edit: TextEdit::default(),
            add: TextEdit::default(),
        }
    }
}

impl VerifyCard {
    /// `at` (or the default spot) kept fully inside the window.
    fn clamped(&self, at: Option<[f32; 2]>) -> [f32; 2] {
        let ([w, h], [ww, wh], r) = (self.size.get(), self.window.get(), self.reader.get());
        let [x, y] = at.unwrap_or([r[0] + INSET, r[1] + r[3] - INSET - h]);
        [x.clamp(EDGE, (ww - w - EDGE).max(EDGE)), y.clamp(EDGE, (wh - h - EDGE).max(EDGE))]
    }

    /// Puts the card where the session says, or where a drag holds it.
    pub fn place(&self, at: Option<[f32; 2]>) {
        let p = match self.drag {
            Some((_, now)) => now,
            None => self.clamped(at),
        };
        self.pos.snap(p);
    }

    pub(crate) fn set_window(&self, size: (f32, f32)) {
        self.window.set([size.0, size.1]);
    }

    pub(crate) fn set_reader(&self, r: [f32; 4]) {
        self.reader.set(r);
    }

    pub fn typing(&self) -> bool {
        self.edit.focused || self.add.focused
    }
}

impl App {
    /// The checklist: the Brief's Verify items with the reviewer's edits, then their own.
    pub fn verify_items(&self) -> Vec<VerifyItem> {
        self.mirror.verify_items()
    }

    /// The card shows while the Brief has a Verify section or the reviewer has items.
    pub fn verify_shown(&self) -> bool {
        !brief_items(self.mirror.brief.as_ref()).is_empty() || !self.verify_items().is_empty()
    }

    pub fn verify_done(&self, item: &str) -> bool {
        self.mirror.verified.iter().any(|v| v == item)
    }

    pub fn verify_open(&self) -> bool {
        self.mirror.verify.open
    }

    /// `Shift+H` and the card's head: folds the card to its head, or opens it; per session.
    pub fn toggle_verify_card(&mut self) {
        if !self.verify_shown() {
            self.status = "Nothing to verify by hand: the brief has no Verify items".into();
            return;
        }
        self.mutate(|s| {
            s.verify.open = !s.verify.open;
            s.version += 1;
        });
    }

    /// A click or Enter on the head; the release that ends a drag is not one.
    pub(crate) fn press_verify_head(&mut self) {
        if self
            .verify_card
            .swallow_press
            .take()
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(300))
        {
            return;
        }
        self.toggle_verify_card();
    }

    /// Ticks or unticks a Verify item, in the session.
    pub fn toggle_verified(&mut self, item: &str) {
        let item = item.to_owned();
        self.mutate(|s| {
            match s.verified.iter().position(|v| *v == item) {
                Some(i) => {
                    s.verified.remove(i);
                }
                None => s.verified.push(item),
            }
            s.version += 1;
        });
    }

    /// Dragging the head moves the card through its live position; only the drop is saved.
    pub(crate) fn verify_drag(&mut self, d: &mut Drag, tx: &mut Tx) {
        let c = &mut self.verify_card;
        match d.phase {
            Phase::Down => {
                let at = c.pos.get();
                c.drag = Some((at, at));
                c.swallow_press = None;
                tx.live_only();
            }
            Phase::Move => {
                if let Some((from, _)) = c.drag {
                    let now = c.clamped(Some([from[0] + d.dx, from[1] + d.dy]));
                    c.drag = Some((from, now));
                    c.pos.snap(now);
                }
                tx.live_only();
            }
            Phase::Up => {
                let Some((from, now)) = c.drag.take() else { return };
                let moved = d.dx.abs() > SLOP || d.dy.abs() > SLOP;
                if d.cancelled() || !moved {
                    c.pos.snap(from);
                    c.swallow_press = moved.then(std::time::Instant::now);
                    if !moved {
                        tx.live_only();
                    }
                    return;
                }
                c.swallow_press = Some(std::time::Instant::now());
                self.mutate(|s| {
                    s.verify.at = Some(now);
                    s.version += 1;
                });
            }
        }
    }

    /// Double-click on the head, or Reset position: back to the reader's bottom left.
    pub fn reset_verify_position(&mut self) {
        if self.mirror.verify.at.is_none() {
            return;
        }
        self.mutate(|s| {
            s.verify.at = None;
            s.version += 1;
        });
        self.verify_card.place(None);
        self.status = "Verify by hand is back at the reader's bottom left".into();
    }

    /// The card's laid-out size: it moves to stay on screen with no view run.
    pub(crate) fn set_verify_size(&mut self, r: [f32; 4], tx: &mut Tx) {
        self.verify_card.size.set([r[2], r[3]]);
        self.verify_card.place(self.mirror.verify.at);
        tx.live_only();
    }

    pub fn start_verify_edit(&mut self, of: ItemRef, text: &str) {
        self.verify_card.editing = Some(of);
        self.verify_card.edit = TextEdit::default();
        self.verify_card.edit.insert(text);
        self.verify_card.edit.focused = true;
        self.status = "Edit the check · Enter saves · Esc cancels".into();
    }

    pub fn cancel_verify_edit(&mut self) {
        self.verify_card.editing = None;
        self.verify_card.edit = TextEdit::default();
        self.status.clear();
    }

    pub fn save_verify_edit(&mut self) {
        let Some(of) = self.verify_card.editing.clone() else { return };
        let text = self.verify_card.edit.text.clone();
        match self.mutate(|s| s.edit_verify(&of, &text)) {
            Ok(()) => self.cancel_verify_edit(),
            Err(why) => self.status = refusal(why).into(),
        }
    }

    pub fn remove_verify_item(&mut self, of: &ItemRef) {
        if self.verify_card.editing.as_ref() == Some(of) {
            self.cancel_verify_edit();
        }
        if self.mutate(|s| s.remove_verify(of)).is_ok() {
            self.status = "Check removed".into();
        }
    }

    /// Enter in the add field: appends the check and keeps the field for the next one.
    pub fn add_verify_item(&mut self) {
        let text = self.verify_card.add.text.clone();
        match self.mutate(|s| s.add_verify(&text)) {
            Ok(()) => {
                self.verify_card.add = TextEdit::default();
                self.verify_card.add.focused = true;
                self.status = "Check added".into();
            }
            Err(why) => self.status = refusal(why).into(),
        }
    }

    pub fn leave_verify_add(&mut self) {
        self.verify_card.add = TextEdit::default();
    }
}

fn refusal(why: VerifyRefusal) -> &'static str {
    match why {
        VerifyRefusal::Empty => "A check needs some text",
        VerifyRefusal::Duplicate => "That check is already on the list",
        VerifyRefusal::Missing => "That check is gone",
    }
}
