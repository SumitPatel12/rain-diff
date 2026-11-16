//! The pop-out peek: one hidden secondary window that "Pop out" fills with a peek's lines and
//! opens. Its state is owned data, so it crosses into the window from the main one.

use std::sync::OnceLock;

use rune::{AppHandle, AppSpec, Cx, El, Key, KeyEvent, Tx, WindowHandle, WindowOptions, WindowSpec, col, row, text};

use crate::app::PeekContent;
use crate::settings::Theme;
use crate::theme::Scheme;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeekWindow {
    pub content: Option<PeekContent>,
    /// The main window's scheme when it popped out; `None` follows the system.
    pub scheme: Option<Scheme>,
}

impl PeekWindow {
    /// What a pop-out puts in the window: a second pop-out replaces the first.
    pub fn show(&mut self, content: PeekContent, scheme: Scheme) {
        self.content = Some(content);
        self.scheme = Some(scheme);
    }
}

static HANDLES: OnceLock<(AppHandle<PeekWindow>, WindowHandle)> = OnceLock::new();

/// Sends `content` to the peek window and brings it to the front; false where there is no such
/// window (tests, headless renders).
pub(crate) fn show(content: PeekContent, scheme: Scheme) -> bool {
    let Some((app, window)) = HANDLES.get() else { return false };
    app.update(move |w: &mut PeekWindow, _| w.show(content, scheme));
    window.open();
    window.focus();
    true
}

/// Registers the hidden peek window on `app`.
pub(crate) fn register<S: 'static>(app: &mut rune::App<S>) {
    let (fonts, families) = crate::fonts();
    let spec = AppSpec {
        scenes: &[("sample", sample)],
        fonts,
        families,
        on_key: Some(on_key),
        min_size: Some((420.0, 200.0)),
        ..AppSpec::new("Rain · Peek", (760.0, 480.0), PeekWindow::default, view)
    };
    let spec = WindowSpec::new("peek", spec).options(WindowOptions {
        visible: false,
        ..WindowOptions::default()
    });
    let handles = app.window(spec);
    let _ = HANDLES.set(handles);
}

fn on_key(_: &mut PeekWindow, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.key == Key::Escape {
        tx.close_window();
        return true;
    }
    false
}

pub fn view(w: &PeekWindow, cx: &mut Cx<PeekWindow>) -> El {
    let scheme = w.scheme.unwrap_or_else(|| Theme::System.scheme(cx.system_dark()));
    let look = crate::theme::look(scheme);
    let k = &look.kit;
    let mut root = col().key("peek-window").w_full().h_full().fill(look.s.reader);
    let Some(c) = &w.content else {
        return root.child(
            text("Pop out a peek from a cite to see its lines here")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .padding(24.0),
        );
    };
    let (dir, name) = c.key.rsplit_once('/').map_or(("", c.key.as_str()), |(d, n)| (d, n));
    let dir = if dir.is_empty() {
        String::new()
    } else {
        format!("{}/", dir.replacen('/', " · ", 1))
    };
    root = root
        .child(
            row()
                .key("peek-window-head")
                .w_full()
                .h(32.0)
                .px(12.0)
                .items_center()
                .label(c.title.clone())
                .child(
                    text(dir)
                        .font(crate::theme::TYPE.small)
                        .color(k.c.text_muted)
                        .truncate_head()
                        .shrink(),
                )
                .child(text(name.to_owned()).font(crate::theme::TYPE.label).color(k.c.text_base)),
        )
        .child(rune::el().w_full().h(1.0).fill(look.s.hairline));
    root.child(
        col()
            .key("peek-window-body")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll_xy()
            .child(crate::view::peek_code(c, &look).min_w(640.0)),
    )
    .child(rune::el().w_full().h(1.0).fill(look.s.hairline))
    .child(
        row()
            .key("peek-window-foot")
            .w_full()
            .h(42.0)
            .px(14.0)
            .items_center()
            .fill(look.s.card_head)
            .child(rune::el().grow(1.0).a11y_hidden())
            .child(text("Esc closes").font(crate::theme::TYPE.small).color(k.c.text_faint)),
    )
}

/// Headless scene: the window as a pop-out of the Overview scene's first How cite leaves it, or
/// of the first change when the diff isn't the story the brief cites.
fn sample(w: &mut PeekWindow) {
    let mut a = crate::init();
    crate::scene_base(&mut a);
    a.scene_brief(false);
    let Some(t) = a.sample_peek_target().or_else(|| a.first_change_peek_target()) else {
        return;
    };
    w.content = Some(a.peek_content(&t));
    w.scheme = crate::theme_flag().map(|t| t.scheme(true));
}
