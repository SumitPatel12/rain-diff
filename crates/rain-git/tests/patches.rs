use rain_core::{FileDiff, FileStatus, LineKind};
use rain_git::{parse_reader, parse_str};

const S: &str = include_str!("../../../fixtures/s.patch");
const M: &str = include_str!("../../../fixtures/m.patch");

fn file<'a>(files: &'a [FileDiff], path: &str) -> &'a FileDiff {
    files.iter().find(|f| f.path == path).unwrap_or_else(|| panic!("no file {path}"))
}

fn totals(files: &[FileDiff]) -> (u32, u32) {
    files.iter().fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions))
}

/// `(kind, old, new)` of the first and last line of a file.
fn ends(f: &FileDiff) -> [(LineKind, u32, u32); 2] {
    let l = |r: &rain_core::LineRec| (r.kind, r.old, r.new);
    [l(&f.lines[0]), l(f.lines.last().unwrap())]
}

#[test]
fn s_patch_matches_hand_checked_expectations() {
    let p = parse_str(S, 3);
    let files = &p.files;
    assert_eq!(files.len(), 6);
    assert_eq!(totals(files), (10, 32));
    assert!(files.iter().all(|f| f.repo == 3));

    let html = file(files, "app/src/app/vouchers/legacy-voucher.component.html");
    assert_eq!(html.status, FileStatus::Deleted);
    assert_eq!((html.additions, html.deletions), (0, 20));
    assert_eq!(ends(html), [(LineKind::Removed, 1, 0), (LineKind::Removed, 20, 0)]);
    assert_eq!(html.line_text(0), "<section class=\"app-legacy-voucher\">");

    let scss = file(files, "app/src/app/vouchers/voucher-form.component.scss");
    assert_eq!(scss.status, FileStatus::Modified);
    assert_eq!(scss.hunks.len(), 2);
    assert_eq!((scss.hunks[1].old_start, scss.hunks[1].new_start), (17, 17));
    assert_eq!(ends(scss), [(LineKind::Removed, 1, 0), (LineKind::Context, 23, 23)]);
    let blank_removed = scss.lines.iter().position(|l| l.kind == LineKind::Removed && l.old == 20).unwrap();
    assert_eq!(scss.line_text(blank_removed as u32), "");

    let ts = file(files, "app/src/app/vouchers/vouchers-form.component.ts");
    assert_eq!(ts.status, FileStatus::Renamed);
    assert_eq!(ts.old_path.as_deref(), Some("app/src/app/vouchers/voucher-form.component.ts"));
    let i = files.iter().position(|f| std::ptr::eq(f, ts)).unwrap();
    assert_eq!(p.meta[i].similarity, Some(95));
    assert_eq!(p.meta[i].new_mode.as_deref(), Some("100644"));

    let json = file(files, "app/src/assets/i18n/en.json");
    let last = json.lines.last().unwrap();
    assert!(last.no_newline && last.kind == LineKind::Added && last.new == 16);
    assert_eq!(json.line_text(json.line_count() - 1), "}");
    assert!(!json.lines[json.lines.len() - 2].no_newline);
    assert!(json.max_columns > 2_000);

    let png = file(files, "app/src/assets/voucher-icon.png");
    assert!(png.binary && png.contents_unavailable && png.lines.is_empty());

    let cs = file(files, "server/src/Services/VoucherService.cs");
    assert_eq!((cs.additions, cs.deletions), (3, 5));
    assert_eq!(cs.hunk_section(&cs.hunks[0]), "namespace Acme.Services");
    assert_eq!(ends(cs), [(LineKind::Context, 10, 10), (LineKind::Context, 72, 70)]);
    assert!((0..cs.line_count()).all(|i| !cs.line_text(i).ends_with('\r')));
    assert_eq!(cs.line_text(0), "    {");
}

#[test]
fn m_patch_matches_hand_checked_expectations() {
    let p = parse_str(M, 0);
    let files = &p.files;
    assert_eq!(files.len(), 40);
    assert_eq!(totals(files), (2481, 691));
    let count = |s: FileStatus| files.iter().filter(|f| f.status == s).count();
    assert_eq!(
        (count(FileStatus::Renamed), count(FileStatus::Deleted), count(FileStatus::Added)),
        (2, 2, 3)
    );

    let zh = file(files, "app/src/assets/i18n/礼券.json");
    assert_eq!(zh.status, FileStatus::Modified);
    assert_eq!((zh.additions, zh.deletions), (3, 2));
    assert_eq!(ends(zh), [(LineKind::Context, 1, 1), (LineKind::Context, 4, 5)]);
    assert_eq!(zh.line_text(3), "  \"voucher\": \"礼券 💵\",");

    let moved = file(files, "app/src/app/cart2/moved/cart2-form.component.ts");
    assert_eq!(moved.old_path.as_deref(), Some("app/src/app/cart2/cart2-form.component.ts"));
    let i = files.iter().position(|f| std::ptr::eq(f, moved)).unwrap();
    assert_eq!(p.meta[i].similarity, Some(99));

    let mig = file(files, "server/src/Acme.Data/Migrations/20260101000000_AddVoucherColumns.cs");
    assert_eq!(mig.status, FileStatus::Added);
    assert_eq!(mig.deletions, 0);
    assert_eq!(ends(mig)[0], (LineKind::Added, 0, 1));
    assert_eq!(mig.lines.last().unwrap().new, mig.additions);
}

#[test]
fn parse_reader_streams_the_same_as_parse_str_and_handles_crlf_patches() {
    let a = parse_str(M, 0);
    let b = parse_reader(std::io::BufReader::with_capacity(7, M.as_bytes()), 0).unwrap();
    assert_eq!(format!("{:?}", a.files), format!("{:?}", b.files));
    assert!(!M.contains('\r'));
    let crlf = M.replace('\n', "\r\n");
    assert_eq!(format!("{:?}", parse_str(&crlf, 0).files), format!("{:?}", a.files));
}

#[test]
fn plain_unified_diffs_and_trimmed_context_lines_parse() {
    let p = parse_str("note\n--- a/f.txt\t2026-01-01\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n one\n\n-two\n+2\n", 0);
    let f = &p.files[0];
    assert_eq!((f.path.as_str(), f.status), ("f.txt", FileStatus::Modified));
    assert_eq!(f.line_count(), 4);
    assert_eq!((f.lines[1].kind, f.line_text(1)), (LineKind::Context, ""));
    let added = parse_str("--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+fn main() {}\n", 0);
    assert_eq!(added.files[0].status, FileStatus::Added);
    assert_eq!(ends(&added.files[0]), [(LineKind::Added, 0, 1), (LineKind::Added, 0, 1)]);
}

#[test]
fn removed_lines_that_look_like_headers_stay_in_the_hunk() {
    let p = parse_str(
        "diff --git a/x.sql b/x.sql\n--- a/x.sql\n+++ b/x.sql\n@@ -1,2 +1,1 @@\n--- comment\n-++ b\n+x\n",
        0,
    );
    let f = &p.files[0];
    assert_eq!(p.files.len(), 1);
    assert_eq!((f.additions, f.deletions), (1, 2));
    assert_eq!(f.line_text(0), "-- comment");
}

#[test]
fn copies_modes_and_submodules_set_status_and_availability() {
    let p = parse_str(
        "diff --git a/a.txt b/b.txt\nsimilarity index 100%\ncopy from a.txt\ncopy to b.txt\n\
         diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n\
         diff --git a/lib b/lib\nindex 1111111..2222222 160000\n--- a/lib\n+++ b/lib\n@@ -1 +1 @@\n\
         -Subproject commit 1111111111111111111111111111111111111111\n\
         +Subproject commit 2222222222222222222222222222222222222222\n",
        0,
    );
    assert_eq!(p.files.len(), 3);
    assert_eq!(p.files[0].status, FileStatus::Copied);
    assert_eq!(p.files[0].old_path.as_deref(), Some("a.txt"));
    assert_eq!(p.meta[0].similarity, Some(100));
    assert_eq!(p.files[1].status, FileStatus::Modified);
    assert_eq!(
        (p.meta[1].old_mode.as_deref(), p.meta[1].new_mode.as_deref()),
        (Some("100644"), Some("100755"))
    );
    assert!(p.files[2].contents_unavailable && !p.files[2].binary);
}

#[test]
fn hostile_and_fuzzed_patches_never_panic_or_blow_up() {
    let cases = [
        "",
        "diff --git\n--- \n+++ \n@@ -4294967295,4294967295 +4294967295,4294967295 @@\n-a\n+b\n\\ No newline",
        "diff --git \"a/unclosed b/unclosed\n--- \"a/\\777\\x\n+++ b/\n@@ -0,0 +0,0 @@\n",
        "diff --git 🚀 a/x 🚀 b/y\n@@ -1,999999 +1,999999 @@\n+truncated",
        "--- a/f\n+++ b/f\n@@ -10,2 +10,2 @@\n ctx\n-old\n+new\n@@ -2,1 +2,1 @@\n-out_of_order\n+ok\n",
        "diff --git a/é b/é\n\\ No newline\n@@ -1 +1 @@\n\\\n",
    ];
    for patch in cases {
        let p = parse_str(patch, 0);
        check(&p.files);
    }

    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed as usize) % n
    };
    let fragments = [
        "diff --git a/f.rs b/f.rs\n",
        "diff --git \"a/sp ace\\303\\251\" \"b/sp ace\\n\"\n",
        "--- a/f.rs\n",
        "+++ b/f.rs\n",
        "--- /dev/null\n",
        "+++ /dev/null\n",
        "@@ -1,3 +1,3 @@ fn f()\n",
        "@@ -0,0 +1,5 @@\n",
        "@@ -4294967295,10 +0,4294967295 @@\n",
        "@@ malformed @@\n",
        " context line\n",
        "-removed line\n",
        "+added line\n",
        "\\ No newline at end of file\n",
        "rename from \"a/\\123\"\n",
        "similarity index 999%\n",
        "Binary files a/x and b/x differ\n",
        "\r\n",
        "\n",
        "\u{ff}\u{fe}\n",
    ];
    for _ in 0..2_000 {
        let len = 1 + next(32);
        let patch: String = (0..len).map(|_| fragments[next(fragments.len())]).collect();
        let p = parse_str(&patch, 0);
        assert!(p.files.len() <= len, "more files than lines");
        check(&p.files);
        let mut bytes = patch.into_bytes();
        if !bytes.is_empty() {
            let at = next(bytes.len());
            bytes[at] = 0xff;
        }
        check(&parse_reader(&bytes[..], 0).unwrap().files);
    }
}

/// Every record points inside the arena, hunks index real lines, nothing is over-allocated.
fn check(files: &[FileDiff]) {
    for f in files {
        assert!(f.text.capacity() <= f.text.len().max(64) * 2 + 64, "arena over-allocated");
        for (i, l) in f.lines.iter().enumerate() {
            assert!(l.start <= l.end && l.end as usize <= f.text.len());
            let _ = f.line_text(i as u32);
        }
        for h in &f.hunks {
            assert!(h.lines.start <= h.lines.end && h.lines.end <= f.line_count());
            let _ = f.hunk_section(h);
        }
    }
}
