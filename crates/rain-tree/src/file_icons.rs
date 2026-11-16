//! Two-tone file and folder icons, coloured by kind: a tinted page with a solid mark (code,
//! data, text, config, image, lock, test, database), and solid folders.

use rune::{Color, Icon};

macro_rules! page {
    ($name:ident, $id:literal, $mark:literal) => {
        pub static $name: Icon = Icon {
            name: concat!("rain-file-", $id),
            view: 16.0,
            body: concat!(
                r#"<path opacity="0.32" d="M3.5 1.5h5.8l3.2 3.2v9.3a.5.5 0 0 1-.5.5h-8a.5.5 0 0 1-.5-.5z"/><path opacity="0.6" d="M9.3 1.5v3.2h3.2z"/>"#,
                $mark
            ),
        };
    };
}

page!(
    CODE,
    "code",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M6.6 7.4 4.9 9.3l1.7 1.9M9.4 7.4l1.7 1.9-1.7 1.9"/>"#
);
page!(
    DATA,
    "data",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M6.8 7c-.9 0-1.1.4-1.1 1v.6c0 .5-.3.7-.7.7.4 0 .7.2.7.7v.6c0 .6.2 1 1.1 1M9.2 7c.9 0 1.1.4 1.1 1v.6c0 .5.3.7.7.7-.4 0-.7.2-.7.7v.6c0 .6-.2 1-1.1 1"/>"#
);
page!(
    TEXT,
    "text",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" d="M5.5 7.5h5M5.5 9.7h5M5.5 11.9h3"/>"#
);
page!(
    CONFIG,
    "config",
    r#"<circle fill="none" stroke="currentColor" stroke-width="1.5" cx="8" cy="9.6" r="1.6"/><path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" d="M8 6.6v.9M8 11.7v.9M5 9.6h.9M10.1 9.6h.9"/>"#
);
page!(
    IMAGE,
    "image",
    r#"<path d="m4.8 12.6 2.2-2.9 1.6 1.9 1.1-1.3 1.6 2.3z"/><circle cx="10" cy="7.6" r="1"/>"#
);
page!(
    LOCK,
    "lock",
    r#"<rect x="5.6" y="9" width="4.8" height="3.6" rx=".6"/><path fill="none" stroke="currentColor" stroke-width="1.4" d="M6.6 9V8a1.4 1.4 0 0 1 2.8 0v1"/>"#
);
page!(
    TEST,
    "test",
    r#"<path fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" d="m5.4 9.7 1.8 1.8 3.4-3.6"/>"#
);
page!(
    DB,
    "db",
    r#"<ellipse fill="none" stroke="currentColor" stroke-width="1.4" cx="8" cy="7.6" rx="2.6" ry="1"/><path fill="none" stroke="currentColor" stroke-width="1.4" d="M5.4 7.6v4c0 .6 1.2 1 2.6 1s2.6-.4 2.6-1v-4M5.4 9.6c0 .6 1.2 1 2.6 1s2.6-.4 2.6-1"/>"#
);

/// A closed folder, solid.
pub static FOLDER: Icon = Icon {
    name: "rain-folder",
    view: 16.0,
    body: r#"<path d="M1.5 4a1 1 0 0 1 1-1h3.7l1.5 1.6h5.8a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z"/>"#,
};

/// An open folder: the back tinted, the front solid.
pub static FOLDER_OPEN: Icon = Icon {
    name: "rain-folder-open",
    view: 16.0,
    body: r#"<path opacity="0.5" d="M1.5 4a1 1 0 0 1 1-1h3.7l1.5 1.6h4.8a1 1 0 0 1 1 1v1H1.5z"/><path d="M1.6 7.2h13.2a.5.5 0 0 1 .5.6l-1.1 4.5a1 1 0 0 1-1 .7H2.5a1 1 0 0 1-1-.8z"/>"#,
};

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::WHITE)
}

/// The folder colour (Material's folder slate).
pub fn folder_color() -> Color {
    hex("#90A4AE")
}

/// A file's icon and colour, by name: tests first, then the extension.
pub fn file_glyph(name: &str) -> (&'static Icon, Color) {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s);
    if stem.ends_with(".spec") || stem.ends_with(".test") || stem.ends_with("_test") || stem.ends_with("tests") || stem.ends_with("specs") {
        return (&TEST, hex("#8BC34A"));
    }
    if lower.ends_with(".lock") || lower == "package-lock.json" {
        return (&LOCK, hex("#9E9E9E"));
    }
    match ext {
        "rs" => (&CODE, hex("#E8865C")),
        "ts" | "tsx" | "mts" | "cts" => (&CODE, hex("#4F9BE8")),
        "js" | "jsx" | "mjs" | "cjs" => (&CODE, hex("#E8D44D")),
        "cs" | "csx" => (&CODE, hex("#A179DC")),
        "py" => (&CODE, hex("#5A9FD4")),
        "go" => (&CODE, hex("#29BEB0")),
        "java" | "kt" | "kts" | "scala" => (&CODE, hex("#E76F00")),
        "c" | "h" | "cc" | "cpp" | "hpp" | "m" | "mm" => (&CODE, hex("#659AD2")),
        "swift" => (&CODE, hex("#F05138")),
        "rb" => (&CODE, hex("#E0524C")),
        "php" => (&CODE, hex("#8892BF")),
        "sh" | "bash" | "zsh" | "ps1" | "psm1" | "bat" | "cmd" => (&CODE, hex("#89E051")),
        "html" | "htm" | "xml" | "xaml" | "svg" | "vue" | "svelte" => (&CODE, hex("#E8743B")),
        "css" | "scss" | "sass" | "less" => (&CODE, hex("#CF649A")),
        "json" | "jsonc" | "json5" => (&DATA, hex("#E8C547")),
        "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "env" | "props" | "csproj" | "sln" | "editorconfig" => (&CONFIG, hex("#9EA7AD")),
        "md" | "mdx" | "txt" | "rst" | "adoc" => (&TEXT, hex("#73A7E0")),
        "sql" => (&DB, hex("#E38C00")),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" => (&IMAGE, hex("#26A69A")),
        _ => (&TEXT, hex("#9EA7AD")),
    }
}
