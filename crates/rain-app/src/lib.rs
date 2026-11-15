//! The Rain window: a story's diff across its repos with the list beside the reader, comment
//! threads, overlays (Map, Files, Brief), zoom, a keymap and a command palette.
//!
//! `run` opens it for a [`Launch`]; the loader thread does the git work, so the first frame
//! shows a loading state and the diff arrives through `poll`.

mod app;
mod icon;
mod keys;
mod load;
mod palette;
mod peek_window;
mod render;
mod settings;
pub mod theme;
mod view;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use rain_core::{Launch, RepoLaunch, Shared};
use rune::tok::TextStyle;
use rune::{AppSpec, Tx};

pub use app::{App, ComposerState, LayoutSnapshot, LayoutState, Overlay, Phase};
pub use keys::{command_of, on_key};
pub use load::LARGE_FILE_LINES;
pub use peek_window::PeekWindow;
pub use render::render_overview;
pub use settings::{Chrome, CodeSize, Density, Settings, Theme, ViewLayout};
pub use view::view;

/// The installed JetBrains Mono Nerd Font's family name.
pub const MONO_FAMILY: &str = "JetBrainsMono Nerd Font Mono";
pub const CODE: TextStyle = TextStyle::new(13.0, 400.0).mono();

const MONO_FILES: [&str; 3] = [
    "JetBrainsMonoNerdFontMono-Regular.ttf",
    "JetBrainsMonoNerdFontMono-Medium.ttf",
    "JetBrainsMonoNerdFontMono-Bold.ttf",
];
const SANS_FILES: [&str; 3] = ["segoeui.ttf", "seguisb.ttf", "segoeuib.ttf"];

/// Font files and the (sans, mono) family lists for `AppSpec`. Uses the installed JetBrains
/// Mono and Segoe UI when both are found, else Rune's built-in Inter with the system mono.
pub fn fonts() -> (&'static [&'static [u8]], (&'static str, &'static str)) {
    static FONTS: OnceLock<(Vec<&'static [u8]>, bool)> = OnceLock::new();
    let (files, found) = FONTS.get_or_init(load_fonts);
    if *found {
        (files.as_slice(), ("Segoe UI", "JetBrainsMono Nerd Font Mono, ui-monospace"))
    } else {
        (&[], ("Inter", "ui-monospace, Inter"))
    }
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
    }
    if let Some(win) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(win).join("Fonts"));
    }
    dirs
}

fn read_font(name: &str) -> Option<&'static [u8]> {
    let bytes = font_dirs().into_iter().find_map(|d| std::fs::read(d.join(name)).ok())?;
    Some(Box::leak(bytes.into_boxed_slice()))
}

fn load_fonts() -> (Vec<&'static [u8]>, bool) {
    let mono: Vec<_> = MONO_FILES.iter().filter_map(|f| read_font(f)).collect();
    let sans: Vec<_> = SANS_FILES.iter().filter_map(|f| read_font(f)).collect();
    if mono.is_empty() || sans.is_empty() {
        return (Vec::new(), false);
    }
    (sans.into_iter().chain(mono).collect(), true)
}

static START: OnceLock<(Launch, Arc<Shared>, Instant)> = OnceLock::new();

/// `--fixture`, `--overview`, `--session`, `--goto`, `--comments`, `--repo` (repeatable, with
/// `--base`/`--target`/`--merge-base` applying to all), `--clean`, `-w`, `--chrome`: read from the arguments
/// Rune did not take, for runs where no CLI built the [`Launch`] (scenes, `cargo run -p rain`).
pub fn launch_from_args(args: &[String]) -> Launch {
    let mut l = Launch::default();
    let (mut base, mut target, mut merge_base) = (None::<String>, None::<String>, false);
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| {
            *i += 1;
            args.get(*i).cloned().unwrap_or_default()
        };
        match args[i].as_str() {
            "--fixture" => l.fixture = Some(next(&mut i).into()),
            "--overview" => l.overview = Some(next(&mut i).into()),
            "--session" => l.session = next(&mut i),
            "--goto" => l.goto = Some(next(&mut i)),
            "--comments" => l.comments.push(next(&mut i).into()),
            "--repo" => dirs.push(next(&mut i).into()),
            "--base" => base = Some(next(&mut i)),
            "--target" => target = Some(next(&mut i)),
            "--merge-base" => merge_base = true,
            "--clean" => l.clean = true,
            "-w" | "--ignore-whitespace" => l.ignore_whitespace = true,
            "--chrome" => l.chrome = Some(next(&mut i)),
            _ => {}
        }
        i += 1;
    }
    if dirs.is_empty() && (base.is_some() || target.is_some()) {
        dirs.push(".".into());
    }
    l.repos = dirs
        .into_iter()
        .map(|dir| {
            let d = RepoLaunch::default();
            RepoLaunch {
                dir,
                base: base.clone().unwrap_or(d.base),
                target: target.clone().unwrap_or(d.target),
                merge_base,
                include_untracked: d.include_untracked,
            }
        })
        .collect();
    l
}

fn start() -> &'static (Launch, Arc<Shared>, Instant) {
    START.get_or_init(|| (launch_from_args(rune::extra_args()), Arc::new(Shared::default()), Instant::now()))
}

/// `--chrome <zed|islands>` from the process arguments: Rune takes the flag itself, so it never
/// reaches the arguments it leaves.
fn chrome_arg(args: impl IntoIterator<Item = String>) -> Option<String> {
    let mut args = args.into_iter();
    args.by_ref().find(|a| a == "--chrome")?;
    args.next().filter(|v| settings::Chrome::parse(v).is_some())
}

/// The arguments for Rune without Rain's `--chrome <value>`: Rune's own `--chrome` draws macOS
/// traffic lights over headless snaps.
fn rune_argv(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        if a == "--chrome" {
            args.next();
        } else {
            out.push(a);
        }
    }
    out
}

fn init() -> App {
    let (launch, shared, at) = start();
    let mut launch = launch.clone();
    if launch.chrome.is_none() {
        launch.chrome = chrome_arg(std::env::args());
    }
    let mut app = App::new(
        launch,
        shared.clone(),
        Some(rain_comments::SessionFile::dir()),
        Some(Settings::default_path()),
    );
    app.timings.launch = Some(*at);
    app
}

fn setup(s: &mut App, waker: rune::Waker) {
    let w = waker.clone();
    s.shared.set_wake(move || w());
    if s.phase != Phase::Ready {
        s.start_loading(waker);
    }
}

fn poll(s: &mut App, _t: f64, tx: &mut Tx) -> bool {
    s.poll(tx)
}

/// Scenes load on the spot, start from default settings (so `--theme` and the panes don't
/// follow the user's saved ones) and never write the session or settings files.
fn scene_base(s: &mut App) {
    s.autosave = None;
    *s.shared.lock() = rain_core::Session {
        id: "scene".into(),
        ..Default::default()
    };
    s.reset_settings(Settings {
        window: s.settings.window,
        ..Settings::default()
    });
    s.settings.chrome = s.chrome();
    s.settings_path = None;
    s.load_now();
}

/// The `--session` as saved (brief, notes, threads, place) over its real diff, loaded on the spot
/// so a headless render or script sees it from the first frame; writes nothing.
fn scene_session(s: &mut App) {
    s.autosave = None;
    s.settings_path = None;
    s.settings.chrome = s.chrome();
    s.load_now();
}

/// The first must-review file, else the first file.
fn scene_file(s: &App) -> usize {
    s.keys
        .iter()
        .position(|k| s.tags.get(k) == Some(&rain_core::FileTag::MustReview))
        .unwrap_or(0)
}

/// Two threads from "claude" on the first must-review (else first) file, one with replies.
pub fn seed_threads(s: &mut App) {
    use rain_core::{Anchor, Side};
    let pick = scene_file(s);
    let Some(f) = s.diff.files().get(pick) else { return };
    let added: Vec<u32> = f
        .lines
        .iter()
        .filter(|l| l.new != 0 && l.kind == rain_core::LineKind::Added)
        .map(|l| l.new)
        .collect();
    let Some(&first) = added.first() else { return };
    let path = f.path.clone();
    let repo = s.repo_tag(pick).to_owned();
    let third = added.get(2).copied().unwrap_or(first);
    s.mutate(|ss| {
        let a = rain_comments::ops::add_thread(
            ss,
            &repo,
            &path,
            Anchor::Line {
                side: Side::New,
                line: first,
            },
            "F1 issue (blocking): The amount is not validated before it is stored. Reject zero and negative vouchers.",
            Some("claude"),
            None,
        );
        rain_comments::ops::reply(ss, &a, "Good catch, adding a guard.", None);
        rain_comments::ops::reply(ss, &a, "Also cap it at the account total?", Some("claude"));
        rain_comments::ops::add_thread(
            ss,
            &repo,
            &path,
            Anchor::Range {
                side: Side::New,
                start: first,
                end: third,
            },
            "F2 question: Should this run inside the account's unit of work?",
            Some("claude"),
            None,
        );
    });
    if let Some(t) = s.mirror.threads.first() {
        s.expanded.insert(t.id.clone());
    }
}

/// A story note, a file note and a line note from "claude" on the scene file, added the way
/// `POST /api/notes` adds them.
pub fn seed_notes(s: &mut App) {
    use rain_core::{Anchor, Note, NoteKind, Side};
    let pick = scene_file(s);
    let Some(f) = s.diff.files().get(pick) else { return };
    let last_added = f
        .lines
        .iter()
        .rev()
        .find(|l| l.new != 0 && l.kind == rain_core::LineKind::Added)
        .map(|l| l.new);
    let (repo, path) = (s.repo_tag(pick).to_owned(), f.path.clone());
    let note = |kind, anchor, title: Option<&str>, body: &str| Note {
        id: String::new(),
        kind,
        repo: if kind == NoteKind::Story { String::new() } else { repo.clone() },
        file_path: if kind == NoteKind::Story { String::new() } else { path.clone() },
        anchor,
        title: title.map(str::to_owned),
        body: body.into(),
        author: Some("claude".into()),
        created_at: String::new(),
        updated_at: String::new(),
        ..Note::default()
    };
    let mut notes = vec![
        note(
            NoteKind::Story,
            None,
            Some("What this change does"),
            "Adds **vouchers** to accounts end to end.\n\n- The service validates and stores a voucher.\n- The API exposes it; the client form posts it.\n- Formatting-only files can be skimmed.",
        ),
        note(
            NoteKind::File,
            None,
            None,
            "The core of the change: validation lives here, so read it first. Persistence goes through the account's unit of work.",
        ),
    ];
    if let Some(n) = last_added {
        notes.push(note(
            NoteKind::Line,
            Some(Anchor::Line { side: Side::New, line: n }),
            None,
            "Amounts are kept in cents from here on.",
        ));
    }
    s.mutate(|ss| rain_comments::add_notes(ss, notes, "", false));
    s.status.clear();
}

/// Opens the find bar on `query` without taking the keyboard, so its hits mark the ruler.
fn scene_find(s: &mut App, query: &str) {
    s.open_find();
    s.find.edit.focused = false;
    s.find.edit.text = query.into();
    s.run_find();
}

/// Narrows to the first repo with a must-review file (else the first repo) and must-review.
fn scene_filter(s: &mut App) {
    let pick = scene_file(s);
    let mut f = rain_tree::TreeFilter::default();
    if s.repos.len() > 1
        && let Some(file) = s.diff.files().get(pick)
    {
        f.repos.insert(file.repo);
    }
    if s.tags.values().any(|t| *t == rain_core::FileTag::MustReview) {
        f.tags.insert(rain_core::FileTag::MustReview);
    }
    s.apply_filter(&f);
}

/// Opens the scene file whole, reading both blobs on this thread (needs `--repo`).
fn scene_whole(s: &mut App) {
    let g = (0..s.keys.len())
        .find(|&g| s.diff.files()[g].status == rain_core::FileStatus::Modified && s.diff.files()[g].lines.len() < 400)
        .unwrap_or(0);
    s.diff.set_whole(g, true);
    s.fetch_texts(g);
    s.goto_later(s.keys.get(g).cloned().unwrap_or_default());
}

/// The files scene in one chrome, whatever `--chrome` says.
fn scene_chrome(s: &mut App, chrome: settings::Chrome) {
    scene_base(s);
    seed_threads(s);
    s.launch.chrome = None;
    s.settings.chrome = chrome;
}

fn spec() -> AppSpec<App> {
    let (fonts, families) = fonts();
    let size = Settings::load(&Settings::default_path()).window;
    AppSpec {
        scenes: &[
            ("files", |s| {
                scene_base(s);
                seed_threads(s);
            }),
            ("split", |s| {
                scene_base(s);
                s.settings.layout = ViewLayout::Split;
                s.apply_split();
            }),
            ("zoomed", |s| {
                scene_base(s);
                seed_threads(s);
                seed_notes(s);
                s.toggle_zoom();
                let g = scene_file(s);
                if let Some(k) = s.keys.get(g).cloned() {
                    s.goto_later(k);
                }
            }),
            ("composer", |s| {
                scene_base(s);
                s.composer_scene("composer");
            }),
            ("composer-question", |s| {
                scene_base(s);
                s.composer_scene("composer-question");
            }),
            ("thread-states", |s| {
                scene_base(s);
                s.composer_scene("thread-states");
            }),
            ("orphan", |s| {
                scene_base(s);
                s.composer_scene("orphan");
            }),
            ("palette", |s| {
                scene_base(s);
                s.open_palette();
            }),
            ("ruler", |s| {
                scene_base(s);
                seed_threads(s);
                seed_notes(s);
                scene_find(s, "voucher");
                let g = scene_file(s);
                if let Some(k) = s.keys.get(g).cloned() {
                    s.goto_later(k);
                }
            }),
            ("filter", |s| {
                scene_base(s);
                seed_notes(s);
                scene_filter(s);
            }),
            ("wholefile", |s| {
                scene_base(s);
                scene_whole(s);
            }),
            ("notes", |s| {
                scene_base(s);
                seed_notes(s);
                let g = scene_file(s);
                if let Some(k) = s.keys.get(g).cloned() {
                    s.goto_later(k);
                }
            }),
            ("finish", |s| {
                scene_base(s);
                seed_threads(s);
                s.set_viewed(0, true);
                s.open_finish();
            }),
            ("since-round", |s| {
                scene_base(s);
                for g in 0..s.keys.len().min(3) {
                    s.set_viewed(g, true);
                }
                s.open_finish();
                s.set_verdict(rain_core::Verdict::RequestChanges);
                s.record_round();
                s.build_lens(1);
            }),
            ("reload-banner", |s| {
                scene_base(s);
                seed_threads(s);
                let g = scene_file(s);
                s.reloader.changed.insert(g, vec![0]);
                s.reloader.banner = Some(app::Stale {
                    repo: String::new(),
                    branch: "feature/vouchers".into(),
                    commits: Some(3),
                    sha: "d4e5f6a7b8c9".into(),
                });
                let counts = app::ReloadCounts {
                    moved: 3,
                    addressed: 1,
                    orphaned: 2,
                    changed_files: 1,
                };
                s.reloader.toast = Some((counts.message(), Instant::now()));
                s.reloader.counts = Some(counts);
                if let Some(k) = s.keys.get(g).cloned() {
                    s.goto_later(k);
                }
            }),
            ("conversation", |s| {
                scene_base(s);
                seed_threads(s);
                s.seed_conversation();
            }),
            ("question-answering", |s| {
                scene_base(s);
                s.ask_scene(false);
            }),
            ("question-answered", |s| {
                scene_base(s);
                s.ask_scene(true);
            }),
            ("presence", |s| {
                scene_base(s);
                seed_threads(s);
                s.seed_presence();
            }),
            ("overview", |s| {
                scene_base(s);
                s.scene_brief(true);
            }),
            ("map", |s| {
                scene_base(s);
                s.scene_map();
            }),
            ("tour-start", |s| {
                scene_base(s);
                s.scene_tour("tour-start");
            }),
            ("tour-step", |s| {
                scene_base(s);
                s.scene_tour("tour-step");
            }),
            ("tour-end", |s| {
                scene_base(s);
                s.scene_tour("tour-end");
            }),
            ("board", |s| {
                scene_base(s);
                seed_threads(s);
                s.unfold_section(app::board::GENERATED);
            }),
            ("board-single", |s| {
                scene_base(s);
                seed_threads(s);
                s.mutate(|ss| ss.ui.reader = rain_core::ReaderMode::Single);
                s.show_pick(app::board::Pick::Group(rain_triage::Group::Generated));
            }),
            ("picker", |s| {
                scene_base(s);
                s.show_picker("voucher");
            }),
            ("error", |s| {
                s.autosave = None;
                s.settings_path = None;
                s.launch.fixture = None;
                s.launch.repos = vec![RepoLaunch {
                    dir: std::env::temp_dir(),
                    ..RepoLaunch::default()
                }];
                s.load_now();
            }),
            ("chrome-zed", |s| scene_chrome(s, settings::Chrome::Zed)),
            ("chrome-islands", |s| scene_chrome(s, settings::Chrome::Islands)),
            ("overview-empty", |s| {
                scene_base(s);
                s.set_reader_tab(app::ReaderTab::Overview);
            }),
            ("peek", |s| {
                scene_base(s);
                s.scene_brief(false);
                if let Some(t) = s.sample_peek_target() {
                    s.open_peek(t, true);
                }
            }),
            ("selection-toolbar", |s| {
                scene_base(s);
                s.selection_scene(None);
            }),
            ("selection-composer", |s| {
                scene_base(s);
                s.selection_scene(Some(app::composer::Later::Compose(rain_comments::ComposerMode::Comment)));
            }),
            ("context-menu", |s| {
                scene_base(s);
                s.selection_scene(Some(app::composer::Later::Menu));
            }),
            ("session", scene_session),
        ],
        fonts,
        families,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(keys::on_key),
        min_size: Some((900.0, 560.0)),
        ..AppSpec::new("Rain", size, init, view::view)
    }
}

/// Opens the window and runs until it closes. Rune's own flags (`--snap`, `--scene`,
/// `--scale`, `--tree`, ...) come from the process arguments; when `launch` is the default,
/// Rain's flags are read from the arguments Rune left (see [`launch_from_args`]). True when the
/// window closed on a close request rather than the event loop ending some other way.
pub fn run(launch: Launch, shared: Arc<Shared>) -> bool {
    let launch = if launch == Launch::default() {
        launch_from_args(rune::extra_args())
    } else {
        launch
    };
    let _ = START.set((launch, shared, Instant::now()));
    let mut app = rune::App::new(spec())
        .args(rune_argv(std::env::args().skip(1)))
        .modifier_keys()
        .on_close(on_close)
        .on_window(icon::apply);
    peek_window::register(&mut app);
    app.run();
    CLOSE_REQUESTED.load(Ordering::SeqCst)
}

static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The close button, Alt+F4, or a `WM_CLOSE` from another process.
pub(crate) fn on_close(s: &mut App, _: &mut Tx) {
    s.ask_shutdown();
    CLOSE_REQUESTED.store(true, Ordering::SeqCst);
}
