use std::time::Instant;

use rain_syntax::{
    Engine, MAX_BYTES, Mode, Palette, Token, engine, find_syntax, highlight, highlight_with, highlighter, highlighter_with, samples,
    tokens, warm,
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
        "#859289", "#83c092", "#d699b6", "#e67e80", "#dbbc7f", "#a7c080", "#7fbbb3", "#9da9a0", "#e69875", "#dbbc7f",
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
