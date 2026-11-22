//! The redesign's single-colour icons (16 × 16, 1.5 strokes), drawn in the text colour they're
//! given.

#![allow(dead_code)]

use rune::{El, Icon, icon};

macro_rules! stroked {
    ($name:ident, $id:literal, $body:literal) => {
        pub static $name: Icon = Icon {
            name: concat!("rain-", $id),
            view: 16.0,
            body: $body,
        };
    };
}

stroked!(
    SIDEBAR,
    "sidebar",
    r#"<rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="2" y="3" width="12" height="10" rx="2"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M6 3v10"/>"#
);
stroked!(
    SEARCH,
    "search",
    r#"<circle fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" cx="7" cy="7" r="4.5"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M10.5 10.5 14 14"/>"#
);
stroked!(
    FUNNEL,
    "funnel",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M2.5 3h11l-4.2 5v4.5l-2.6-1.3V8z"/>"#
);
stroked!(
    CHEV_D,
    "chev-d",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="m4.5 6 3.5 3.5L11.5 6"/>"#
);
stroked!(
    CHEV_U,
    "chev-u",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="m4.5 10 3.5-3.5 3.5 3.5"/>"#
);
stroked!(
    CHEV_R,
    "chev-r",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="m6 4.5 3.5 3.5L6 11.5"/>"#
);
stroked!(
    CHEV_L,
    "chev-l",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M10 4.5 6.5 8l3.5 3.5"/>"#
);
stroked!(
    FOLD_ALL,
    "fold-all",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="m4.5 2.5 3.5 3 3.5-3M4.5 13.5l3.5-3 3.5 3"/>"#
);
stroked!(
    UNFOLD_ALL,
    "unfold-all",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="m4.5 5.5 3.5-3 3.5 3M4.5 10.5l3.5 3 3.5-3"/>"#
);
stroked!(
    MAP,
    "map",
    r#"<rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="1.5" y="2.5" width="5" height="4" rx="1"/><rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="9.5" y="9.5" width="5" height="4" rx="1"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M4 6.5v3.5a1.5 1.5 0 0 0 1.5 1.5h4"/>"#
);
stroked!(
    TOUR,
    "tour",
    r#"<circle fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" cx="3.5" cy="12.5" r="1.5"/><circle fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" cx="12.5" cy="3.5" r="1.5"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M5 12.5h4.5a2.5 2.5 0 0 0 0-5h-3a2.5 2.5 0 0 1 0-5H11"/>"#
);
stroked!(
    CHAT,
    "chat",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M2.5 3.5h11v7h-6l-3 2.5v-2.5h-2z"/>"#
);
pub static MORE: Icon = Icon {
    name: "rain-more",
    view: 16.0,
    body: r#"<g fill="currentColor"><circle cx="3.5" cy="8" r="1.1"/><circle cx="8" cy="8" r="1.1"/><circle cx="12.5" cy="8" r="1.1"/></g>"#,
};
stroked!(
    CHECK,
    "check",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="m3.5 8.5 3 3 6-7"/>"#
);
stroked!(
    TREE,
    "tree",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M3 2.5v9h3M3 6h3"/><rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="7" y="4.5" width="6" height="3" rx="1"/><rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="7" y="10" width="6" height="3" rx="1"/>"#
);
stroked!(
    SPARK,
    "spark",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M8 2v3M8 11v3M2 8h3M11 8h3M4 4l2 2M10 10l2 2M12 4l-2 2M6 10l-2 2"/>"#
);
stroked!(
    FILE,
    "file",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M3.5 2h5.5l3.5 3.5V14h-9z"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M9 2v3.5h3.5"/>"#
);
stroked!(
    COPY,
    "copy",
    r#"<rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M10.5 5.5V3.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2"/>"#
);
stroked!(
    POPOUT,
    "popout",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M9 2.5h4.5V7M13.5 2.5 8 8"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M11.5 9.5v3a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1h3"/>"#
);
stroked!(
    FOLD,
    "fold",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M2.5 3v10M13.5 8H6M9 4.5 5.5 8 9 11.5"/>"#
);
stroked!(
    CLOSE,
    "close",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="m4 4 8 8M12 4l-8 8"/>"#
);
stroked!(
    PLUS,
    "plus",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M8 3v10M3 8h10"/>"#
);
stroked!(
    RELOAD,
    "reload",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M13 8a5 5 0 1 1-1.5-3.6M13 2.5v3h-3"/>"#
);
stroked!(
    WARN,
    "warn",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M8 2.5 14 13H2z"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M8 6.5v3M8 11.3v.2"/>"#
);
stroked!(
    INFO,
    "info",
    r#"<circle fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" cx="8" cy="8" r="6"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M8 7v4M8 5v.2"/>"#
);
stroked!(
    KBD,
    "kbd",
    r#"<rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="1.5" y="4" width="13" height="8" rx="1.5"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M4 6.5h1M7.5 6.5h1M11 6.5h1M4.5 9.5h7"/>"#
);
stroked!(
    ZOOM,
    "zoom",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10"/>"#
);
stroked!(
    EDIT,
    "edit",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M3 13l1-3.5L10.5 3 13 5.5 6.5 12z"/>"#
);
stroked!(
    EXPAND,
    "expand",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M9.5 2.5h4v4M13.5 2.5 9 7M6.5 13.5h-4v-4M2.5 13.5 7 9"/>"#
);
stroked!(
    CONTRAST,
    "contrast",
    r#"<circle cx="8" cy="8" r="5.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/><path d="M8 2.5v11a5.5 5.5 0 0 0 0-11z" fill="currentColor"/>"#
);
stroked!(
    PIN,
    "pin",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M6 2.5h4l-.5 4 2 2v1h-7v-1l2-2zM8 9.5v4"/>"#
);
stroked!(
    ARROW_R,
    "arrow-r",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M3 8h10M9 4l4 4-4 4"/>"#
);
pub static PLAY: Icon = Icon {
    name: "rain-play",
    view: 16.0,
    body: r#"<path d="M5 3.5v9l7-4.5z" fill="currentColor"/>"#,
};
pub static PAUSE: Icon = Icon {
    name: "rain-pause",
    view: 16.0,
    body: r#"<path d="M4.5 3.5h2.5v9H4.5zM9 3.5h2.5v9H9z" fill="currentColor"/>"#,
};
stroked!(
    DIAGRAM,
    "diagram",
    r#"<rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="2" y="2.5" width="4.5" height="4.5" rx="1"/><rect fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" x="9.5" y="9" width="4.5" height="4.5" rx="1"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M4.25 7v4h5.25M11.75 9V4.75H6.5"/>"#
);
stroked!(
    BOOK,
    "book",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M2.5 3.5h4a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 0-1.5-1.5h-4zM13.5 3.5h-4A1.5 1.5 0 0 0 8 5v8a1.5 1.5 0 0 1 1.5-1.5h4z"/>"#
);
stroked!(
    BRANCH,
    "branch",
    r#"<circle fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" cx="4.5" cy="3.5" r="1.5"/><circle fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" cx="4.5" cy="12.5" r="1.5"/><circle fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" cx="11.5" cy="5.5" r="1.5"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M4.5 5v6M11.5 7a4 4 0 0 1-4 4h-1.5"/>"#
);
stroked!(
    QUESTION,
    "question",
    r#"<circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/><path d="M6.3 6.3a1.8 1.8 0 1 1 2.4 1.7c-.5.2-.7.6-.7 1.1M8 11.3v.2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#
);
/// The Rain mark's strokes, drawn on its rounded square (see [`super::shell::mark`]).
pub static RAIN: Icon = Icon {
    name: "rain-mark",
    view: 22.0,
    body: r#"<path d="M8 5.5 6 10.5M12.5 5.5l-3 7.5M16 8.5l-2 5M10 14.5 9 17" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>"#,
};

pub fn draw(i: &'static Icon, size: f32, color: rune::Color) -> El {
    icon(i, size).color(color).a11y_hidden()
}
