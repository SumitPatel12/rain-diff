//! A synthetic two-repo change for examples, scenes and tests: deterministic, any size.

use std::collections::{BTreeMap, BTreeSet};

use rain_core::{FileDiff, FileStatus, FileTag, RepoInfo};

pub struct Sample {
    pub files: Vec<FileDiff>,
    pub repos: Vec<RepoInfo>,
    pub tags: BTreeMap<String, FileTag>,
    pub viewed: BTreeSet<String>,
    pub comments: BTreeMap<String, u32>,
}

const SERVER_DIRS: [&str; 6] = [
    "src/Acme.Api/Controllers",
    "src/Acme.Core/Services/Vouchers",
    "src/Acme.Core/Models",
    "src/Acme.Data/Migrations",
    "src/Acme.Data/Entities",
    "tests/Acme.Specs/Vouchers",
];
const CLIENT_DIRS: [&str; 5] = [
    "src/app/vouchers/components",
    "src/app/vouchers/services",
    "src/app/shared/models",
    "src/app/accounts",
    "src/assets/i18n",
];
const NAMES: [&str; 8] = ["Account", "Voucher", "Payment", "Customer", "Reader", "Edition", "Hold", "Shelf"];

/// `n` files split about 60/40 between `server` and `app`, with the named files
/// `server/src/Acme.Api/Controllers/VouchersController.cs` and
/// `app/src/app/vouchers/services/voucher.service.ts` always present. Every 7th file is viewed,
/// every 5th has comments, tags cycle by index, and migrations are generated.
pub fn sample(n: usize) -> Sample {
    let repos = vec![
        RepoInfo {
            id: "server".into(),
            name: "acme-server".into(),
            ..RepoInfo::default()
        },
        RepoInfo {
            id: "app".into(),
            name: "acme-app".into(),
            ..RepoInfo::default()
        },
    ];
    let mut paths: Vec<(u16, String)> = vec![
        (0, "src/Acme.Api/Controllers/VouchersController.cs".into()),
        (1, "src/app/vouchers/services/voucher.service.ts".into()),
    ];
    let mut i = 0usize;
    while paths.len() < n {
        let name = NAMES[i % NAMES.len()];
        let round = i / NAMES.len();
        if i % 5 < 3 {
            let dir = SERVER_DIRS[i % SERVER_DIRS.len()];
            let file = if dir.ends_with("Migrations") {
                format!("2026110{}_{name}Change{round}.cs", i % 9)
            } else {
                format!("{name}{}{round}.cs", ["Service", "Model", "Controller", "Spec"][i % 4])
            };
            paths.push((0, format!("{dir}/{file}")));
        } else {
            let dir = CLIENT_DIRS[i % CLIENT_DIRS.len()];
            let lower = name.to_lowercase();
            paths.push((
                1,
                format!("{dir}/{lower}-{round}.{}", ["component.ts", "service.ts", "model.ts"][i % 3]),
            ));
        }
        i += 1;
    }
    paths.truncate(n);
    let mut out = Sample {
        files: Vec::with_capacity(n),
        repos,
        tags: BTreeMap::new(),
        viewed: BTreeSet::new(),
        comments: BTreeMap::new(),
    };
    for (i, (repo, path)) in paths.into_iter().enumerate() {
        let generated = path.contains("/Migrations/");
        let status = match i % 11 {
            3 => FileStatus::Added,
            7 => FileStatus::Deleted,
            9 => FileStatus::Renamed,
            _ => FileStatus::Modified,
        };
        let key = format!("{}/{path}", out.repos[repo as usize].id);
        let tag = if generated {
            FileTag::Generated
        } else if i < 2 {
            FileTag::MustReview
        } else {
            [
                FileTag::MustReview,
                FileTag::Review,
                FileTag::Review,
                FileTag::Skim,
                FileTag::Skim,
                FileTag::FormattingOnly,
                FileTag::Uncovered,
                FileTag::Review,
                FileTag::Ignored,
            ][i % 9]
        };
        out.tags.insert(key.clone(), tag);
        if i % 7 == 6 {
            out.viewed.insert(key.clone());
        }
        if i % 5 == 4 {
            out.comments.insert(key.clone(), (i % 3 + 1) as u32);
        }
        out.files.push(FileDiff {
            repo,
            old_path: (status == FileStatus::Renamed).then(|| format!("{path}.old")),
            path,
            status,
            generated,
            additions: if status == FileStatus::Deleted {
                0
            } else {
                (i * 7 % 120 + 1) as u32
            },
            deletions: if status == FileStatus::Added { 0 } else { (i * 3 % 40) as u32 },
            ..FileDiff::default()
        });
    }
    out
}
