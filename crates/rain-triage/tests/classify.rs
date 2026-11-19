use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rain_core::overview::RiskLevel;
use rain_core::{FileDiff, FileDiffBuilder, FileStatus, LineKind};
use rain_overview::Rated;
use rain_triage::{Group, Input, Item, Shape, Triage, classify};

const GROUPS: &str = include_str!("fixtures/groups.patch");
const M: &str = include_str!("../../../fixtures/m.patch");

struct Loaded {
    files: Vec<FileDiff>,
    keys: Vec<String>,
    similarity: Vec<Option<u8>>,
}

fn load(patch: &str) -> Loaded {
    let p = rain_git::parse_str(patch, 0);
    let keys = p.files.iter().map(|f| f.path.clone()).collect();
    Loaded {
        similarity: p.meta.iter().map(|m| m.similarity).collect(),
        keys,
        files: p.files,
    }
}

fn run(l: &Loaded) -> Triage {
    classify(&Input {
        files: &l.files,
        keys: &l.keys,
        similarity: &l.similarity,
        ..Input::default()
    })
}

fn item<'a>(t: &'a Triage, key: &str) -> &'a Item {
    t.items.iter().find(|i| i.key == key).unwrap_or_else(|| panic!("no {key}"))
}

fn keys(t: &Triage) -> Vec<&str> {
    t.items.iter().map(|i| i.key.as_str()).collect()
}

#[test]
fn every_group_has_its_reason() {
    let t = run(&load(GROUPS));
    let want: [(&str, Group, &str, bool); 15] = [
        ("src/Api/PaymentsController.cs", Group::Must, "public API (controller)", false),
        ("src/Data/Migrations/20260101_AddVoucher.cs", Group::Must, "migration", false),
        ("src/Services/VoucherService.cs", Group::Must, "money path (Voucher)", false),
        ("crates/x/src/parse.rs", Group::Review, "change", true),
        ("src/Math/Calc.cs", Group::Review, "moved with 2 edits (rename)", true),
        ("src/Services/Totals.cs", Group::Review, "formatting + real line (L4)", true),
        ("src/Util/Strings.cs", Group::Review, "change", true),
        ("tests/VoucherServiceTests.cs", Group::Tests, "test (pairs VoucherService.cs)", true),
        ("src/app/form.spec.ts", Group::Tests, "test", true),
        ("src/New/Relocator.cs", Group::Moves, "moved (R100)", true),
        (
            "src/Services/Spacing.cs",
            Group::FormattingOnly,
            "formatting-only (whitespace)",
            true,
        ),
        ("src/Api/Schema.cs", Group::Generated, "generated (header)", true),
        ("src/Gen/Client.g.cs", Group::Generated, "generated (path rule)", true),
        ("package-lock.json", Group::Lockfile, "lockfile", true),
        ("assets/logo.png", Group::Binary, "binary", true),
    ];
    assert_eq!(keys(&t), want.iter().map(|w| w.0).collect::<Vec<_>>(), "list order");
    for (key, group, text, trusted) in want {
        let i = item(&t, key);
        assert_eq!(
            (i.group, i.reason.group, i.reason.text.as_str(), i.reason.trusted),
            (group, group, text, trusted),
            "{key}"
        );
    }
    assert_eq!(item(&t, "src/Services/Totals.cs").real_line, Some(4));
    assert_eq!(item(&t, "src/Math/Calc.cs").moved_from, Some(("src/Calc.cs".into(), 2)));
    assert_eq!(
        item(&t, "tests/VoucherServiceTests.cs").pair.as_deref(),
        Some("src/Services/VoucherService.cs")
    );
    assert_eq!(
        item(&t, "src/Services/VoucherService.cs").pair.as_deref(),
        Some("tests/VoucherServiceTests.cs")
    );
    assert_eq!(item(&t, "src/Services/VoucherService.cs").why, None);
    assert_eq!(
        item(&t, "src/Data/Migrations/20260101_AddVoucher.cs").why.as_deref(),
        Some("money path (Voucher); no test touched")
    );
    assert_eq!(
        item(&t, "src/Api/PaymentsController.cs").why.as_deref(),
        Some("money path (Payments); no test touched")
    );
    assert_eq!(item(&t, "src/Api/PaymentsController.cs").risk, Some(RiskLevel::High));
    assert_eq!(item(&t, "src/Util/Strings.cs").risk, None);
}

#[test]
fn facts_count_groups_tests_and_tests_turned_off() {
    let t = run(&load(GROUPS));
    let f = &t.facts;
    assert_eq!((f.files, f.real, f.tests, f.tests_added, f.tests_removed), (15, 7, 2, 1, 0));
    assert_eq!((f.migrations, f.renames, f.binaries, f.uncited), (1, 2, 1, None));
    let off: Vec<(&str, u32, &str)> = f.tests_off.iter().map(|o| (o.key.as_str(), o.line, o.marker.as_str())).collect();
    assert_eq!(
        off,
        [
            ("crates/x/src/parse.rs", 21, "#[ignore"),
            ("src/app/form.spec.ts", 2, ".skip("),
            ("tests/VoucherServiceTests.cs", 5, "(Skip ="),
        ]
    );
    let groups: Vec<(Group, usize)> = t.groups.iter().map(|g| (g.group, g.count)).collect();
    assert_eq!(
        groups,
        [
            (Group::Must, 3),
            (Group::Review, 4),
            (Group::Tests, 2),
            (Group::Moves, 1),
            (Group::FormattingOnly, 1),
            (Group::Generated, 2),
            (Group::Lockfile, 1),
            (Group::Binary, 1),
        ]
    );
    let must = &t.groups[0];
    assert!(!must.trusted);
    assert_eq!(must.lines, 2 + 2 + 1);
    assert!(t.groups[5].trusted);
    assert_eq!(t.shape, Shape::Dense);
}

#[test]
fn numstat_huge_churn_and_overview_signals() {
    let mut l = load(GROUPS);
    let mut big = FileDiffBuilder::new(0, "src/Big.cs", FileStatus::Modified);
    big.push_hunk(1, 0, 1, 20_001, "");
    for i in 0..20_001 {
        big.push_line(LineKind::Added, 0, i + 1, "x");
    }
    let mut churn = FileDiffBuilder::new(0, "src/Report.cs", FileStatus::Modified);
    churn.push_hunk(1, 0, 1, 400, "");
    for i in 0..400 {
        churn.push_line(LineKind::Added, 0, i + 1, &format!("line {i}"));
    }
    for f in [big.finish(), churn.finish()] {
        l.keys.push(f.path.clone());
        l.files.push(f);
        l.similarity.push(None);
    }
    let formatting: BTreeSet<String> = ["src/Util/Strings.cs".to_owned()].into();
    let ignored: BTreeSet<String> = ["crates/x/src/parse.rs".to_owned()].into();
    let rated: BTreeMap<String, Rated> = [
        (
            "src/Services/Totals.cs".to_owned(),
            Rated {
                order: 0,
                risk: Some(RiskLevel::Critical),
                why: "Totals feed every account".into(),
            },
        ),
        (
            "src/Services/VoucherService.cs".to_owned(),
            Rated {
                order: 1,
                risk: Some(RiskLevel::Low),
                why: String::new(),
            },
        ),
        (
            "src/Gen/Client.g.cs".to_owned(),
            Rated {
                order: 2,
                risk: Some(RiskLevel::Medium),
                why: "regenerated client".into(),
            },
        ),
    ]
    .into();
    let t = classify(&Input {
        files: &l.files,
        keys: &l.keys,
        similarity: &l.similarity,
        formatting_only: Some(&formatting),
        rated: Some(&rated),
        ignored: Some(&ignored),
        round: 1,
    });
    let get = |k| {
        let i = item(&t, k);
        (i.group, i.reason.text.as_str(), i.why.as_deref())
    };
    assert_eq!(get("src/Big.cs"), (Group::Huge, "huge (20001 diff lines)", None));
    assert_eq!(get("src/Report.cs"), (Group::Review, "heuristic: churn (400 lines)", None));
    assert_eq!(
        get("src/Util/Strings.cs"),
        (Group::FormattingOnly, "formatting-only (-w empty)", None)
    );
    assert_eq!(get("crates/x/src/parse.rs"), (Group::Ignored, "ignored (overview)", None));
    assert_eq!(
        get("src/Services/Totals.cs"),
        (Group::Must, "rated critical (overview)", Some("Totals feed every account"))
    );
    assert_eq!(
        get("src/Services/VoucherService.cs"),
        (Group::Must, "money path (Voucher)", None),
        "a low rating does not lower a rule's floor"
    );
    assert_eq!(
        get("src/Gen/Client.g.cs"),
        (Group::Generated, "generated (path rule)", Some("regenerated client"))
    );
    assert_eq!(
        &keys(&t)[..3],
        [
            "src/Services/Totals.cs",
            "src/Services/VoucherService.cs",
            "src/Api/PaymentsController.cs"
        ],
        "cited Must first, then by path"
    );
    assert_eq!(t.facts.uncited, Some(4));
    assert_eq!(t.items.last().unwrap().key, "crates/x/src/parse.rs");
}

#[test]
fn shapes() {
    let tiny = run(&load(&GROUPS[..GROUPS.find("diff --git a/src/Services/Spacing.cs").unwrap()]));
    assert_eq!((tiny.facts.files, tiny.shape), (2, Shape::Tiny));
    let l = load(GROUPS);
    let again = classify(&Input {
        files: &l.files,
        keys: &l.keys,
        round: 2,
        ..Input::default()
    });
    assert_eq!(again.shape, Shape::Iterative);
}

#[test]
fn m_fixture_moves_tests_and_heuristics() {
    let l = load(M);
    let t = run(&l);
    assert_eq!(t.items.len(), l.files.len());
    let html = item(&t, "app/src/app/customer1/moved/customer1-form.component.html");
    assert_eq!(
        html.moved_from.as_ref().map(|m| m.0.as_str()),
        Some("app/src/app/customer1/customer1-form.component.html")
    );
    let ts = item(&t, "app/src/app/cart2/moved/cart2-form.component.ts");
    assert!(ts.moved_from.is_some() || ts.group == Group::Moves, "{ts:?}");
    let mig = item(&t, "server/src/Acme.Data/Migrations/20260101000000_AddVoucherColumns.cs");
    assert_eq!((mig.group, mig.reason.text.as_str()), (Group::Must, "migration"));
    for k in &l.keys {
        let i = item(&t, k);
        if k.ends_with("Controller.cs") {
            assert_eq!((i.group, i.reason.text.as_str()), (Group::Must, "public API (controller)"), "{k}");
        }
        if k.ends_with(".spec.ts") {
            assert_eq!(i.group, Group::Tests, "{k}");
        }
        if i.group.is_change() && (k.contains("Payment") || k.contains("/voucher")) {
            assert_eq!(i.group, Group::Must, "{k}");
            assert!(
                i.reason.text.starts_with("money path") || i.reason.text.starts_with("public API"),
                "{k}"
            );
        }
    }
    assert_eq!(
        item(&t, "app/src/app/customer2/services/customer2.service.spec.ts").group,
        Group::Tests
    );
    let deleted_specs = l
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Deleted && f.path.ends_with(".spec.ts"))
        .count();
    assert!(deleted_specs > 0);
    assert_eq!(t.facts.tests_removed, deleted_specs);
    let positions: Vec<usize> = t
        .items
        .iter()
        .map(|i| Group::ALL.iter().position(|g| *g == i.group).unwrap())
        .collect();
    assert!(positions.windows(2).all(|w| w[0] <= w[1]), "grouped in list order");
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    for patch in [GROUPS, M] {
        let a = serde_json::to_string(&run(&load(patch))).unwrap();
        let mut l = load(patch);
        l.files.reverse();
        l.keys.reverse();
        l.similarity.reverse();
        let b = serde_json::to_string(&run(&l)).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, serde_json::to_string(&run(&load(patch))).unwrap());
    }
    let t = run(&load(GROUPS));
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json["items"][0]["reason"]["group"], "must");
    assert_eq!(json["groups"][4]["group"], "formatting-only");
    assert_eq!(json["facts"]["testsOff"][0]["marker"], "#[ignore");
    assert!(json["items"][5].get("realLine").is_some());
    let back: Triage = serde_json::from_value(json).unwrap();
    assert_eq!(back, t);
    let tags = t.tags();
    assert_eq!(tags["src/Api/PaymentsController.cs"], rain_core::FileTag::MustReview);
    assert_eq!(tags["package-lock.json"], rain_core::FileTag::Generated);
}

const MARKERS_IN_TEXT: &str = r##"diff --git a/crates/rain-ask/src/prompt.rs b/crates/rain-ask/src/prompt.rs
new file mode 100644
--- /dev/null
+++ b/crates/rain-ask/src/prompt.rs
@@ -0,0 +1,4 @@
+//! The prompt a question becomes.
+pub const PREAMBLE: &str = "You are answering a code-review question. \
+Cite code as path:line. Do not edit files. \
+If the answer is not in the excerpt, say so.";
diff --git a/crates/rain-triage/src/rules.rs b/crates/rain-triage/src/rules.rs
--- a/crates/rain-triage/src/rules.rs
+++ b/crates/rain-triage/src/rules.rs
@@ -1,2 +1,7 @@
 fn a() {}
+/// Attribute markers such as `#[ignore` count at the start of a line.
+const ATTRS: [&str; 1] = ["#[ignore"];
+    assert_eq!(test_off("#[ignore = 1]"), Some("#[ignore"));
+#[test]
+#[ignore = "slow"]
 fn b() {}
diff --git a/crates/rain-triage/tests/fixtures/off.rs b/crates/rain-triage/tests/fixtures/off.rs
new file mode 100644
--- /dev/null
+++ b/crates/rain-triage/tests/fixtures/off.rs
@@ -0,0 +1,2 @@
+#[ignore]
+fn c() {}
diff --git a/web/src/form.spec.ts b/web/src/form.spec.ts
--- a/web/src/form.spec.ts
+++ b/web/src/form.spec.ts
@@ -1,1 +1,4 @@
 describe('form', () => {});
+// it.skip('posts') once the API lands
+const name = 'it.skip(';
+/** xit( is how a spec is skipped */
"##;

#[test]
fn markers_inside_strings_comments_and_fixtures_are_ignored() {
    let t = run(&load(MARKERS_IN_TEXT));
    assert_ne!(item(&t, "crates/rain-ask/src/prompt.rs").group, Group::Generated);
    let off: Vec<(&str, u32, &str)> = t
        .facts
        .tests_off
        .iter()
        .map(|o| (o.key.as_str(), o.line, o.marker.as_str()))
        .collect();
    assert_eq!(off, [("crates/rain-triage/src/rules.rs", 6, "#[ignore")]);
}

const HIDING: &str = r##"diff --git a/src/Api/PaymentsController.cs b/src/Api/PaymentsController.cs
--- a/src/Api/PaymentsController.cs
+++ b/src/Api/PaymentsController.cs
@@ -1,2 +1,2 @@
-checkAuth();
-doWork();
+doWork();
+checkAuth();
diff --git a/src/Util/Steps.cs b/src/Util/Steps.cs
--- a/src/Util/Steps.cs
+++ b/src/Util/Steps.cs
@@ -1,2 +1,2 @@
-first();
-second();
+second();
+first();
diff --git a/src/Api/AuthService.cs b/src/Api/AuthService.cs
new file mode 100644
--- /dev/null
+++ b/src/Api/AuthService.cs
@@ -0,0 +1,2 @@
+// Session ids are generated by the server
+class AuthService {}
diff --git a/src/Api/BillingService.cs b/src/Api/BillingService.cs
new file mode 100644
--- /dev/null
+++ b/src/Api/BillingService.cs
@@ -0,0 +1,2 @@
+// <auto-generated />
+class BillingService {}
diff --git a/src/Services/RefundCalc.cs b/src/Services/RefundCalc.cs
--- a/src/Services/RefundCalc.cs
+++ b/src/Services/RefundCalc.cs
@@ -1,1 +1,1 @@
-int  refund = 1;
+int refund = 1;
diff --git a/db/migrations/002_add_test.sql b/db/migrations/002_add_test.sql
new file mode 100644
--- /dev/null
+++ b/db/migrations/002_add_test.sql
@@ -0,0 +1,1 @@
+ALTER TABLE t ADD c int;
diff --git a/app/jobs.py b/app/jobs.py
--- a/app/jobs.py
+++ b/app/jobs.py
@@ -1,2 +1,2 @@
 if ready:
-    run()
+run()
diff --git a/web/cypress/flows/login.cy.ts b/web/cypress/flows/login.cy.ts
--- a/web/cypress/flows/login.cy.ts
+++ b/web/cypress/flows/login.cy.ts
@@ -1,1 +1,2 @@
 describe('login', () => {});
+it.only('signs in', () => {});
"##;

#[test]
fn risky_and_real_changes_are_not_hidden() {
    let l = load(HIDING);
    let formatting: BTreeSet<String> = ["app/jobs.py".to_owned(), "src/Services/RefundCalc.cs".to_owned()].into();
    let ignored: BTreeSet<String> = ["db/migrations/002_add_test.sql".to_owned()].into();
    let t = classify(&Input {
        files: &l.files,
        keys: &l.keys,
        similarity: &l.similarity,
        formatting_only: Some(&formatting),
        ignored: Some(&ignored),
        ..Input::default()
    });
    let get = |k| {
        let i = item(&t, k);
        (i.group, i.reason.text.as_str())
    };
    assert_eq!(get("src/Api/PaymentsController.cs"), (Group::Must, "public API (controller)"));
    assert_eq!(get("src/Util/Steps.cs"), (Group::Review, "change"), "a reorder is real");
    assert_eq!(get("src/Api/AuthService.cs"), (Group::Must, "money path (Auth)"));
    assert_eq!(get("src/Api/BillingService.cs"), (Group::Must, "money path (Billing)"));
    assert!(
        item(&t, "src/Api/BillingService.cs")
            .why
            .as_deref()
            .unwrap()
            .contains("generated (header)")
    );
    assert_eq!(get("src/Services/RefundCalc.cs"), (Group::Must, "money path (Refund)"));
    assert_eq!(get("db/migrations/002_add_test.sql"), (Group::Must, "migration"));
    assert_eq!(get("app/jobs.py"), (Group::Review, "change"), "indentation is Python syntax");
    assert_eq!(get("web/cypress/flows/login.cy.ts"), (Group::Tests, "test"));
    let off: Vec<(&str, &str)> = t.facts.tests_off.iter().map(|o| (o.key.as_str(), o.marker.as_str())).collect();
    assert_eq!(off, [("web/cypress/flows/login.cy.ts", ".only(")]);
}

/// `cargo xtask fixtures --tier l` writes the 300-file fixture (or `RAIN_L_PATCH` names one); skipped without it.
#[test]
#[ignore = "needs fixtures/gen/l"]
fn l_fixture_classifies_fast_and_stably() {
    let path = std::env::var_os("RAIN_L_PATCH").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gen/l/diff.patch"),
        std::path::PathBuf::from,
    );
    let Ok(text) = std::fs::read_to_string(&path) else {
        println!("no {}", path.display());
        return;
    };
    let l = load(&text);
    let t0 = std::time::Instant::now();
    let t = run(&l);
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    println!(
        "l.patch: {} files, shape {:?}, groups {:?}, classify {ms:.1} ms",
        t.items.len(),
        t.shape,
        t.groups.iter().map(|g| (g.group, g.count)).collect::<Vec<_>>()
    );
    assert_eq!(t.items.len(), l.files.len());
    assert_eq!(serde_json::to_string(&t).unwrap(), serde_json::to_string(&run(&l)).unwrap());
    assert!(ms < 100.0, "{ms:.1} ms");
}
