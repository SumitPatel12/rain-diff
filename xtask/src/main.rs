//! `cargo xtask fixtures --tier s|m|l|xl [--out <dir>]`: builds a deterministic scratch git repo
//! (old commit, new commit) of Acme-shaped files and writes its `diff.patch`.
//! `cargo xtask icon`: see [`icon`].

mod icon;

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("fixtures") => match fixtures(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask fixtures: {e}");
                ExitCode::FAILURE
            }
        },
        Some("icon") => match icon::run(&workspace_root(), &args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask icon: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: cargo xtask fixtures --tier s|m|l|xl [--out <dir>]");
            eprintln!("       cargo xtask icon [--svg <big.svg>] [--small <small.svg>]");
            ExitCode::from(2)
        }
    }
}

type Res<T> = Result<T, String>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn fixtures(args: &[String]) -> Res<()> {
    let mut tier = None;
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--tier" => tier = it.next().cloned(),
            "--out" => out = it.next().map(PathBuf::from),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let tier = tier.ok_or("--tier is required")?;
    let tier = Tier::parse(&tier).ok_or_else(|| format!("unknown tier {tier}"))?;
    let root = workspace_root();
    let out = out.unwrap_or_else(|| root.join("fixtures").join("gen").join(tier.name()));
    let started = Instant::now();
    let (old, new) = build_tier(tier);
    let repo = out.join("repo");
    write_repo(&repo, &old, &new)?;
    let patch = out.join("diff.patch");
    git_diff(&repo, &patch)?;
    let stats = PatchStats::of(&std::fs::read(&patch).map_err(|e| e.to_string())?);
    if matches!(tier, Tier::S | Tier::M) {
        let dest = root.join("fixtures").join(format!("{}.patch", tier.name()));
        std::fs::copy(&patch, &dest).map_err(|e| format!("copy to {}: {e}", dest.display()))?;
    }
    println!(
        "tier {}: {} files, {} diff lines (+{} -{} ctx {}), {} hunks, {:.1} MB patch, {:.1} s -> {}",
        tier.name(),
        stats.files,
        stats.lines(),
        stats.added,
        stats.removed,
        stats.context,
        stats.hunks,
        stats.bytes as f64 / 1e6,
        started.elapsed().as_secs_f64(),
        patch.display()
    );
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tier {
    S,
    M,
    L,
    Xl,
}

impl Tier {
    fn parse(s: &str) -> Option<Tier> {
        Some(match s {
            "s" => Tier::S,
            "m" => Tier::M,
            "l" => Tier::L,
            "xl" => Tier::Xl,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Tier::S => "s",
            Tier::M => "m",
            Tier::L => "l",
            Tier::Xl => "xl",
        }
    }
}

// ---------------------------------------------------------------- deterministic random

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }

    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

// ---------------------------------------------------------------- file model

#[derive(Clone)]
enum Content {
    Text {
        lines: Vec<String>,
        crlf: bool,
        final_newline: bool,
    },
    Binary(Vec<u8>),
}

impl Content {
    fn text(lines: Vec<String>) -> Content {
        Content::Text {
            lines,
            crlf: false,
            final_newline: true,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Content::Binary(b) => b.clone(),
            Content::Text {
                lines,
                crlf,
                final_newline,
            } => {
                let eol = if *crlf { "\r\n" } else { "\n" };
                let mut s = String::with_capacity(lines.iter().map(|l| l.len() + 2).sum());
                for (i, l) in lines.iter().enumerate() {
                    s.push_str(l);
                    if i + 1 < lines.len() || *final_newline {
                        s.push_str(eol);
                    }
                }
                s.into_bytes()
            }
        }
    }
}

type Tree = Vec<(String, Content)>;

const ENTITIES: [&str; 16] = [
    "Hold", "Customer", "Account", "Voucher", "Reader", "Cart", "Edition", "Format", "Payment", "Store", "Wishlist", "Preorder", "Shelf",
    "Author", "Document", "Event",
];
const VERBS: [&str; 10] = [
    "Get",
    "Create",
    "Update",
    "Delete",
    "Recalculate",
    "Sync",
    "Validate",
    "Assign",
    "Schedule",
    "Export",
];
const TYPES: [(&str, &str); 8] = [
    ("Guid", "uniqueidentifier"),
    ("string", "nvarchar(200)"),
    ("int", "int"),
    ("decimal", "decimal(18,2)"),
    ("DateTime", "datetime2"),
    ("bool", "bit"),
    ("long", "bigint"),
    ("DateTimeOffset", "datetimeoffset"),
];
const PROPS: [&str; 16] = [
    "Name",
    "TenantId",
    "StoreId",
    "CreatedAt",
    "UpdatedAt",
    "Amount",
    "Status",
    "Notes",
    "IsDeleted",
    "SortOrder",
    "ExternalId",
    "OwnerId",
    "Total",
    "Description",
    "ScheduledOn",
    "Version",
];

fn kebab(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn cs_method(rng: &mut Rng, entity: &str, n: usize) -> Vec<String> {
    let verb = rng.pick(&VERBS);
    let mut v = vec![
        format!("        public async Task<{entity}ViewModel> {verb}{entity}{n}Async(Guid id, CancellationToken cancellationToken)"),
        "        {".into(),
        format!("            var entity = await _db.{entity}s.SingleOrDefaultAsync(x => x.Id == id, cancellationToken);"),
        "            if (entity == null)".into(),
        "            {".into(),
        format!("                throw new NotFoundException(nameof({entity}), id);"),
        "            }".into(),
    ];
    for _ in 0..rng.range(1, 6) {
        let p = rng.pick(&PROPS);
        v.push(format!("            entity.{p} = request.{p} ?? entity.{p};"));
    }
    v.push("            await _db.SaveChangesAsync(cancellationToken);".into());
    v.push(format!("            return _mapper.Map<{entity}ViewModel>(entity);"));
    v.push("        }".into());
    v.push(String::new());
    v
}

fn cs_class(rng: &mut Rng, entity: &str, suffix: &str, methods: usize) -> Vec<String> {
    let ns = if suffix == "Controller" {
        "Acme.Api.Controllers"
    } else {
        "Acme.Services"
    };
    let mut v: Vec<String> = [
        "using System;",
        "using System.Threading;",
        "using System.Threading.Tasks;",
        "using Microsoft.EntityFrameworkCore;",
        "using Acme.Data;",
        "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(format!("namespace {ns}"));
    v.push("{".into());
    v.push(format!("    public class {entity}{suffix} : I{entity}{suffix}"));
    v.push("    {".into());
    v.push("        private readonly AppDbContext _db;".into());
    v.push("        private readonly IMapper _mapper;".into());
    v.push(String::new());
    for i in 0..methods {
        v.extend(cs_method(rng, entity, i));
    }
    v.push("    }".into());
    v.push("}".into());
    v
}

fn ts_component(rng: &mut Rng, entity: &str, part: &str, methods: usize) -> Vec<String> {
    let k = kebab(entity);
    let mut v = vec![
        "import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';".to_string(),
        format!("import {{ {entity}Service }} from '../services/{k}.service';"),
        String::new(),
        "@Component({".into(),
        format!("  selector: 'app-{k}-{part}',"),
        format!("  templateUrl: './{k}-{part}.component.html',"),
        format!("  styleUrl: './{k}-{part}.component.scss',"),
        "  changeDetection: ChangeDetectionStrategy.OnPush,".into(),
        "})".into(),
        format!("export class {entity}{}Component {{", capitalize(part)),
        format!("  private readonly service = inject({entity}Service);"),
        format!("  readonly {}Id = input.required<string>();", entity.to_lowercase()),
        "  readonly saved = output<void>();".into(),
        String::new(),
    ];
    for i in 0..methods {
        let verb = rng.pick(&VERBS).to_lowercase();
        v.push(format!("  {verb}{entity}{i}(): void {{"));
        for _ in 0..rng.range(2, 6) {
            let p = rng.pick(&PROPS);
            v.push(format!(
                "    this.form.controls.{}.setValue(this.model.{});",
                p.to_lowercase(),
                p.to_lowercase()
            ));
        }
        v.push(format!(
            "    this.service.{verb}(this.{}Id()).subscribe(() => this.saved.emit());",
            entity.to_lowercase()
        ));
        v.push("  }".into());
        v.push(String::new());
    }
    v.push("}".into());
    v
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
        .unwrap_or_default()
}

fn html_template(rng: &mut Rng, entity: &str, rows: usize) -> Vec<String> {
    let mut v = vec![
        format!("<section class=\"app-{}\">", kebab(entity)),
        "  <form [formGroup]=\"form\">".into(),
    ];
    for _ in 0..rows {
        let p = rng.pick(&PROPS).to_lowercase();
        v.push("    <div class=\"app-field\">".into());
        v.push(format!(
            "      <label for=\"{p}\">{{{{ '{}.{p}' | translate }}}}</label>",
            entity.to_lowercase()
        ));
        v.push(format!("      <input id=\"{p}\" formControlName=\"{p}\" />"));
        v.push("    </div>".into());
    }
    v.push("  </form>".into());
    v.push("</section>".into());
    v
}

fn scss(rng: &mut Rng, entity: &str, blocks: usize) -> Vec<String> {
    let mut v = vec!["@use 'variables' as *;".to_string(), String::new()];
    for i in 0..blocks {
        v.push(format!(".app-{}__part-{i} {{", kebab(entity)));
        v.push(format!("  padding: {}rem;", rng.range(1, 4) as f32 * 0.25));
        v.push(format!("  gap: {}rem;", rng.range(1, 4) as f32 * 0.5));
        v.push("  color: $text-base;".into());
        v.push("}".into());
        v.push(String::new());
    }
    v
}

fn spec_ts(rng: &mut Rng, entity: &str, cases: usize) -> Vec<String> {
    let k = kebab(entity);
    let mut v = vec![
        "import { TestBed } from '@angular/core/testing';".to_string(),
        format!("import {{ {entity}Service }} from './{k}.service';"),
        String::new(),
        format!("describe('{entity}Service', () => {{"),
    ];
    for i in 0..cases {
        let verb = rng.pick(&VERBS).to_lowercase();
        v.push(format!("  it('{verb}s the {k} ({i})', () => {{"));
        v.push(format!("    const service = TestBed.inject({entity}Service);"));
        v.push(format!("    expect(service.{verb}).toBeDefined();"));
        v.push("  });".into());
        v.push(String::new());
    }
    v.push("});".into());
    v
}

fn json_i18n(rng: &mut Rng, entity: &str, keys: usize) -> Vec<String> {
    let mut v = vec!["{".to_string(), format!("  \"{}\": {{", entity.to_lowercase())];
    for i in 0..keys {
        let p = rng.pick(&PROPS);
        let comma = if i + 1 < keys { "," } else { "" };
        v.push(format!("    \"{}{i}\": \"{p} {i}\"{comma}", p.to_lowercase()));
    }
    v.push("  }".into());
    v.push("}".into());
    v
}

fn migration(rng: &mut Rng, entity: &str, columns: usize) -> Vec<String> {
    let mut v = vec![
        "using Microsoft.EntityFrameworkCore.Migrations;".to_string(),
        String::new(),
        "namespace Acme.Data.Migrations".into(),
        "{".into(),
        format!("    public partial class Add{entity}Columns : Migration"),
        "    {".into(),
        "        protected override void Up(MigrationBuilder migrationBuilder)".into(),
        "        {".into(),
    ];
    for i in 0..columns {
        let (ty, sql) = rng.pick(&TYPES);
        v.push("            migrationBuilder.AddColumn<".to_string() + ty + ">(");
        v.push(format!("                name: \"{}{i}\",", rng.pick(&PROPS)));
        v.push(format!("                table: \"{entity}s\","));
        v.push(format!("                type: \"{sql}\","));
        v.push("                nullable: true);".into());
        v.push(String::new());
    }
    v.push("        }".into());
    v.push("    }".into());
    v.push("}".into());
    v
}

/// One EF Core `modelBuilder.Entity(...)` block, ~40 lines.
fn snapshot_entity(rng: &mut Rng, ns: &str, n: usize, indent: &str, blank: &str, out: &mut Vec<String>) {
    out.push(format!("{indent}modelBuilder.Entity(\"{ns}.Entity{n}\", b =>"));
    out.push(format!("{indent}    {{"));
    out.push(format!("{indent}        b.Property<Guid>(\"Id\")"));
    out.push(format!("{indent}            .ValueGeneratedOnAdd()"));
    out.push(format!("{indent}            .HasColumnType(\"uniqueidentifier\");"));
    out.push(blank.to_owned());
    for p in 0..6 {
        let (ty, sql) = TYPES[(n + p + rng.below(3)) % TYPES.len()];
        let name = PROPS[(n * 7 + p) % PROPS.len()];
        out.push(format!("{indent}        b.Property<{ty}>(\"{name}{p}\")"));
        if ty == "string" {
            out.push(format!("{indent}            .IsRequired()"));
            out.push(format!("{indent}            .HasMaxLength(200)"));
        }
        out.push(format!("{indent}            .HasColumnType(\"{sql}\");"));
        out.push(blank.to_owned());
    }
    out.push(format!("{indent}        b.HasKey(\"Id\");"));
    out.push(blank.to_owned());
    out.push(format!("{indent}        b.HasIndex(\"TenantId{}\");", n % 6));
    out.push(blank.to_owned());
    out.push(format!("{indent}        b.ToTable(\"Entity{n}\", (string)null);"));
    out.push(format!("{indent}    }});"));
    out.push(blank.to_owned());
}

/// An `AppDbContextModelSnapshot` of at least `min_lines` lines.
fn snapshot(seed: u64, ns: &str, class: &str, min_lines: usize, indent_unit: &str, indent_blanks: bool) -> Vec<String> {
    let mut rng = Rng(seed);
    let i2 = indent_unit.repeat(2);
    let i3 = indent_unit.repeat(3);
    let mut v = vec![
        "// <auto-generated />".to_string(),
        "using System;".into(),
        "using Microsoft.EntityFrameworkCore;".into(),
        "using Microsoft.EntityFrameworkCore.Infrastructure;".into(),
        String::new(),
        "#nullable disable".into(),
        String::new(),
        format!("namespace {ns}.Migrations"),
        "{".into(),
        format!("{indent_unit}[DbContext(typeof(AppDbContext))]"),
        format!("{indent_unit}partial class {class} : ModelSnapshot"),
        format!("{indent_unit}{{"),
        format!("{i2}protected override void BuildModel(ModelBuilder modelBuilder)"),
        format!("{i2}{{"),
    ];
    let blank = if indent_blanks { i3.as_str() } else { "" };
    let mut n = 0;
    while v.len() + 4 < min_lines {
        snapshot_entity(&mut rng, &format!("{ns}.Domain"), n, &i3, blank, &mut v);
        n += 1;
    }
    v.push(format!("{i2}}}"));
    v.push(format!("{indent_unit}}}"));
    v.push("}".into());
    v
}

/// Applies `edits` random modifications, insertions and deletions.
fn mutate(rng: &mut Rng, lines: &[String], edits: usize, block: &mut dyn FnMut(&mut Rng) -> Vec<String>) -> Vec<String> {
    let mut out = lines.to_vec();
    let mut at: Vec<usize> = (0..edits).map(|_| rng.below(out.len().max(1))).collect();
    at.sort_unstable_by(|a, b| b.cmp(a));
    for i in at {
        if i >= out.len() {
            continue;
        }
        match rng.below(10) {
            0..=4 => out[i] = tweak(rng, &out[i]),
            5..=7 => {
                let b = block(rng);
                out.splice(i..i, b);
            }
            _ => {
                let n = rng.range(1, 6).min(out.len() - i);
                out.drain(i..i + n);
            }
        }
    }
    out
}

fn tweak(rng: &mut Rng, line: &str) -> String {
    if let Some(p) = line.find('(')
        && rng.chance(50)
        && !line[p + 1..].starts_with(')')
    {
        return format!("{}(options, {}", &line[..p], &line[p + 1..]);
    }
    if let Some(d) = line.find(|c: char| c.is_ascii_digit()) {
        let mut s = line.to_owned();
        s.replace_range(d..d + 1, &((line.as_bytes()[d] - b'0' + 1) % 10).to_string());
        return s;
    }
    if line.trim().is_empty() {
        return "        // reviewed".to_owned();
    }
    let prop = rng.pick(&PROPS);
    format!("{line} // {prop} kept for compatibility")
}

fn long_line(rng: &mut Rng, prefix: &str, min: usize) -> String {
    let mut s = String::from(prefix);
    while s.len() < min {
        let _ = write!(s, "M{} {}L{} {}Z", rng.below(999), rng.below(999), rng.below(999), rng.below(999));
    }
    s
}

fn png(seed: u64, len: usize) -> Vec<u8> {
    let mut rng = Rng(seed);
    let mut v = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 0];
    v.extend((0..len).map(|_| rng.next() as u8));
    v
}

// ---------------------------------------------------------------- tiers

/// A generated source file of `kind` for entity `e`, scaled by `size`.
type BlockFn = Box<dyn FnMut(&mut Rng) -> Vec<String>>;

fn source(rng: &mut Rng, kind: usize, e: &str, size: usize) -> (String, Vec<String>, BlockFn) {
    let k = kebab(e);
    let ent = e.to_owned();
    match kind % 7 {
        0 => (
            format!("server/src/Acme.Api/Controllers/{e}Controller.cs"),
            cs_class(rng, e, "Controller", size),
            Box::new(move |r| {
                let n = 90 + r.below(9);
                cs_method(r, &ent, n)
            }),
        ),
        1 => (
            format!("server/src/Acme.Services/{e}/{e}Service.cs"),
            cs_class(rng, e, "Service", size),
            Box::new(move |r| {
                let n = 90 + r.below(9);
                cs_method(r, &ent, n)
            }),
        ),
        2 => (
            format!("app/src/app/{k}/{k}-form.component.ts"),
            ts_component(rng, e, "form", size),
            Box::new(move |r| ts_component(r, &ent, "form", 1)[14..].to_vec()),
        ),
        3 => (
            format!("app/src/app/{k}/{k}-form.component.html"),
            html_template(rng, e, size * 2),
            Box::new(move |r| html_template(r, &ent, 2)[2..10].to_vec()),
        ),
        4 => (
            format!("app/src/app/{k}/{k}-form.component.scss"),
            scss(rng, e, size * 2),
            Box::new(move |r| scss(r, &ent, 2)),
        ),
        5 => (
            format!("app/src/app/{k}/services/{k}.service.spec.ts"),
            spec_ts(rng, e, size * 2),
            Box::new(move |r| spec_ts(r, &ent, 2)[4..14].to_vec()),
        ),
        _ => (
            format!("app/src/assets/i18n/{k}.json"),
            json_i18n(rng, e, size * 4),
            Box::new(move |r| (0..4).map(|i| format!("    \"extra{}\": \"{}\",", r.below(999), i)).collect()),
        ),
    }
}

fn tier_s() -> (Tree, Tree) {
    let mut rng = Rng(1);
    let cs = cs_class(&mut rng, "Voucher", "Service", 4);
    let cs_new = mutate(&mut rng, &cs, 4, &mut |r| cs_method(r, "Voucher", 50));
    let ts = ts_component(&mut rng, "Voucher", "form", 3);
    let mut ts_new = ts.clone();
    ts_new[1] = "import { VouchersService } from '../services/vouchers.service';".into();
    let html = html_template(&mut rng, "LegacyVoucher", 4);
    let scss_old = scss(&mut rng, "Voucher", 4);
    let scss_new = mutate(&mut rng, &scss_old, 3, &mut |r| scss(r, "Voucher", 1));
    let json = json_i18n(&mut rng, "Voucher", 12);
    let mut json_new = json.clone();
    json_new[3] = format!("    \"icon\": \"{}\",", long_line(&mut rng, "data:image/svg+xml;utf8,", 2_100));
    json_new[6] = "    \"amount\": \"Voucher amount\",".into();
    let old = vec![
        (
            "server/src/Services/VoucherService.cs".into(),
            Content::Text {
                lines: cs,
                crlf: true,
                final_newline: true,
            },
        ),
        ("app/src/app/vouchers/voucher-form.component.ts".into(), Content::text(ts)),
        ("app/src/app/vouchers/legacy-voucher.component.html".into(), Content::text(html)),
        ("app/src/app/vouchers/voucher-form.component.scss".into(), Content::text(scss_old)),
        ("app/src/assets/i18n/en.json".into(), Content::text(json)),
        ("app/src/assets/voucher-icon.png".into(), Content::Binary(png(10, 300))),
    ];
    let new = vec![
        (
            "server/src/Services/VoucherService.cs".into(),
            Content::Text {
                lines: cs_new,
                crlf: true,
                final_newline: true,
            },
        ),
        ("app/src/app/vouchers/vouchers-form.component.ts".into(), Content::text(ts_new)),
        ("app/src/app/vouchers/voucher-form.component.scss".into(), Content::text(scss_new)),
        (
            "app/src/assets/i18n/en.json".into(),
            Content::Text {
                lines: json_new,
                crlf: false,
                final_newline: false,
            },
        ),
        ("app/src/assets/voucher-icon.png".into(), Content::Binary(png(11, 320))),
    ];
    (old, new)
}

/// `count` files of mixed kinds; `size` scales file length, `edits` the changes per file.
fn mixed(seed: u64, count: usize, size: usize, edits: usize, specials: bool) -> (Tree, Tree) {
    let mut rng = Rng(seed);
    let (mut old, mut new) = (Tree::new(), Tree::new());
    for i in 0..count {
        let e = format!("{}{}", ENTITIES[i % ENTITIES.len()], i / ENTITIES.len());
        let sized = size + rng.below(size + 1);
        let (path, lines, mut block) = source(&mut rng, i, &e, sized);
        let crlf = specials && i % 15 == 3;
        let wrap = |lines: Vec<String>| Content::Text {
            lines,
            crlf,
            final_newline: true,
        };
        match i % 20 {
            7 => new.push((path, wrap(lines))),
            13 => old.push((path, wrap(lines))),
            17 => {
                let changed = mutate(&mut rng, &lines, 1, &mut block);
                let renamed = match path.rsplit_once('/') {
                    Some((dir, file)) => format!("{dir}/moved/{file}"),
                    None => format!("moved/{path}"),
                };
                old.push((path, wrap(lines)));
                new.push((renamed, wrap(changed)));
            }
            _ => {
                let n = edits + rng.below(edits + 1);
                let mut changed = mutate(&mut rng, &lines, n, &mut block);
                if specials && i % 30 == 11 && !changed.is_empty() {
                    let at = rng.below(changed.len());
                    changed[at] = long_line(&mut rng, "    // path: ", 2_048);
                }
                old.push((path.clone(), wrap(lines)));
                new.push((path, wrap(changed)));
            }
        }
    }
    (old, new)
}

fn tier_m() -> (Tree, Tree) {
    let (mut old, mut new) = mixed(2, 38, 8, 12, false);
    let mut rng = Rng(3);
    new.push((
        "server/src/Acme.Data/Migrations/20260101000000_AddVoucherColumns.cs".into(),
        Content::text(migration(&mut rng, "Voucher", 12)),
    ));
    let zh_old = vec![
        "{".to_string(),
        "  \"voucher\": \"礼券\",".into(),
        "  \"thanks\": \"谢谢 🙏\"".into(),
        "}".into(),
    ];
    let zh_new = vec![
        "{".to_string(),
        "  \"voucher\": \"礼券 💵\",".into(),
        "  \"thanks\": \"非常感谢 🙏\",".into(),
        "  \"reader\": \"读者团队 📚\"".into(),
        "}".into(),
    ];
    old.push(("app/src/assets/i18n/礼券.json".into(), Content::text(zh_old)));
    new.push(("app/src/assets/i18n/礼券.json".into(), Content::text(zh_new)));
    (old, new)
}

fn tier_l() -> (Tree, Tree) {
    let (mut old, mut new) = mixed(4, 301, 9, 14, true);
    let mut rng = Rng(5);
    let cjk_old: Vec<String> = (0..60).map(|i| format!("  \"key{i}\": \"书店 {i} — 顧客 ✓\",")).collect();
    let mut cjk_new = cjk_old.clone();
    for i in (0..60).step_by(4) {
        cjk_new[i] = format!("  \"key{i}\": \"书店 {i} — 顧客 ✓ 🎉📚 古本屋 서점\",");
    }
    let wrap = |mut v: Vec<String>| {
        v.insert(0, "{".into());
        v.push("  \"end\": \"終\"".into());
        v.push("}".into());
        Content::text(v)
    };
    old.push(("app/src/assets/i18n/zh-cn.json".into(), wrap(cjk_old)));
    new.push(("app/src/assets/i18n/zh-cn.json".into(), wrap(cjk_new)));
    let snap = snapshot(6, "Acme", "AppDbContextModelSnapshot", 120_000, "    ", false);
    let mut snap_new = snap.clone();
    let mut edited = 0;
    while edited < 2_000 {
        let i = rng.below(snap_new.len());
        if snap_new[i].contains("HasMaxLength(200)") {
            snap_new[i] = snap_new[i].replace("HasMaxLength(200)", "HasMaxLength(256)");
            edited += 1;
        } else if snap_new[i].contains(".HasColumnType(\"int\")") {
            snap_new[i] = snap_new[i].replace(".HasColumnType(\"int\")", ".HasColumnType(\"bigint\")");
            edited += 1;
        }
    }
    let path = "server/src/Acme.Data/Migrations/AppDbContextModelSnapshot.cs".to_string();
    old.push((path.clone(), Content::text(snap)));
    new.push((path, Content::text(snap_new)));
    (old, new)
}

fn tier_xl() -> (Tree, Tree) {
    let (mut old, mut new) = tier_l();
    let path = "Migrations/AppDbContextModelSnapshot.cs".to_string();
    old.push((
        path.clone(),
        Content::text(snapshot(7, "Acme", "AppDbContextModelSnapshot", 500_000, "    ", false)),
    ));
    new.push((
        path,
        Content::text(snapshot(7, "Acme.Core", "AppDbContextModelSnapshot", 500_000, "\t", true)),
    ));
    let added = snapshot(8, "Acme.Reporting", "ReportingDbContextModelSnapshot", 300_000, "    ", false);
    new.push(("Migrations/AppDbContextModelSnapshot2.cs".into(), Content::text(added)));
    (old, new)
}

fn build_tier(t: Tier) -> (Tree, Tree) {
    match t {
        Tier::S => tier_s(),
        Tier::M => tier_m(),
        Tier::L => tier_l(),
        Tier::Xl => tier_xl(),
    }
}

// ---------------------------------------------------------------- git

fn git(repo: &Path, args: &[&str]) -> Res<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

fn write_tree(repo: &Path, tree: &Tree) -> Res<()> {
    for (path, content) in tree {
        let p = repo.join(path);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut f = std::io::BufWriter::new(std::fs::File::create(&p).map_err(|e| format!("{}: {e}", p.display()))?);
        f.write_all(&content.bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn clear_worktree(repo: &Path) -> Res<()> {
    for entry in std::fs::read_dir(repo).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name() == ".git" {
            continue;
        }
        let p = entry.path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        }
        .map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

fn write_repo(repo: &Path, old: &Tree, new: &Tree) -> Res<()> {
    if repo.exists() {
        std::fs::remove_dir_all(repo).map_err(|e| format!("remove {}: {e}", repo.display()))?;
    }
    std::fs::create_dir_all(repo).map_err(|e| e.to_string())?;
    git(repo, &["init", "-q", "-b", "main"])?;
    for (k, v) in [
        ("core.autocrlf", "false"),
        ("core.safecrlf", "false"),
        ("user.name", "rain fixtures"),
        ("user.email", "fixtures@rain.invalid"),
        ("commit.gpgsign", "false"),
        ("gc.auto", "0"),
    ] {
        git(repo, &["config", k, v])?;
    }
    write_tree(repo, old)?;
    git(repo, &["add", "-A", "-f", "."])?;
    git(repo, &["commit", "-q", "--no-verify", "-m", "old"])?;
    clear_worktree(repo)?;
    write_tree(repo, new)?;
    git(repo, &["add", "-A", "-f", "."])?;
    git(repo, &["commit", "-q", "--no-verify", "-m", "new"])
}

fn git_diff(repo: &Path, patch: &Path) -> Res<()> {
    let file = std::fs::File::create(patch).map_err(|e| e.to_string())?;
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "--no-pager",
            "diff",
            "--no-ext-diff",
            "--color=never",
            "--find-renames",
            "HEAD~1",
            "HEAD",
        ])
        .stdout(file)
        .status()
        .map_err(|e| format!("git diff: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("git diff exited {status}"))
    }
}

#[derive(Default)]
struct PatchStats {
    files: usize,
    hunks: usize,
    added: usize,
    removed: usize,
    context: usize,
    bytes: usize,
}

impl PatchStats {
    fn of(patch: &[u8]) -> PatchStats {
        let mut s = PatchStats {
            bytes: patch.len(),
            ..PatchStats::default()
        };
        let mut in_hunk = false;
        for line in patch.split(|&b| b == b'\n') {
            if line.starts_with(b"diff --git ") {
                s.files += 1;
                in_hunk = false;
            } else if line.starts_with(b"@@ ") {
                s.hunks += 1;
                in_hunk = true;
            } else if in_hunk {
                match line.first() {
                    Some(b'+') => s.added += 1,
                    Some(b'-') => s.removed += 1,
                    Some(b' ') => s.context += 1,
                    _ => {}
                }
            }
        }
        s
    }

    fn lines(&self) -> usize {
        self.added + self.removed + self.context
    }
}
