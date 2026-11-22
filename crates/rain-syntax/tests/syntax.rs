use std::time::Instant;

use rain_syntax::{
    Carry, Engine, MAX_BYTES, Mode, Palette, Token, engine, find_syntax, highlight, highlight_from, highlight_with, highlighter,
    highlighter_with, samples, tokens, warm,
};
use rune_kit::syntax::{Highlight, Paint, builtin};

fn token_at(lang: &str, code: &str, needle: &str) -> Option<Token> {
    let at = code.find(needle).expect("needle in sample");
    let toks = tokens(find_syntax(lang).expect("syntax"), code);
    toks.iter().find(|(r, _)| r.start <= at && at < r.end).map(|(_, t)| *t)
}

fn assert_tidy(code: &str, hl: &[Highlight]) {
    let mut last = 0;
    for (r, _) in hl {
        assert!(r.start < r.end, "empty {r:?}");
        assert!(r.start >= last, "overlap or unsorted at {r:?}");
        assert!(r.end <= code.len(), "out of bounds {r:?}");
        assert!(
            code.is_char_boundary(r.start) && code.is_char_boundary(r.end),
            "inside a char {r:?}"
        );
        last = r.end;
    }
}

fn paint_at(mode: Mode, p: Palette, lang: &str, code: &str, needle: &str) -> Option<Paint> {
    let at = code.find(needle).expect("needle in sample");
    let hl = highlight_with(mode, p, Some(lang), code);
    hl.iter().find(|(r, _)| r.start <= at && at < r.end).map(|(_, p)| *p)
}

#[test]
fn csharp_lines_are_mostly_coloured() {
    for mode in [Mode::Fast, Mode::Rich] {
        csharp_coverage(mode);
    }
}

fn csharp_coverage(mode: Mode) {
    let code = samples::CSHARP;
    let hl = highlight_with(mode, Palette::GithubDark, Some("cs"), code);
    assert_tidy(code, &hl);
    let mut start = 0;
    let (mut lines, mut coloured) = (0, 0);
    for line in code.split_inclusive('\n') {
        let end = start + line.len();
        if !line.trim().is_empty() {
            lines += 1;
            if hl.iter().any(|(r, _)| r.start < end && r.end > start) {
                coloured += 1;
            } else {
                eprintln!("{mode:?} plain: {line:?}");
            }
        }
        start = end;
    }
    assert!(code.lines().count() >= 58 && lines >= 50, "sample has {lines} non-blank lines");
    assert!(
        coloured * 100 >= lines * 90,
        "{mode:?}: {coloured}/{lines} non-blank lines coloured"
    );
}

#[test]
fn csharp_tokens_get_their_colours() {
    let code = samples::CSHARP;
    assert_eq!(token_at("cs", code, "public sealed"), Some(Token::Keyword));
    assert_eq!(token_at("cs", code, "string label"), Some(Token::Type));
    assert_eq!(token_at("cs", code, "// Totals"), Some(Token::Comment));
    assert_eq!(token_at("cs", code, "/* Vouchers"), Some(Token::Comment));
    assert_eq!(token_at("cs", code, "\"voucher\""), Some(Token::String));
    assert_eq!(token_at("cs", code, "0m &&"), Some(Token::Number));
    assert_eq!(token_at("ts", samples::TYPESCRIPT, "const rows"), Some(Token::Keyword));
    assert_eq!(token_at("ts", samples::TYPESCRIPT, "string, page"), Some(Token::Type));
    assert_eq!(token_at("sql", samples::SQL, "dbo.Vouchers"), None);
    assert_eq!(token_at("sql", samples::SQL, "SELECT"), Some(Token::Keyword));
    assert_eq!(token_at("ps1", samples::POWERSHELL, "$repos ="), Some(Token::Variable));
    for mode in [Mode::Fast, Mode::Rich] {
        for p in Palette::ALL {
            let at = |needle| paint_at(mode, p, "cs", code, needle);
            let color = |t| Some(Paint::Color(p.color(t)));
            assert_eq!(at("public sealed"), color(Token::Keyword), "{mode:?}");
            assert_eq!(at("sealed class"), color(Token::Keyword), "{mode:?}");
            assert_eq!(at("using System"), color(Token::Keyword), "{mode:?}");
            assert_eq!(at("string label"), color(Token::Type), "{mode:?}");
            assert_eq!(at("decimal amount"), color(Token::Type), "{mode:?}");
            assert_eq!(at("// Totals"), color(Token::Comment), "{mode:?}");
            assert_eq!(at("/* Vouchers"), color(Token::Comment), "{mode:?}");
            assert_eq!(at("$\"Voucher for"), color(Token::String), "{mode:?}");
            assert_eq!(at("200)]"), color(Token::Number), "{mode:?}");
        }
    }
}

#[test]
fn web_and_dotnet_languages_resolve() {
    for (lang, name) in [
        ("cs", "C#"),
        ("csx", "C#"),
        ("ts", "TypeScript"),
        ("tsx", "TypeScriptReact"),
        ("js", "JavaScript (Babel)"),
        ("html", "HTML"),
        ("cshtml", "HTML"),
        ("razor", "HTML"),
        ("scss", "SCSS"),
        ("css", "CSS"),
        ("json", "JSON"),
        ("sql", "SQL"),
        ("ps1", "PowerShell"),
        ("psm1", "PowerShell"),
        ("yml", "YAML"),
        ("yaml", "YAML"),
        ("md", "Markdown"),
        ("xml", "XML"),
        ("csproj", "XML"),
        ("Dockerfile", "Dockerfile"),
        ("Makefile", "Makefile"),
        ("src/app/vouchers.component.ts", "TypeScript"),
    ] {
        let found = find_syntax(lang).map(|s| s.name.as_str());
        assert_eq!(found, Some(name), "{lang}");
    }
    for lang in ["txt", "", "zzz-unknown"] {
        assert!(find_syntax(lang).is_none(), "{lang}");
    }
}

#[test]
fn only_languages_without_a_fast_scanner_need_the_warm_up() {
    for lang in ["ts", "a/b.tsx", "cs", "scss", "sql", "rs", "json"] {
        assert!(rain_syntax::fast_covers(Some(lang)), "{lang}");
    }
    for lang in ["ps1", "docs/README.md", "Dockerfile"] {
        assert!(!rain_syntax::fast_covers(Some(lang)), "{lang}");
    }
    assert!(rain_syntax::fast_covers(None));
    for (lang, _) in samples::ALL {
        let syntect = matches!(engine(Mode::Fast, Some(lang)), Engine::Syntect(_));
        assert_eq!(rain_syntax::fast_covers(Some(lang)), !syntect, "{lang}");
    }
}

#[test]
fn fast_scanners_take_the_languages_they_know() {
    for lang in ["ts", "tsx", "js", "json", "html", "cshtml", "csproj", "css", "yml", "sql", "rs"] {
        assert!(matches!(engine(Mode::Fast, Some(lang)), Engine::Builtin), "{lang}");
        assert!(matches!(engine(Mode::Rich, Some(lang)), Engine::Syntect(_)), "{lang}");
    }
    assert!(matches!(engine(Mode::Fast, Some("cs")), Engine::CSharp));
    assert!(matches!(engine(Mode::Fast, Some("csx")), Engine::CSharp));
    assert!(matches!(engine(Mode::Rich, Some("cs")), Engine::Syntect(_)));
    assert!(matches!(engine(Mode::Fast, Some("scss")), Engine::Scss));
    for lang in ["ps1", "md", "Dockerfile"] {
        assert!(matches!(engine(Mode::Fast, Some(lang)), Engine::Syntect(_)), "{lang}");
    }
    for mode in [Mode::Fast, Mode::Rich] {
        assert!(matches!(engine(mode, Some("zzz-unknown")), Engine::None));
        assert!(matches!(engine(mode, None), Engine::None));
    }
    let code = samples::TYPESCRIPT;
    let hl = highlight(Palette::GithubDark, Some("ts"), code);
    assert!(
        hl.iter().all(|(_, p)| matches!(p, Paint::Color(_))),
        "builtin kinds map onto the palette"
    );
    let at = code.find("const rows").unwrap();
    let paint = hl.iter().find(|(r, _)| r.start <= at && at < r.end).map(|(_, p)| *p);
    assert_eq!(paint, Some(Paint::Color(Palette::GithubDark.color(Token::Keyword))));
}

#[test]
fn fast_scss_keeps_line_comments_apart_from_urls() {
    let code = "// note: don't\n.a { background: url(http://x.io/a.png); /* c */ color: red; } // tail\n";
    let p = Palette::GithubDark;
    let at = |needle| paint_at(Mode::Fast, p, "scss", code, needle);
    let comment = Some(Paint::Color(p.color(Token::Comment)));
    assert_eq!(at("// note"), comment);
    assert_eq!(at("// tail"), comment);
    assert_eq!(at("/* c */"), comment);
    assert_ne!(at("//x.io"), comment);
    assert_ne!(at("color"), comment);
    assert_ne!(at("color"), None);
    assert_tidy(code, &highlight_with(Mode::Fast, p, Some("scss"), code));
}

#[test]
fn every_sample_is_coloured_and_tidy_in_every_palette() {
    for mode in [Mode::Fast, Mode::Rich] {
        for palette in Palette::ALL {
            for (lang, code) in samples::ALL {
                let hl = highlight_with(mode, palette, Some(lang), code);
                assert_tidy(code, &hl);
                assert!(hl.len() >= 10, "{mode:?} {lang}: {} ranges", hl.len());
            }
        }
    }
}

#[test]
fn unknown_languages_fall_back_to_the_builtin_scanners() {
    let code = "fn main() { let x = 1; }";
    assert_eq!(
        highlight(Palette::GithubDark, Some("zzz-unknown"), code),
        builtin(Some("zzz-unknown"), code)
    );
    assert_eq!(highlight(Palette::GithubDark, None, code), builtin(None, code));
    assert!(!highlight(Palette::GithubDark, Some("rs"), code).is_empty());
}

#[test]
fn work_is_capped_at_the_rune_kit_limit() {
    let line = "var x = \"0123456789\"; // padding to make the line longer\n";
    let code = line.repeat(MAX_BYTES / line.len() * 2);
    let hl = highlight(Palette::GithubDark, Some("cs"), &code);
    assert_tidy(&code, &hl);
    assert!(hl.last().unwrap().0.end <= MAX_BYTES);
}

#[test]
fn palettes_are_distinct_and_keyed_apart() {
    for p in [Palette::GithubDark, Palette::GithubLight] {
        let colors = Token::ALL.map(|t| p.color(t));
        for i in 0..colors.len() {
            for j in 0..i {
                assert_ne!(colors[i], colors[j], "{p:?} tokens {i} and {j}");
            }
        }
    }
    let code = "public class A {}";
    let dark = highlighter(Palette::GithubDark).highlight(Some("cs"), code);
    let light = highlighter(Palette::GithubLight).highlight(Some("cs"), code);
    assert_ne!(dark, light);
    let rich = highlighter_with(Mode::Rich, Palette::GithubDark).highlight(Some("cs"), code);
    assert_ne!(dark, rich, "fast and rich keep separate cache entries");
}

const CHUNK: usize = 128;

/// `code` highlighted [`CHUNK`] lines at a time as the diff view does, each chunk from the state
/// the one before ended in (`carry`) or from a fresh one.
fn chunked(mode: Mode, lang: &str, code: &str, carry: bool) -> Vec<Highlight> {
    let lines: Vec<&str> = code.split_inclusive('\n').collect();
    let mut out = Vec::new();
    let mut from = Carry::default();
    let mut off = 0;
    for chunk in lines.chunks(CHUNK) {
        let text = chunk.concat();
        let (hl, end) = highlight_from(mode, Palette::GithubDark, Some(lang), &text, &from);
        assert_tidy(&text, &hl);
        out.extend(hl.into_iter().map(|(r, p)| (r.start + off..r.end + off, p)));
        off += text.len();
        from = if carry { end } else { Carry::default() };
    }
    out
}

fn per_byte(code: &str, hl: &[Highlight]) -> Vec<Option<Paint>> {
    let mut out = vec![None; code.len()];
    for (r, p) in hl {
        out[r.clone()].fill(Some(*p));
    }
    out
}

/// 150 lines: `open` on line 121, `inside` on 122 to 135 (so over the chunk's end at 128), `close`
/// on 136, with `before` and `after` around them.
fn crossing(before: &str, open: &str, inside: &str, close: &str, after: &str) -> String {
    (1..=150)
        .map(|n| match n {
            ..=120 => before,
            121 => open,
            122..=135 => inside,
            136 => close,
            _ => after,
        })
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Chunked with the state carried colours `code` as one pass over it does, line 130 as `inside`,
/// and chunked from a fresh state does not.
fn assert_carried(mode: Mode, lang: &str, code: &str, inside: Option<Token>) {
    let whole = per_byte(code, &highlight_with(mode, Palette::GithubDark, Some(lang), code));
    let line = |n: usize| code.split_inclusive('\n').take(n - 1).map(str::len).sum::<usize>();
    for n in [122, 128, 129, 130, 135] {
        let want = inside.map(|t| Paint::Color(Palette::GithubDark.color(t)));
        assert_eq!(whole[line(n)], want, "{mode:?} {lang}: line {n} in one pass");
    }
    let carried = per_byte(code, &chunked(mode, lang, code, true));
    for n in 1..=150 {
        let r = line(n)..line(n + 1);
        assert_eq!(carried[r.clone()], whole[r], "{mode:?} {lang}: line {n}");
    }
    let fresh = per_byte(code, &chunked(mode, lang, code, false));
    assert_ne!(fresh, whole, "{mode:?} {lang}: the sample does not cross a chunk");
}

#[test]
fn a_block_comment_over_a_chunk_end_stays_a_comment() {
    let c = Some(Token::Comment);
    let text = "still the comment, with 'quotes' and \"more\"";
    for (mode, lang, before, open, close, after) in [
        (Mode::Fast, "ts", "const a = 1;", "/* start", "end */", "const b = 2;"),
        (Mode::Rich, "ts", "const a = 1;", "/* start", "end */", "const b = 2;"),
        (Mode::Fast, "cs", "var a = 1;", "/* start", "end */", "var b = 2;"),
        (Mode::Rich, "cs", "var a = 1;", "/* start", "end */", "var b = 2;"),
        (
            Mode::Fast,
            "scss",
            ".a { color: red; }",
            "/* start",
            "end */",
            ".b { color: blue; }",
        ),
        (Mode::Fast, "sql", "SELECT 1;", "/* start", "end */", "SELECT 2;"),
        (Mode::Fast, "html", "<p>one</p>", "<!-- start", "end -->", "<p>two</p>"),
        (Mode::Fast, "ps1", "$a = 1", "<# start", "end #>", "$b = 2"),
    ] {
        assert_carried(mode, lang, &crossing(before, open, text, close, after), c);
    }
}

#[test]
fn a_multi_line_string_over_a_chunk_end_stays_a_string() {
    let s = Some(Token::String);
    let text = "still the string /* no comment */ and more";
    for (mode, lang, before, open, close, after) in [
        (Mode::Fast, "ps1", "$a = 1", "$s = @'", "'@", "$b = 2"),
        (Mode::Rich, "cs", "var a = 1;", "var s = @\"start", "end\";", "var b = 2;"),
        (Mode::Fast, "rs", "let a = 1;", "let s = \"start", "end\";", "let b = 2;"),
        (Mode::Fast, "py", "a = 1", "s = \"\"\"start", "end\"\"\"", "b = 2"),
        (Mode::Fast, "sql", "SELECT 1;", "SELECT 'start", "end';", "SELECT 2;"),
        (Mode::Fast, "go", "a := 1", "s := `start", "end`", "b := 2"),
    ] {
        assert_carried(mode, lang, &crossing(before, open, text, close, after), s);
    }
}

#[test]
fn a_template_literal_over_a_chunk_end_stays_a_string() {
    let s = Some(Token::String);
    for (mode, lang) in [(Mode::Fast, "ts"), (Mode::Fast, "tsx"), (Mode::Fast, "js"), (Mode::Rich, "ts")] {
        for text in [
            "still the template, it's \"quoted\"",
            "still the ${name} template, it's ${a + b} long",
        ] {
            let code = crossing("const a = 1;", "const s = `start", text, "end`;", "const b = 2;");
            assert_carried(mode, lang, &code, s);
        }
    }
    let fence = crossing("Some *words* here.", "```", "# not a heading", "```", "# A heading");
    assert_carried(Mode::Fast, "md", &fence, None);
}

#[test]
fn only_an_open_comment_or_string_is_carried() {
    let p = Palette::GithubDark;
    let end = |mode, lang, code| highlight_from(mode, p, Some(lang), code, &Carry::default()).1;
    for code in [
        "const a = 1; // note\n",
        "const a = 'x';\n",
        "/* closed */\n",
        "const a = `closed`;\n",
    ] {
        assert!(end(Mode::Fast, "ts", code).is_fresh(), "{code:?}");
    }
    let open = end(Mode::Fast, "ts", "/* open\n");
    assert!(!open.is_fresh());
    assert_eq!(open, end(Mode::Fast, "ts", "const a = 1;\n/* again\n"));
    assert_ne!(open, end(Mode::Fast, "ts", "const s = `open\n"));
    let (hl, still) = highlight_from(Mode::Fast, p, Some("ts"), "more\n", &open);
    assert_eq!(hl, [(0..5, Paint::Color(p.color(Token::Comment)))]);
    assert_eq!(still, open);
    let (hl, closed) = highlight_from(Mode::Fast, p, Some("ts"), "*/ let a;\n", &open);
    assert_eq!(hl[0], (0..2, Paint::Color(p.color(Token::Comment))));
    assert!(closed.is_fresh());
    let ps = end(Mode::Fast, "ps1", "<# open\n");
    assert_eq!(ps, end(Mode::Fast, "ps1", "<# open\n"));
    assert_ne!(ps, end(Mode::Fast, "ps1", "$a = 1\n"));
    let long = format!("/* open\n{}", "x\n".repeat(MAX_BYTES));
    assert!(end(Mode::Fast, "ts", &long).is_fresh(), "past the cap the end state is unknown");
    assert!(end(Mode::Fast, "ps1", &long.replace("/*", "<#")).is_fresh());
}

#[test]
#[ignore = "release perf: cargo test --release -- --ignored --nocapture"]
fn bench_chunks_and_warm() {
    let w = warm();
    println!("warm: {:.1} ms", w.as_secs_f64() * 1e3);
    for mode in [Mode::Fast, Mode::Rich] {
        for (lang, code) in samples::ALL {
            let lines: Vec<&str> = code.lines().collect();
            let chunk: String = (0..128).map(|i| format!("{}\n", lines[i % lines.len()])).collect();
            let t = Instant::now();
            highlight_with(mode, Palette::GithubDark, Some(lang), &chunk);
            let first = t.elapsed().as_secs_f64() * 1e3;
            let runs = 50;
            let t = Instant::now();
            for _ in 0..runs {
                std::hint::black_box(highlight_with(mode, Palette::GithubDark, Some(lang), &chunk));
            }
            let ms = t.elapsed().as_secs_f64() * 1e3 / runs as f64;
            let verdict = if ms <= 3.0 { "goal met" } else { "GOAL MISSED (<= 3 ms)" };
            println!("{mode:?} {lang:4}: {ms:6.2} ms per 128-line chunk (first {first:.2} ms), {verdict}");
        }
    }
    assert!(w.as_millis() <= 400, "warm took {w:?}");
}

#[test]
fn everforest_and_gruvbox_paint_only_their_palette_hexes() {
    let hex = |p: Palette, t| p.color(t).hex()[..7].to_owned();
    let everforest = [
        "#859289", "#dbbc7f", "#d699b6", "#e69875", "#7fbbb3", "#83c092", "#d3c6aa", "#a59c86", "#e69875", "#dbbc7f",
    ];
    let gruvbox = [
        "#928374", "#b8bb26", "#d3869b", "#fb4934", "#fabd2f", "#b8bb26", "#83a598", "#a89984", "#83a598", "#8ec07c",
    ];
    for (p, want) in [(Palette::Everforest, everforest), (Palette::Gruvbox, gruvbox)] {
        assert_eq!(Token::ALL.map(|t| hex(p, t)), want, "{p:?}");
        assert!(p.is_dark());
        let main = [Token::Comment, Token::String, Token::Keyword, Token::Type, Token::Number].map(|t| p.color(t));
        for i in 0..main.len() {
            for j in 0..i {
                assert_ne!(main[i], main[j], "{p:?} keeps comments, strings, keywords, types and numbers apart");
            }
        }
        let theme = p.syntax_theme(rune_kit::syntax::SyntaxTheme::neutral(true));
        assert_eq!(theme.color(rune_kit::syntax::Syntax::Keyword), p.color(Token::Keyword));
        assert_eq!(theme.color(rune_kit::syntax::Syntax::Property), p.color(Token::Variable));
        let code = "public class A {}";
        assert_ne!(
            highlighter(p).highlight(Some("cs"), code),
            highlighter(Palette::GithubDark).highlight(Some("cs"), code)
        );
    }
}
