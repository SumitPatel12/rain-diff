//! Syntax highlighting for the diff view behind a [`rune_kit::syntax::Highlighter`], coloured by
//! a [`Palette`] (GitHub-like, Everforest or Gruvbox): rune-kit's linear scanners (plus a C# extension) where they fit,
//! syntect grammars (two-face's extended set, Oniguruma regexes) for the rest; see [`engine`].
//!
//! syntect scopes are classified into ten [`Token`]s by one syntect theme whose foreground colours are
//! token indices, so every palette shares the theme and a palette is a lookup table.

use std::ops::Range;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rune::Color;
use rune_kit::syntax::{Highlight, Highlighter, Paint, Syntax};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SynColor, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

pub mod fast;
#[doc(hidden)]
pub mod samples;

/// The bytes of a source the highlighter colours, as in rune-kit (cut at a line end).
pub const MAX_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Palette {
    GithubDark,
    GithubLight,
    Everforest,
    Gruvbox,
}

/// A kind of token, each a colour of a [`Palette`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    Comment,
    String,
    Number,
    Keyword,
    Type,
    Function,
    Variable,
    Punctuation,
    Tag,
    Attribute,
}

impl Token {
    pub const ALL: [Token; 10] = [
        Token::Comment,
        Token::String,
        Token::Number,
        Token::Keyword,
        Token::Type,
        Token::Function,
        Token::Variable,
        Token::Punctuation,
        Token::Tag,
        Token::Attribute,
    ];
}

/// sRGB hex per token, in [`Token::ALL`] order, one row per [`Palette`]. GitHub dark and light
/// follow Primer's "prettylights" syntax colours, with distinct picks where Primer reuses one.
/// Everforest follows Rain's redesign roles (Everforest hexes, with a warm grey for
/// punctuation); Gruvbox takes only its palette's own hexes, as its Vim theme assigns them.
const HEX: [[&str; 10]; 4] = [
    [
        "#8b949e", "#a5d6ff", "#79c0ff", "#ff7b72", "#ffa657", "#d2a8ff", "#e3b341", "#9198a1", "#7ee787", "#56d4dd",
    ],
    [
        "#6e7781", "#0a3069", "#0550ae", "#cf222e", "#953800", "#8250df", "#7d4e00", "#57606a", "#116329", "#1b7c83",
    ],
    // grey1, yellow, purple, orange, blue, aqua, fg, punct, orange, yellow
    [
        "#859289", "#dbbc7f", "#d699b6", "#e69875", "#7fbbb3", "#83c092", "#d3c6aa", "#a59c86", "#e69875", "#dbbc7f",
    ],
    // gray, green, purple, red, yellow, green, blue, fg4, blue, aqua
    [
        "#928374", "#b8bb26", "#d3869b", "#fb4934", "#fabd2f", "#b8bb26", "#83a598", "#a89984", "#83a598", "#8ec07c",
    ],
];

impl Palette {
    pub const ALL: [Palette; 4] = [Palette::GithubDark, Palette::GithubLight, Palette::Everforest, Palette::Gruvbox];

    pub fn for_dark(dark: bool) -> Palette {
        if dark { Palette::GithubDark } else { Palette::GithubLight }
    }

    pub fn is_dark(self) -> bool {
        self != Palette::GithubLight
    }

    pub fn color(self, t: Token) -> Color {
        static COLORS: OnceLock<[[Color; 10]; 4]> = OnceLock::new();
        let colors = COLORS.get_or_init(|| HEX.map(|row| row.map(|s| rune::color::hex(s).expect("palette hex"))));
        colors[self as usize][t as usize]
    }

    /// `base` with the token kinds rune-kit's scanners paint ([`Syntax`]) in this palette, for
    /// Markdown code blocks and anything else that colours by kind.
    pub fn syntax_theme(self, base: rune_kit::syntax::SyntaxTheme) -> rune_kit::syntax::SyntaxTheme {
        [
            Syntax::Keyword,
            Syntax::String,
            Syntax::Number,
            Syntax::Comment,
            Syntax::Type,
            Syntax::Function,
            Syntax::Property,
            Syntax::Punctuation,
        ]
        .into_iter()
        .fold(base, |t, k| match kind_token(k) {
            Some(tok) => t.with(k, self.color(tok)),
            None => t,
        })
    }
}

/// Scope selectors per token. syntect picks the most specific match, so the narrow entries
/// override the broad ones regardless of order.
const SCOPES: &[(&str, Token)] = &[
    ("comment, punctuation.definition.comment", Token::Comment),
    (
        "string, punctuation.definition.string, constant.character.escape, markup.raw.inline, markup.inline.raw, markup.underline.link",
        Token::String,
    ),
    (
        "constant.numeric, constant.language, constant.character, constant.other, keyword.other.unit, storage.type.numeric",
        Token::Number,
    ),
    (
        "keyword, storage.modifier, storage.type.class, storage.type.struct, storage.type.interface, storage.type.enum, \
         storage.type.namespace, storage.type.function, storage.type.record, storage.type.delegate, storage.type.event, \
         storage.type.variable, storage.type.let, keyword.operator.word, keyword.operator.new, keyword.operator.expression, \
         keyword.operator.logical.sql, keyword.operator.assignment.alias.sql, keyword.operator.comparison.powershell, \
         keyword.operator.logical.powershell, storage.type.js, storage.type.ts, storage.type.tsx, markup.heading, entity.name.section",
        Token::Keyword,
    ),
    (
        "storage.type, entity.name.type, entity.name.class, entity.name.struct, entity.name.interface, entity.name.enum, \
         entity.other.inherited-class, support.type, support.class, entity.name.namespace, entity.name.label",
        Token::Type,
    ),
    (
        "entity.name.function, support.function, variable.function, meta.function-call.identifier, entity.name.method",
        Token::Function,
    ),
    (
        "variable.parameter, variable.other.member, variable.other.property, variable.other.object.property, \
         support.type.property-name, meta.property-name, variable.language, support.variable, variable.other.readwrite.powershell, \
         punctuation.definition.variable, variable.other.constant, meta.mapping.key string, markup.bold, markup.italic",
        Token::Variable,
    ),
    (
        "punctuation, keyword.operator, meta.brace, markup.list punctuation.definition",
        Token::Punctuation,
    ),
    ("entity.name.tag, punctuation.definition.tag, meta.tag punctuation", Token::Tag),
    (
        "entity.other.attribute-name, meta.annotation, meta.attribute, entity.other.pseudo-class, entity.other.pseudo-element",
        Token::Attribute,
    ),
];

/// Scopes left in the text colour although a broader selector above would colour them.
const PLAIN: &str = "constant.other.table-name.sql, constant.other.database-name.sql";

/// syntect's extended syntax set, built once.
pub fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// Loads the syntax set and theme and runs the common grammars once, ahead of first use;
/// returns how long that took. syntect compiles a grammar's regexes on first match, which
/// costs tens of ms per language, so call this off the UI thread at startup.
pub fn warm() -> Duration {
    static DONE: OnceLock<()> = OnceLock::new();
    let t = Instant::now();
    DONE.get_or_init(|| {
        theme();
        for (lang, code) in samples::ALL {
            if let Engine::Syntect(s) = engine(Mode::Rich, Some(lang)) {
                tokens(s, code);
            }
        }
    });
    t.elapsed()
}

const UNCOLOURED: SynColor = SynColor { r: 0, g: 0, b: 0, a: 0 };

/// Foreground colours are token indices (alpha 255); [`UNCOLOURED`] marks unclassified text.
fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let item = |sel: &str, fg: SynColor| ThemeItem {
            scope: ScopeSelectors::from_str(sel).expect("scope selector"),
            style: StyleModifier {
                foreground: Some(fg),
                background: None,
                font_style: None,
            },
        };
        let scopes = SCOPES
            .iter()
            .map(|&(sel, tok)| {
                item(
                    sel,
                    SynColor {
                        r: tok as u8,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                )
            })
            .chain([item(PLAIN, UNCOLOURED)])
            .collect();
        Theme {
            name: Some("rain-tokens".into()),
            author: None,
            settings: ThemeSettings {
                foreground: Some(UNCOLOURED),
                ..ThemeSettings::default()
            },
            scopes,
        }
    })
}

/// The language key for a rune-kit language string (an extension such as `cs` or `tsx`, a fence
/// word, a file name such as `Dockerfile`, or a path), with the web and .NET aliases applied;
/// `None` for empty and plain text.
fn language_key(lang: &str) -> Option<String> {
    let name = lang.trim().rsplit(['/', '\\']).next().unwrap_or("");
    let key = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => name.trim_start_matches('.'),
    };
    let lower = key.to_ascii_lowercase();
    let key = match lower.as_str() {
        "" | "txt" | "text" | "plain" | "plaintext" => return None,
        "cshtml" | "razor" | "vbhtml" => "html",
        "csx" | "csharp" | "c#" => "cs",
        "psm1" | "psd1" | "powershell" | "pwsh" => "ps1",
        "csproj" | "props" | "targets" | "resx" | "config" | "xaml" | "nuspec" => "xml",
        _ => key,
    };
    Some(key.to_owned())
}

/// The syntect syntax for a rune-kit language string (see [`language_key`]).
pub fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let key = language_key(lang)?;
    let set = syntaxes();
    set.find_syntax_by_extension(&key)
        .or_else(|| set.find_syntax_by_token(&key))
        .filter(|s| s.name != "Plain Text")
}

/// How much a highlighter may spend per chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Linear scanners wherever one fits (rune-kit's, and [`fast`] for C# and SCSS), syntect for the
    /// rest. The diff view's default: 128 lines of C# cost ~0.1 ms instead of ~6 ms.
    Fast,
    /// syntect wherever it knows the language: richer scopes, ~50-180 µs a line.
    Rich,
}

/// Which engine colours a language.
#[derive(Clone, Copy, Debug)]
pub enum Engine {
    /// rune-kit's linear scanners for a language they know.
    Builtin,
    /// rune-kit's TypeScript scanner extended for C# ([`fast::csharp`]).
    CSharp,
    /// rune-kit's CSS scanner extended for SCSS ([`fast::scss`]).
    Scss,
    Syntect(&'static SyntaxReference),
    None,
}

/// The engine `mode` uses for `lang`. Fast: rune-kit for JS/TS, JSON, HTML, XML, CSS, YAML,
/// SQL, shell and the rest it knows, [`fast`] for C# and SCSS, syntect for PowerShell,
/// Markdown, Dockerfile and others. Rich: syntect first.
pub fn engine(mode: Mode, lang: Option<&str>) -> Engine {
    let Some(key) = lang.and_then(language_key) else {
        return Engine::None;
    };
    let syntect = || find_syntax(&key).map(Engine::Syntect);
    let fast = || match key.to_ascii_lowercase().as_str() {
        "cs" => Some(Engine::CSharp),
        "scss" => Some(Engine::Scss),
        _ => rune_kit::syntax::known(&key).then_some(Engine::Builtin),
    };
    let found = match mode {
        Mode::Fast => fast().or_else(syntect),
        Mode::Rich => syntect().or_else(fast),
    };
    found.unwrap_or(Engine::None)
}

fn kind_token(k: Syntax) -> Option<Token> {
    Some(match k {
        Syntax::Keyword => Token::Keyword,
        Syntax::String => Token::String,
        Syntax::Number => Token::Number,
        Syntax::Comment => Token::Comment,
        Syntax::Type => Token::Type,
        Syntax::Function => Token::Function,
        Syntax::Property => Token::Variable,
        Syntax::Punctuation => Token::Punctuation,
        _ => return None,
    })
}

/// The first [`MAX_BYTES`] of `code`, cut after a line end.
fn capped(code: &str) -> &str {
    if code.len() <= MAX_BYTES {
        return code;
    }
    match code.as_bytes()[..MAX_BYTES].iter().rposition(|&b| b == b'\n') {
        Some(i) => &code[..=i],
        None => "",
    }
}

/// `code`'s highlights in `palette` with [`Mode::Fast`].
pub fn highlight(palette: Palette, lang: Option<&str>, code: &str) -> Vec<Highlight> {
    highlight_with(Mode::Fast, palette, lang, code)
}

/// `code`'s highlights: sorted, non-overlapping byte ranges painted from `palette`. Thread-safe,
/// so a worker can highlight ahead of the view. See [`engine`] for which engine runs.
pub fn highlight_with(mode: Mode, palette: Palette, lang: Option<&str>, code: &str) -> Vec<Highlight> {
    let code = capped(code);
    let mut out = match engine(mode, lang) {
        Engine::Syntect(s) => {
            return tokens(s, code)
                .into_iter()
                .map(|(r, t)| (r, Paint::Color(palette.color(t))))
                .collect();
        }
        Engine::Builtin => rune_kit::syntax::builtin(lang.and_then(language_key).as_deref(), code),
        Engine::CSharp => fast::csharp(code),
        Engine::Scss => fast::scss(code),
        Engine::None => return rune_kit::syntax::builtin(lang, code),
    };
    for (_, p) in &mut out {
        if let Paint::Kind(k) = *p
            && let Some(t) = kind_token(k)
        {
            *p = Paint::Color(palette.color(t));
        }
    }
    out
}

/// `code`'s token ranges by `syntax`, merged across whitespace on one line.
pub fn tokens(syntax: &SyntaxReference, code: &str) -> Vec<(Range<usize>, Token)> {
    let set = syntaxes();
    let mut h = HighlightLines::new(syntax, theme());
    let mut out: Vec<(Range<usize>, Token)> = Vec::new();
    let mut off = 0;
    for line in LinesWithEndings::from(code) {
        let Ok(regions) = h.highlight_line(line, set) else { break };
        for (style, piece) in regions {
            let start = off;
            off += piece.len();
            let fg = style.foreground;
            let body = piece.trim_end_matches(['\r', '\n']);
            if fg.a == 0 || body.trim().is_empty() {
                continue;
            }
            let Some(&tok) = Token::ALL.get(usize::from(fg.r)) else { continue };
            let end = start + body.len();
            if let Some((prev, t)) = out.last_mut()
                && *t == tok
                && code[prev.end..start].bytes().all(|b| b == b' ' || b == b'\t')
            {
                prev.end = end;
                continue;
            }
            out.push((start..end, tok));
        }
    }
    out
}

/// A rune-kit highlighter in `palette` with [`Mode::Fast`].
pub fn highlighter(palette: Palette) -> Highlighter {
    highlighter_with(Mode::Fast, palette)
}

/// A rune-kit highlighter, keyed so palettes and modes keep separate cache entries.
pub fn highlighter_with(mode: Mode, palette: Palette) -> Highlighter {
    Highlighter::new(move |lang, code| highlight_with(mode, palette, lang, code)).key((mode, palette))
}
