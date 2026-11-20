use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use rain_core::{FileDiff, FileStatus, FileTag, RepoInfo};

use crate::sample::{Sample, sample};
use crate::{NodeKind, TreeFilter, TreeState};

fn state(s: &Sample) -> TreeState {
    TreeState::build(&s.files, &s.repos, &s.tags, |k| s.viewed.contains(k), &s.comments)
}

fn file(repo: u16, path: &str) -> FileDiff {
    FileDiff {
        repo,
        path: path.into(),
        status: FileStatus::Modified,
        additions: 1,
        ..FileDiff::default()
    }
}

fn repos() -> Vec<RepoInfo> {
    ["server", "app"]
        .map(|id| RepoInfo {
            id: id.into(),
            ..RepoInfo::default()
        })
        .to_vec()
}

/// Each visible row as `depth name`, `[x]`/`[ ]` for files' viewed marks.
fn outline(t: &TreeState) -> Vec<String> {
    (0..t.model.visible_len())
        .map(|i| {
            let (n, r) = t.model.row(i).unwrap();
            let mark = match n.kind {
                NodeKind::File(f) if t.files[f as usize].viewed => "[x] ",
                NodeKind::File(_) => "[ ] ",
                _ => "",
            };
            format!("{}{mark}{}", "  ".repeat(r.depth as usize), n.name)
        })
        .collect()
}

fn tags(pairs: &[(&str, FileTag)]) -> BTreeMap<String, FileTag> {
    pairs.iter().map(|(k, t)| (k.to_string(), *t)).collect()
}

fn small() -> TreeState {
    let files = vec![
        file(1, "src/app/vouchers/voucher.service.ts"),
        file(0, "src/Api/Controllers/VouchersController.cs"),
        file(0, "src/Api/Controllers/AccountController.cs"),
        file(0, "src/Data/Migrations/20261101_AddVoucher.cs"),
        file(0, "README.md"),
        file(1, "src/app/vouchers/voucher.component.ts"),
    ];
    let t = tags(&[
        ("server/src/Api/Controllers/VouchersController.cs", FileTag::MustReview),
        ("app/src/app/vouchers/voucher.service.ts", FileTag::MustReview),
        ("server/src/Api/Controllers/AccountController.cs", FileTag::Skim),
        ("server/src/Data/Migrations/20261101_AddVoucher.cs", FileTag::Generated),
    ]);
    let comments = [("app/src/app/vouchers/voucher.component.ts".to_string(), 2)].into();
    TreeState::build(&files, &repos(), &t, |k| k == "server/README.md", &comments)
}

#[test]
fn repos_are_roots_in_launch_order_with_folders_first_and_compacted() {
    let t = small();
    assert_eq!(
        outline(&t),
        [
            "server",
            "  src",
            "    Api/Controllers",
            "      [ ] AccountController.cs",
            "      [ ] VouchersController.cs",
            "    Data/Migrations",
            "      [ ] 20261101_AddVoucher.cs",
            "  [x] README.md",
            "app",
            "  src/app/vouchers",
            "    [ ] voucher.component.ts",
            "    [ ] voucher.service.ts",
        ]
    );
    let e = t.file("server/README.md").unwrap();
    assert_eq!((e.tag, e.viewed), (FileTag::Review, true), "untagged files default to Review");
    assert_eq!(t.file("app/src/app/vouchers/voucher.component.ts").unwrap().comments, 2);
}

#[test]
fn must_review_filter_keeps_only_those_files_and_their_folders() {
    let mut t = small();
    t.apply(&TreeFilter {
        tags: [FileTag::MustReview].into(),
        ..TreeFilter::default()
    });
    assert_eq!(
        outline(&t),
        [
            "server",
            "  src",
            "    Api/Controllers",
            "      [ ] VouchersController.cs",
            "app",
            "  src/app/vouchers",
            "    [ ] voucher.service.ts",
        ]
    );
    assert_eq!(
        t.shown_keys(),
        [
            "server/src/Api/Controllers/VouchersController.cs",
            "app/src/app/vouchers/voucher.service.ts"
        ]
    );
    t.apply(&TreeFilter::default());
    assert_eq!(t.model.visible_len(), 12, "clearing the filter shows everything again");
}

#[test]
fn unviewed_and_commented_toggles() {
    let mut t = small();
    t.apply(&TreeFilter {
        unviewed_only: true,
        ..TreeFilter::default()
    });
    assert!(!t.is_shown("server/README.md") && t.is_shown("server/src/Api/Controllers/VouchersController.cs"));
    assert_eq!(t.shown_keys().len(), 5);
    t.apply(&TreeFilter {
        commented_only: true,
        ..TreeFilter::default()
    });
    assert_eq!(t.shown_keys(), ["app/src/app/vouchers/voucher.component.ts"]);
}

#[test]
fn text_filter_finds_vouchers_by_name_and_folder() {
    let mut t = small();
    t.apply(&TreeFilter {
        text: "vouchers".into(),
        ..TreeFilter::default()
    });
    let keys = t.shown_keys();
    assert!(keys.contains(&"server/src/Api/Controllers/VouchersController.cs"));
    assert!(keys.contains(&"app/src/app/vouchers/voucher.service.ts"));
    assert!(!keys.contains(&"server/src/Api/Controllers/AccountController.cs"));
    assert!(!keys.contains(&"server/README.md"));
    t.apply(&TreeFilter {
        text: "VOUCHERS ctrl".into(),
        ..TreeFilter::default()
    });
    assert_eq!(
        t.shown_keys(),
        ["server/src/Api/Controllers/VouchersController.cs"],
        "terms AND, case aside"
    );
}

#[test]
fn chip_counts_ignore_the_tag_filter_but_follow_the_rest() {
    let mut t = small();
    let counts = |t: &TreeState| FileTag::ALL.map(|g| t.tag_count(g));
    // Must, Review, Skim, Formatting, Generated, Ignored, Uncovered
    assert_eq!(counts(&t), [2, 2, 1, 0, 1, 0, 0]);
    t.apply(&TreeFilter {
        tags: [FileTag::Skim].into(),
        ..TreeFilter::default()
    });
    assert_eq!(counts(&t), [2, 2, 1, 0, 1, 0, 0], "a chosen tag doesn't zero the other chips");
    t.apply(&TreeFilter {
        unviewed_only: true,
        ..TreeFilter::default()
    });
    assert_eq!(counts(&t), [2, 1, 1, 0, 1, 0, 0], "README is viewed");
}

#[test]
fn repo_filter_keeps_only_that_repos_files_and_their_folders() {
    let mut t = small();
    assert_eq!(t.repos(), [(0, "server".to_string()), (1, "app".to_string())]);
    t.apply(&TreeFilter {
        repos: [1].into(),
        ..TreeFilter::default()
    });
    assert!(!t.filter.is_empty());
    assert_eq!(
        outline(&t),
        [
            "app",
            "  src/app/vouchers",
            "    [ ] voucher.component.ts",
            "    [ ] voucher.service.ts"
        ]
    );
    assert_eq!(
        t.shown_keys(),
        [
            "app/src/app/vouchers/voucher.component.ts",
            "app/src/app/vouchers/voucher.service.ts"
        ],
        "tree order unchanged"
    );
    t.apply(&TreeFilter::default());
    assert_eq!(t.model.visible_len(), 12);
}

#[test]
fn repo_counts_ignore_the_repo_axis_and_tag_counts_honour_it() {
    let mut t = small();
    let counts = |t: &TreeState| FileTag::ALL.map(|g| t.tag_count(g));
    assert_eq!((t.repo_count(0), t.repo_count(1)), (4, 2));
    t.apply(&TreeFilter {
        repos: [0].into(),
        ..TreeFilter::default()
    });
    assert_eq!((t.repo_count(0), t.repo_count(1)), (4, 2), "a chosen repo doesn't zero the others");
    assert_eq!(counts(&t), [1, 1, 1, 0, 1, 0, 0], "tag chips count server only");
    t.apply(&TreeFilter {
        tags: [FileTag::MustReview].into(),
        ..TreeFilter::default()
    });
    assert_eq!((t.repo_count(0), t.repo_count(1)), (1, 1), "repo chips honour the tag axis");
    t.apply(&TreeFilter {
        unviewed_only: true,
        ..TreeFilter::default()
    });
    assert_eq!(t.repo_count(0), 3, "and the unviewed toggle (README is viewed)");
}

#[test]
fn repo_and_tag_filters_combine() {
    let mut t = small();
    t.apply(&TreeFilter {
        repos: [0].into(),
        tags: [FileTag::MustReview].into(),
        ..TreeFilter::default()
    });
    assert_eq!(t.shown_keys(), ["server/src/Api/Controllers/VouchersController.cs"]);
    assert_eq!(t.folder_counts("server"), (0, 1));
    assert_eq!(t.folder_counts("app"), (0, 0));
    t.apply(&TreeFilter {
        repos: [0, 1].into(),
        tags: [FileTag::MustReview].into(),
        ..TreeFilter::default()
    });
    assert_eq!(
        t.shown_keys(),
        [
            "server/src/Api/Controllers/VouchersController.cs",
            "app/src/app/vouchers/voucher.service.ts"
        ]
    );
}

#[test]
fn a_single_repo_lists_one_root_and_no_repo_roots_list_none() {
    let one = vec![RepoInfo {
        id: "server".into(),
        ..RepoInfo::default()
    }];
    let t = TreeState::build(&[file(0, "a.rs")], &one, &BTreeMap::new(), |_| false, &BTreeMap::new());
    assert_eq!(t.repos().len(), 1);
    let t = TreeState::build(&[file(0, "a.rs")], &[], &BTreeMap::new(), |_| false, &BTreeMap::new());
    assert!(t.repos().is_empty());
    let t = TreeState::build(&[file(1, "a.rs")], &repos(), &BTreeMap::new(), |_| false, &BTreeMap::new());
    assert_eq!(t.repos(), [(1, "app".to_string())], "repos without files have no root");
}

#[test]
fn progress_folder_counts_and_marks() {
    let mut t = small();
    let p = t.progress();
    assert_eq!((p.viewed, p.total, p.must_viewed, p.must_total), (1, 6, 0, 2));
    assert_eq!(t.folder_counts("server"), (1, 4));
    assert_eq!(t.folder_counts("server/src/Api/Controllers"), (0, 2));
    assert!(t.set_viewed("server/src/Api/Controllers/VouchersController.cs", true));
    assert!(
        !t.set_viewed("server/src/Api/Controllers/VouchersController.cs", true),
        "no change, no report"
    );
    assert_eq!(t.progress().must_viewed, 1);
    assert_eq!(t.folder_counts("server/src/Api/Controllers"), (1, 2));
    let changed = t.set_viewed_under("server/src/Api/Controllers", true);
    assert_eq!(changed, ["server/src/Api/Controllers/AccountController.cs"]);
    assert_eq!(t.folder_counts("server"), (3, 4));
    let changed = t.mark_tag_viewed(FileTag::MustReview);
    assert_eq!(changed, ["app/src/app/vouchers/voucher.service.ts"]);
    let p = t.progress();
    assert_eq!((p.viewed, p.must_viewed), (4, 2));
    assert_eq!(t.tag_progress(FileTag::MustReview), (2, 2));
}

#[test]
fn folder_marks_reach_only_the_files_the_filter_shows() {
    let mut t = small();
    t.apply(&TreeFilter {
        tags: [FileTag::Skim].into(),
        ..TreeFilter::default()
    });
    assert_eq!(t.folder_counts("server"), (0, 1));
    assert_eq!(
        t.set_viewed_under("server", true),
        ["server/src/Api/Controllers/AccountController.cs"]
    );
    assert!(!t.file("server/src/Api/Controllers/VouchersController.cs").unwrap().viewed);
}

#[test]
fn retagging_reapplies_the_filter() {
    let mut t = small();
    t.apply(&TreeFilter {
        tags: [FileTag::MustReview].into(),
        ..TreeFilter::default()
    });
    t.set_tags(&tags(&[("server/README.md", FileTag::MustReview)]));
    assert_eq!(t.shown_keys(), ["server/README.md"]);
    assert_eq!(
        t.file("server/src/Data/Migrations/20261101_AddVoucher.cs").unwrap().tag,
        FileTag::Review,
        "untagged non-generated files fall back to Review"
    );
}

#[test]
fn files_without_a_known_repo_sit_at_the_top_level() {
    let t = TreeState::build(
        &[file(0, "a/b.rs"), file(0, "c.rs")],
        &[],
        &BTreeMap::new(),
        |_| false,
        &BTreeMap::new(),
    );
    assert_eq!(outline(&t), ["a", "  [ ] b.rs", "[ ] c.rs"]);
    let unnamed = vec![RepoInfo {
        name: "rain".into(),
        ..RepoInfo::default()
    }];
    let t = TreeState::build(&[file(0, "c.rs")], &unnamed, &BTreeMap::new(), |_| false, &BTreeMap::new());
    assert_eq!(outline(&t), ["rain", "  [ ] c.rs"]);
    assert!(t.file("c.rs").is_some(), "a repo without an id keys files by bare path");
}

#[test]
fn sample_has_the_named_files_and_two_roots() {
    let s = sample(300);
    let t = state(&s);
    assert_eq!(t.files.len(), 300);
    let keys: BTreeSet<&str> = t.files.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys.len(), 300, "keys are unique");
    assert!(keys.contains("server/src/Acme.Api/Controllers/VouchersController.cs"));
    let roots: Vec<&str> = t.model.children(None).collect();
    assert_eq!(roots, ["server", "app"]);
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
fn apply_on_302_files_is_fast() {
    let s = sample(302);
    let mut t = state(&s);
    let filters = [
        TreeFilter {
            tags: [FileTag::MustReview].into(),
            ..TreeFilter::default()
        },
        TreeFilter {
            unviewed_only: true,
            text: "voucher".into(),
            ..TreeFilter::default()
        },
        TreeFilter {
            repos: [1].into(),
            tags: [FileTag::Review].into(),
            ..TreeFilter::default()
        },
        TreeFilter::default(),
    ];
    let mut worst = 0.0f64;
    for _ in 0..5 {
        for f in &filters {
            let at = Instant::now();
            t.apply(f);
            let _ = t.model.visible_len();
            worst = worst.max(ms(at));
        }
    }
    let budget = if cfg!(debug_assertions) { 20.0 } else { 2.0 };
    println!("apply on 302 files: worst {worst:.3} ms (budget {budget} ms)");
    assert!(worst <= budget, "apply took {worst:.3} ms");
}

#[test]
#[ignore = "release perf"]
fn build_5k_files_under_50ms() {
    let s = sample(5000);
    let mut best = f64::MAX;
    for _ in 0..5 {
        let at = Instant::now();
        let t = state(&s);
        let _ = t.model.visible_len();
        best = best.min(ms(at));
    }
    let mut t = state(&s);
    let at = Instant::now();
    t.apply(&TreeFilter {
        tags: [FileTag::Review].into(),
        ..TreeFilter::default()
    });
    let _ = t.model.visible_len();
    let apply = ms(at);
    println!("5k files: build {best:.2} ms, apply {apply:.2} ms");
    assert!(best <= 50.0, "build took {best:.2} ms");
}

#[test]
fn file_icons_follow_the_material_theme_rules() {
    use crate::icon_name;
    for (name, icon) in [
        ("main.rs", "rust"),
        ("foo.component.ts", "angular-component"),
        ("foo.spec.ts", "test-ts"),
        ("index.d.ts", "typescript-def"),
        ("app.ts", "typescript"),
        ("Dockerfile", "docker"),
        ("package.json", "nodejs"),
        ("tsconfig.app.json", "tsconfig"),
        ("x.cs", "csharp"),
        ("X.CS", "csharp"),
        ("README.md", "readme"),
        ("notes.md", "markdown"),
        (".gitignore", "git"),
        ("Cargo.lock", "lock"),
        ("data.unknownext", "file"),
        ("Makefile.unknown", "file"),
        ("noextension", "file"),
    ] {
        assert_eq!(icon_name(name), icon, "{name}");
    }
}

#[test]
fn every_shipped_icon_rasterizes_and_the_tables_are_sorted() {
    use crate::material::{FILE_EXTENSIONS, FILE_NAMES, ICONS};
    for table in [&FILE_NAMES[..], &FILE_EXTENSIONS[..]] {
        assert!(table.windows(2).all(|w| w[0].0 < w[1].0), "sorted and unique for binary search");
        assert!(table.iter().all(|(_, i)| (*i as usize) < ICONS.len()));
    }
    let cache = rune::ImageCache::new(std::sync::Arc::new(|| {}));
    for svg in &ICONS {
        for bytes in std::iter::once(svg.dark).chain(svg.light) {
            let image = cache.svg(bytes, 16.0, 2.0);
            assert!(image.wait().is_ok(), "{} fails to rasterize: {:?}", svg.name, image.error());
        }
    }
}
