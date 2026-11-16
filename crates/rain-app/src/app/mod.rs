//! The window's state and everything that changes it: loading, the session mirror, the layout,
//! viewed marks, comments, find and the palette. Each command lands in its feature's module.

pub(crate) mod ask;
pub(crate) mod board;
mod brief;
mod chapters;
mod comments;
pub(crate) mod composer;
pub(crate) mod conversation;
mod diagram;
mod editor;
mod filter;
mod find;
pub(crate) mod layout;
mod nav;
mod notes;
pub(crate) mod overview;
pub(crate) mod picker;
mod place;
mod reload;
mod remote;
pub(crate) mod rest;
mod review;
mod ruler;
mod session;
mod tour;
mod wholefile;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use rain_comments::Autosave;
use rain_core::command::Command;
use rain_core::{Anchor, Disposition, FileTag, Launch, RepoInfo, Session, Shared, Thread};
use rain_diagram::Layout;
use rain_diffview::{DiffMode, DiffState};
use rain_git::Resolved;
use rain_overview::{Chapters, Loaded};
use rain_tree::TreeState;
use rune::{TextEdit, Tx};
use rune_kit::PaneState;

use crate::load::{self, Data, LARGE_FILE_LINES, Msg};
use crate::settings::{Density, Settings, Theme, ViewLayout};
use crate::theme::Scheme;

pub use ask::AskState;
pub use board::BoardState;
pub use brief::{ArtifactDraft, BriefState, ComposeMode};
pub use chapters::Subset;
pub use composer::ComposerUi;
pub use conversation::ConversationState;
pub use diagram::DiagramPage;
pub use editor::Launcher;
pub use filter::FilterState;
pub use find::Find;
pub use layout::{LayoutSnapshot, LayoutState, Menus, MoreSub, Overlay};
pub use overview::{PeekContent, PeekTarget, ReaderTab};
pub use picker::PickerState;
pub use reload::{ReloadCounts, ReloadState, Stale};
#[cfg(test)]
pub(crate) use remote::APPLY_CALLS;
pub use review::{ReviewState, VERDICTS, verdict_label};
pub use ruler::RulerState;
pub(crate) use session::{comment_counts, file_of_thread, is_viewed};
pub use tour::{FoundBy, TourStage, TourState};

/// Thread authors whose messages don't count as "someone else's": the user writes without one.
pub(crate) const ME: &str = "";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading(String),
    Error(String),
    Ready,
}

/// An open comment box: on which file and lines, and whether it edits a message.
#[derive(Clone, Debug, PartialEq)]
pub struct ComposerState {
    /// Index into the full file list.
    pub file: usize,
    pub anchor: Anchor,
    pub preview: String,
    /// `(thread, message)` when editing an existing message.
    pub edit_of: Option<(String, String)>,
}

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub edit: TextEdit,
    pub sel: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub launch: Option<Instant>,
    /// Launch → loader done, data → first frame drawn with it, launch → that frame.
    pub load_ms: f64,
    pub data_to_frame_ms: f64,
    pub launch_to_frame_ms: f64,
    pub installed: Option<Instant>,
}

pub struct App {
    pub launch: Launch,
    pub shared: Arc<Shared>,
    pub session_path: Option<PathBuf>,
    pub settings_path: Option<PathBuf>,
    pub settings: Settings,
    pub autosave: Option<Autosave>,
    pub phase: Phase,
    pub repos: Vec<RepoInfo>,
    pub resolved: Vec<Option<Resolved>>,
    pub diff: DiffState,
    pub keys: Vec<String>,
    pub hashes: Vec<String>,
    by_key: BTreeMap<String, usize>,
    pub tree: TreeState,
    pub overview: Option<Loaded>,
    pub overview_issues: Vec<String>,
    pub tags: BTreeMap<String, FileTag>,
    /// The overview's `why` per file key.
    pub why: BTreeMap<String, String>,
    /// Threads shown in their file's "Outdated" strip instead of on a line: their code changed,
    /// or their lines are outside the diff. By file.
    pub unplaced: BTreeMap<usize, Vec<String>>,
    pub chapters: Chapters,
    /// The session as last seen, and its version.
    pub mirror: Session,
    pub seen_version: u64,
    pub seen_overview_version: u64,
    /// The chapter `J`/`K` last moved to.
    pub chapter: usize,
    pub filter: FilterState,
    /// The session version the filter subset was last checked against.
    filter_version: u64,
    pub composer: Option<ComposerState>,
    pub composer_edit: TextEdit,
    pub reply_to: Option<String>,
    pub reply_edit: TextEdit,
    pub expanded: BTreeSet<String>,
    /// Threads and notes (`note:<id>`) folded into the gutter; kept across reloads, not saved.
    pub folded: BTreeSet<String>,
    /// The note open in the Intent window.
    pub popped_note: Option<String>,
    /// Active time toward the break reminder, and its rain.
    pub rest: rest::Rest,
    pub focused_thread: Option<String>,
    pub palette: PaletteState,
    pub find: Find,
    pub diagram: DiagramPage,
    pub layouts: Vec<Option<Layout>>,
    /// The list pane; closed it draws as a rail.
    pub sidebar: PaneState,
    pub layout: LayoutState,
    pub menus: Menus,
    pub board: BoardState,
    pub picker: PickerState,
    pub conversation: ConversationState,
    pub brief: BriefState,
    pub tour: TourState,
    pub ask: AskState,
    pub review: ReviewState,
    pub composer_ui: ComposerUi,
    pub reloader: ReloadState,
    pub ruler: RulerState,
    pub diff_width: f32,
    pub window_width: f32,
    /// A window size laid out since the last save.
    window_size: Option<(f32, f32)>,
    pub status: String,
    pub warm: bool,
    /// The code palette the highlighter was made for, and the scheme the view last drew.
    hl_palette: Option<rain_syntax::Palette>,
    pub seen_scheme: Cell<Scheme>,
    pending_goto: Option<String>,
    rx: Option<Receiver<Msg>>,
    texts_tx: Option<Sender<Msg>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    pub timings: Timings,
    pub launcher: Launcher,
}

impl App {
    pub fn new(launch: Launch, shared: Arc<Shared>, session_dir: Option<PathBuf>, settings_path: Option<PathBuf>) -> App {
        let (settings, settings_path, settings_error) = match settings_path.as_deref().map(Settings::try_load) {
            Some(Ok(s)) => (s, settings_path, None),
            // A broken hand edit is reported and left alone rather than replaced by the defaults.
            Some(Err(e)) => (Settings::default(), None, Some(format!("settings ignored: {e}"))),
            None => (Settings::default(), None, None),
        };
        if let Some(e) = &settings_error {
            eprintln!("Rain: {e}");
        }
        let id = load::session_id(&launch);
        let session_path = session_dir.map(|d| rain_comments::SessionFile::path_in(&d, &id));
        if let Some(path) = &session_path {
            let mut s = shared.lock();
            if s.id.is_empty() && s.threads.is_empty() && s.viewed.is_empty() {
                let loaded = if launch.clean {
                    Session::default()
                } else {
                    // Moved aside, not dropped: autosave would otherwise write the empty session over it.
                    rain_comments::SessionFile::load(path).unwrap_or_else(|e| {
                        let kept = path.with_extension("json.corrupt");
                        eprintln!("Rain: {} is unreadable ({e}); kept as {}", path.display(), kept.display());
                        let _ = std::fs::rename(path, &kept);
                        Session::default()
                    })
                };
                let version = s.version.max(loaded.version);
                let comments_version = s.comments_version.max(loaded.comments_version);
                *s = loaded;
                s.version = version;
                s.comments_version = comments_version;
            }
            if s.id.is_empty() {
                s.id = id.clone();
            }
        }
        let autosave = session_path
            .clone()
            .map(|p| rain_comments::autosave(shared.clone(), p, std::time::Duration::from_millis(400)));
        let goto = launch.goto.clone();
        if settings.migrated
            && let Some(p) = &settings_path
        {
            let _ = settings.save(p);
        }
        App {
            sidebar: layout::list_pane(settings.sidebar_width, settings.list_open),
            layout: LayoutState::default(),
            menus: Menus::default(),
            board: BoardState::default(),
            picker: PickerState::default(),
            conversation: ConversationState::default(),
            brief: BriefState::default(),
            tour: TourState::default(),
            ask: AskState::default(),
            review: ReviewState::default(),
            composer_ui: ComposerUi::default(),
            reloader: ReloadState::default(),
            ruler: RulerState::default(),
            launch,
            shared,
            session_path,
            settings_path,
            settings,
            autosave,
            phase: Phase::Loading("Starting".into()),
            repos: Vec::new(),
            resolved: Vec::new(),
            diff: DiffState::default(),
            keys: Vec::new(),
            hashes: Vec::new(),
            by_key: BTreeMap::new(),
            tree: TreeState::build(&[] as &[rain_core::FileDiff], &[], &BTreeMap::new(), |_| false, &BTreeMap::new()),
            overview: None,
            overview_issues: Vec::new(),
            tags: BTreeMap::new(),
            why: BTreeMap::new(),
            unplaced: BTreeMap::new(),
            chapters: Chapters::default(),
            mirror: Session::default(),
            seen_version: u64::MAX,
            seen_overview_version: 0,
            chapter: 0,
            filter: FilterState::default(),
            filter_version: u64::MAX,
            composer: None,
            composer_edit: TextEdit::default(),
            reply_to: None,
            reply_edit: TextEdit::default(),
            expanded: BTreeSet::new(),
            folded: BTreeSet::new(),
            popped_note: None,
            rest: rest::Rest::default(),
            focused_thread: None,
            palette: PaletteState::default(),
            find: Find::default(),
            diagram: DiagramPage::default(),
            layouts: Vec::new(),
            diff_width: f32::MAX,
            window_width: 0.0,
            window_size: None,
            status: settings_error.unwrap_or_default(),
            warm: false,
            hl_palette: None,
            seen_scheme: Cell::new(Scheme::Dark),
            pending_goto: goto,
            rx: None,
            texts_tx: None,
            wake: None,
            timings: Timings::default(),
            launcher: Box::new(|argv: &[String]| editor::spawn_detached(argv)),
        }
    }

    // ----- loading -----

    /// Starts the loader thread; results arrive through [`App::poll`].
    pub fn start_loading(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.texts_tx = Some(tx.clone());
        self.wake = Some(wake.clone());
        self.phase = Phase::Loading("Starting".into());
        load::spawn(self.launch.clone(), self.shared.clone(), tx, wake);
    }

    /// A load is under way (or held), so setup starts none.
    pub fn loading_started(&self) -> bool {
        self.rx.is_some()
    }

    /// Shows the loader with `msg` over a load that never answers (the loading scene).
    pub fn hold_loading(&mut self, msg: &str) {
        self.rx = Some(channel().1);
        self.phase = Phase::Loading(msg.into());
    }

    /// Loads on this thread (scenes, tests).
    pub fn load_now(&mut self) {
        match load::load(&self.launch, &self.shared, &|_| {}) {
            Ok(d) => self.install(d),
            Err(e) => self.phase = Phase::Error(e),
        }
        rain_syntax::warm();
        self.warm = true;
        self.hl_palette = None;
    }

    pub fn reload(&mut self) {
        match self.wake.clone() {
            Some(w) if self.phase == Phase::Ready => self.reload_in_place(w),
            Some(w) => self.start_loading(w),
            None => self.load_now(),
        }
    }

    pub fn install(&mut self, d: Data) {
        let before = self.before_install();
        let keep = self.current_key();
        self.repos = d.repos;
        self.resolved = d.resolved;
        self.keys = d.keys;
        self.hashes = d.hashes;
        self.by_key = self.keys.iter().enumerate().map(|(i, k)| (k.clone(), i)).collect();
        self.tags = d.tags;
        self.overview = d.overview;
        self.overview_issues = d.overview_issues;
        self.timings.load_ms = d.timings.total_ms;
        let mut diff = DiffState::new(d.files);
        diff.set_repo_ids(self.repos.iter().map(|r| r.id.clone()).collect());
        diff.set_collapsed_where(true, |f| f.lines.len() > LARGE_FILE_LINES || (f.generated && f.lines.len() > 2_000));
        self.diff = diff;
        self.install_excerpt_source();
        self.filter.subset = None;
        self.composer = None;
        self.mirror = self.shared.lock().clone();
        self.seen_version = self.mirror.version;
        self.seen_overview_version = self.mirror.overview_version;
        self.rebuild_overview_views();
        self.install_brief_facts(&d.triage);
        self.settle_reader_tab();
        let session = &self.mirror;
        let viewed = |k: &str| {
            self.by_key
                .get(k)
                .is_some_and(|&i| is_viewed(session, &self.repos, &self.diff, &self.hashes, i))
        };
        let counts = comment_counts(session, self);
        self.tree = TreeState::build(self.diff.files(), &self.repos, &self.tags, viewed, &counts);
        self.install_triage(d.triage);
        self.phase = Phase::Ready;
        self.enter_chapter(self.chapter);
        self.apply_split();
        self.rebuild_annotations();
        self.hl_palette = None;
        if self.pending_goto.is_none() {
            self.pending_goto = keep.clone();
            self.brief.restore_tab = keep.is_some();
        }
        self.after_install(before, keep);
        self.timings.installed = Some(Instant::now());
    }

    /// Lets `POST /api/comment-imports` snapshot a thread's code against this diff at post time.
    fn install_excerpt_source(&self) {
        let files = self.diff.files().to_vec();
        let ids: Vec<String> = self.repos.iter().map(|r| r.id.clone()).collect();
        self.shared.set_excerpt_source(Box::new(move |repo, path, anchor| {
            let f = files
                .iter()
                .find(|f| f.path == path && (repo.is_empty() || ids.get(f.repo as usize).is_some_and(|id| id == repo)))?;
            rain_comments::ops::snapshot_at(f, anchor, path).map(|mut s| {
                s.language = rain_diffview::language(path).map(str::to_owned);
                s
            })
        }));
    }

    /// Tags, notes' `why`, chapters and diagram layouts from the overview.
    pub(crate) fn rebuild_overview_views(&mut self) {
        self.why.clear();
        self.layouts.clear();
        if let Some(l) = &self.overview {
            for o in &l.document.reading_order {
                for e in &o.review {
                    self.why.insert(rain_core::paths::join(&o.repo, &e.path), e.why.clone());
                }
            }
            self.chapters = rain_overview::chapters(&l.document, &self.keys, &self.tags);
            self.layouts = l.targets.iter().map(|t| rain_diagram::render(&l.document.graph, t)).collect();
            if let Some(d) = rain_overview::default_target(&l.document.graph, &l.targets)
                && let Some(i) = l.targets.iter().position(|t| t == d)
            {
                self.diagram.target = i;
            }
        } else {
            self.chapters = Chapters::default();
        }
        self.diagram.target = self.diagram.target.min(self.layouts.len().saturating_sub(1));
    }

    /// Frame start: loader messages, session changes from other threads, the highlighter's
    /// theme, a pending jump, requests from the server. True when anything changed.
    pub fn poll(&mut self, tx: &mut Tx) -> bool {
        let mut changed = false;
        // Taken before the loader messages, so an install is timed through the frame that drew it.
        if let Some(at) = self.timings.installed.take() {
            self.timings.data_to_frame_ms = at.elapsed().as_secs_f64() * 1000.0;
            if let Some(l) = self.timings.launch {
                self.timings.launch_to_frame_ms = l.elapsed().as_secs_f64() * 1000.0;
            }
            if std::env::var_os("RAIN_TIMINGS").is_some() {
                let unix_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis());
                eprintln!(
                    "Rain timings: files {} rows {} load {:.0} ms, data->frame {:.0} ms, run->frame {:.0} ms, unix_ms {unix_ms}",
                    self.keys.len(),
                    self.diff.row_count(),
                    self.timings.load_ms,
                    self.timings.data_to_frame_ms,
                    self.timings.launch_to_frame_ms
                );
            }
        }
        if let Some(rx) = &self.rx {
            let msgs: Vec<Msg> = rx.try_iter().collect();
            for m in msgs {
                changed = true;
                match m {
                    Msg::Progress(p) if self.phase != Phase::Ready => self.phase = Phase::Loading(p),
                    Msg::Progress(p) => self.status = p,
                    Msg::Done(r) => match *r {
                        Ok(d) => {
                            self.install(d);
                            // The diff lands at once: fading its rows in costs 25-50 ms a frame.
                            tx.instant();
                        }
                        Err(e) => self.phase = Phase::Error(e),
                    },
                    Msg::Warm => {
                        self.warm = true;
                        self.hl_palette = None;
                    }
                    texts @ Msg::Texts { .. } => self.install_texts(texts),
                }
            }
        }
        if self.phase == Phase::Ready {
            changed |= self.sync_session();
            if self.filter.subset.is_some() && !self.tree.filter.is_empty() && self.filter_version != self.seen_version {
                // Retags and comment counts refilter the tree without telling the subset.
                self.filter_version = self.seen_version;
                self.apply_filter(&self.tree.filter.clone());
            }
            let palette = self.seen_scheme.get().syntax();
            if self.warm && self.hl_palette != Some(palette) {
                self.hl_palette = Some(palette);
                self.diff.set_highlighter(rain_syntax::highlighter(palette));
                for v in self.subsets_mut() {
                    v.diff.set_highlighter(rain_syntax::highlighter(palette));
                }
                changed = true;
            }
            if let Some(g) = self.pending_goto.take() {
                let tab = std::mem::take(&mut self.brief.restore_tab).then(|| self.brief.tab.replace(ReaderTab::Changes));
                match g.strip_prefix("thread:") {
                    Some(id) => self.goto_thread(id, tx),
                    None => self.goto_spec(&g, tx),
                }
                if let Some(t) = tab {
                    self.brief.tab = t;
                }
                changed = true;
            }
            changed |= self.peek_tick(Instant::now());
            changed |= self.selection_poll(tx);
            changed |= self.apply_requests(tx);
            changed |= self.rounds_poll(tx);
            changed |= self.ask_poll(tx);
        }
        changed |= self.rest_poll(tx);
        self.save_pane_state();
        changed
    }

    /// Saves the list's width and openness once a drag ends, and the window size; zoom hides the
    /// list without changing the preference.
    fn save_pane_state(&mut self) {
        if self.sidebar.is_resizing() {
            return;
        }
        let mut dirty = false;
        let sw = self.sidebar.width();
        if (sw - self.settings.sidebar_width).abs() > 0.5 {
            self.settings.sidebar_width = sw;
            dirty = true;
        }
        if !self.layout.is_zoomed() && self.sidebar.is_open() != self.settings.list_open {
            self.settings.list_open = self.sidebar.is_open();
            dirty = true;
        }
        if let Some(w) = self.window_size.take()
            && ((w.0 - self.settings.window.0).abs() >= 1.0 || (w.1 - self.settings.window.1).abs() >= 1.0)
        {
            self.settings.window = w;
            dirty = true;
        }
        if dirty {
            self.save_settings();
        }
    }

    /// Replaces the settings and rebuilds the panes from them (scenes start from the defaults).
    pub fn reset_settings(&mut self, settings: Settings) {
        self.sidebar = layout::list_pane(settings.sidebar_width, settings.list_open);
        self.layout = LayoutState::default();
        self.settings = settings;
    }

    pub fn save_settings(&self) {
        if let Some(p) = &self.settings_path {
            let _ = self.settings.save(p);
        }
    }

    // ----- files and keys -----

    /// The subset of files on screen: file-at-a-time's one file, else the filter's, else none
    /// (every file).
    pub fn subset(&self) -> Option<&Subset> {
        self.board.single.as_ref().or(self.filter.subset.as_ref())
    }

    pub fn subset_mut(&mut self) -> Option<&mut Subset> {
        self.board.single.as_mut().or(self.filter.subset.as_mut())
    }

    /// Every subset diff that exists, so settings and texts reach them all.
    pub(crate) fn subsets_mut(&mut self) -> impl Iterator<Item = &mut Subset> {
        self.board.single.as_mut().into_iter().chain(self.filter.subset.as_mut())
    }

    /// The diff on screen: the subset's when there is one, else every file.
    pub fn active(&self) -> &DiffState {
        match self.subset() {
            Some(v) => &v.diff,
            None => &self.diff,
        }
    }

    pub fn active_mut(&mut self) -> &mut DiffState {
        if self.subset().is_some() {
            return &mut self.subset_mut().expect("checked above").diff;
        }
        &mut self.diff
    }

    /// The full-list index of the active diff's file `local`.
    pub fn global(&self, local: usize) -> usize {
        match self.subset() {
            Some(v) => v.files.get(local).copied().unwrap_or(usize::MAX),
            None => local,
        }
    }

    pub fn local(&self, global: usize) -> Option<usize> {
        match self.subset() {
            Some(v) => v.files.iter().position(|&g| g == global),
            None => (global < self.keys.len()).then_some(global),
        }
    }

    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.by_key.get(key).copied()
    }

    /// Reads the active diff's file `local` whole from git, off the UI thread when the window
    /// runs, so its unchanged ranges can open.
    pub fn fetch_texts(&mut self, local: usize) {
        let g = self.global(local);
        let Some(f) = self.diff.files().get(g).cloned() else { return };
        let r = f.repo as usize;
        let (Some(Some(resolved)), Some(repo)) = (self.resolved.get(r).cloned(), self.repos.get(r)) else {
            self.status = "No repository to read the whole file from".into();
            return;
        };
        let (dir, key) = (repo.dir.clone(), self.keys[g].clone());
        self.status = format!("Reading {}", f.path);
        match (self.texts_tx.clone(), self.wake.clone()) {
            (Some(tx), Some(wake)) => {
                let _ = std::thread::Builder::new().name("rain-texts".into()).spawn(move || {
                    let _ = tx.send(load::read_texts(&dir, &resolved, &f, key));
                    wake();
                });
            }
            _ => {
                let msg = load::read_texts(&dir, &resolved, &f, key);
                self.install_texts(msg);
            }
        }
    }

    pub(crate) fn install_texts(&mut self, msg: Msg) {
        let Msg::Texts { key, old, new } = msg else { return };
        let Some(g) = self.index_of(&key) else { return };
        if old.is_none() && new.is_none() {
            self.status = format!("Could not read {key}");
            self.set_whole_everywhere(g, false);
            return;
        }
        let wanted = self.diff.needs_texts(g)
            || self
                .subsets_mut()
                .any(|v| v.files.iter().position(|&x| x == g).is_some_and(|l| v.diff.needs_texts(l)));
        if !wanted {
            return;
        }
        self.set_texts_everywhere(g, old, new);
        self.status.clear();
    }

    /// The file the reader is on: the cursor's, else the one at the top of the diff.
    pub fn current(&self) -> Option<usize> {
        let a = self.active();
        if a.files().is_empty() {
            return None;
        }
        let local = a.cursor().map_or_else(|| a.visible_file(), |(f, _)| f);
        let g = self.global(local);
        (g < self.keys.len()).then_some(g)
    }

    pub fn current_key(&self) -> Option<String> {
        self.current().map(|i| self.keys[i].clone())
    }

    /// The repo a thread or viewed mark records: empty in a one-repo session, as difit writes it.
    pub fn repo_tag(&self, file: usize) -> &str {
        if self.repos.len() <= 1 {
            return "";
        }
        let r = self.diff.files()[file].repo as usize;
        self.repos.get(r).map_or("", |r| r.id.as_str())
    }

    pub fn file_of_thread(&self, t: &Thread) -> Option<usize> {
        file_of_thread(self, t)
    }

    // ----- split -----

    /// `s`: split ↔ unified (Auto counts as split).
    pub fn toggle_split(&mut self) {
        let next = match self.settings.layout {
            ViewLayout::Unified => ViewLayout::Split,
            ViewLayout::Split | ViewLayout::Auto => ViewLayout::Unified,
        };
        self.set_layout(next);
    }

    pub fn set_layout(&mut self, layout: ViewLayout) {
        self.settings.layout = layout;
        self.save_settings();
        self.apply_split();
        self.status = match self.layout_reason() {
            Some(why) => format!("Split, showing unified: {why}"),
            None => format!("{} view", layout.label()),
        };
    }

    pub fn effective_mode(&self) -> DiffMode {
        self.layout_decision().0
    }

    /// Puts the diff layout and the density's row heights on every diff.
    pub fn apply_split(&mut self) {
        let m = self.effective_mode();
        let metrics = match self.settings.density {
            Density::Compact => rain_diffview::Metrics::COMPACT,
            Density::Comfortable => rain_diffview::Metrics::COMFORTABLE,
        };
        self.diff.set_mode(m);
        self.diff.set_metrics(metrics);
        for v in self.subsets_mut() {
            v.diff.set_mode(m);
            v.diff.set_metrics(metrics);
        }
    }

    pub fn set_diff_width(&mut self, w: f32) {
        if (w - self.diff_width).abs() >= 1.0 {
            self.diff_width = w;
            self.apply_split();
        }
    }

    /// The window was laid out at `size`; its width decides docking, its size is saved.
    pub fn set_window_size(&mut self, size: (f32, f32)) {
        if (size.0 - self.window_width).abs() >= 1.0 {
            self.window_width = size.0;
        }
        if (size.0 - self.settings.window.0).abs() >= 1.0 || (size.1 - self.settings.window.1).abs() >= 1.0 {
            self.window_size = Some(size);
        }
    }

    // ----- palette -----

    pub fn open_palette(&mut self) {
        self.palette.open = true;
        self.palette.edit = TextEdit::default();
        self.palette.edit.focused = true;
        self.palette.sel = 0;
    }

    pub fn close_palette(&mut self) {
        self.palette.open = false;
        self.palette.edit.focused = false;
    }

    // ----- misc commands -----

    pub fn toggle_theme(&mut self) {
        let dark = self.seen_scheme.get().is_dark();
        self.settings.theme = if dark { Theme::Light } else { Theme::Dark };
        self.save_settings();
    }

    pub fn toggle_density(&mut self) {
        self.settings.density = match self.settings.density {
            Density::Compact => Density::Comfortable,
            Density::Comfortable => Density::Compact,
        };
        self.save_settings();
        self.apply_split();
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.settings.theme = theme;
        self.save_settings();
    }

    pub fn set_density(&mut self, density: Density) {
        self.settings.density = density;
        self.save_settings();
        self.apply_split();
    }

    /// Closes the topmost layer `Esc` reaches before the filter: the palette, the top overlay,
    /// the composer, a reply, find, the tour, a focused thread. Zoom is not a layer.
    fn close_layer(&mut self) -> bool {
        if self.menus.close() {
            return true;
        }
        if self.palette.open {
            self.close_palette();
            return true;
        }
        if self.close_peek() {
            return true;
        }
        if self.hide_selection_toolbar() {
            return true;
        }
        if self.layout.pop_layer().is_some() {
            return true;
        }
        if self.cancel_artifact() {
            return true;
        }
        if self.composer.is_some() {
            self.cancel_composer();
            return true;
        }
        if self.reply_to.take().is_some() {
            return true;
        }
        if self.find.open {
            self.close_find();
            return true;
        }
        self.leave_tour() || self.focused_thread.take().is_some()
    }

    /// Escape: one layer at a time, then the file filter, then the diff takes it.
    pub fn escape(&mut self, tx: &mut Tx) -> bool {
        if self.close_layer() || self.clear_filter() {
            return true;
        }
        self.active_mut().key(
            &rune::KeyEvent {
                key: rune::Key::Escape,
                mods: rune::Mods::default(),
            },
            tx,
        )
    }

    /// Runs a keymap or palette command in the module that owns it.
    pub fn run(&mut self, cmd: Command, tx: &mut Tx) -> bool {
        match cmd {
            Command::CursorDown => {
                self.active_mut().cursor_down(tx);
            }
            Command::CursorUp => {
                self.active_mut().cursor_up(tx);
            }
            Command::NextHunk => {
                self.active_mut().next_hunk(true, tx);
            }
            Command::PrevHunk => {
                self.active_mut().next_hunk(false, tx);
            }
            Command::NextFile | Command::PrevFile => self.step_item(cmd == Command::NextFile, tx),
            Command::NextSection | Command::NextChapter => self.step_section(true, tx),
            Command::PrevSection | Command::PrevChapter => self.step_section(false, tx),
            Command::Back => self.jump_history(true, tx),
            Command::Forward => self.jump_history(false, tx),
            Command::NextWaiting => self.next_waiting(tx),
            Command::ViewedAndNext => self.viewed_and_next(tx),
            Command::ToggleViewed => self.toggle_viewed_current(),
            Command::MarkGroupViewed => self.accept_group(tx),
            Command::UndoAccept => self.undo_accept(tx),
            Command::ToggleSplit => self.toggle_split(),
            Command::LayoutAuto => self.set_layout(ViewLayout::Auto),
            Command::ToggleReaderMode => self.toggle_reader_mode(tx),
            Command::ToggleList => self.toggle_list(),
            Command::Zoom => self.toggle_zoom(),
            Command::ToggleWholeFile => self.toggle_whole_file(tx),
            Command::ExpandAllContext => self.expand_all_context(tx),
            Command::FilterFiles | Command::FocusTree => self.focus_list_filter(tx),
            Command::ClearFilter => {
                self.clear_filter();
            }
            Command::Picker => self.open_picker(tx),
            Command::Palette => self.open_palette(),
            Command::Find => self.open_find(),
            Command::Comment => {
                if !self.artifact_key(ComposeMode::Comment) {
                    self.comment(tx);
                }
            }
            Command::Ask => {
                if !self.artifact_key(ComposeMode::Question) {
                    self.ask_question(tx);
                }
            }
            Command::Reply => self.open_reply(tx),
            Command::Agree => self.dispose(Disposition::Agree),
            Command::Dismiss => self.dispose(Disposition::Dismiss),
            Command::Defer => self.dispose(Disposition::Defer),
            Command::Conversation => self.toggle_conversation(tx),
            Command::Map | Command::Overview => self.toggle_map(),
            Command::Brief => self.toggle_reader_tab(),
            Command::GoOverview => self.set_reader_tab(ReaderTab::Overview),
            Command::GoChanges => self.set_reader_tab(ReaderTab::Changes),
            Command::Tour => self.start_tour(tx),
            Command::FinishReview => self.finish_review(tx),
            Command::KeySheet => {
                self.open_palette();
                self.status = "Every command with its keys".into();
            }
            Command::OpenInEditor => self.open_in_editor(),
            Command::CopyLocation => self.copy_location(tx),
            Command::CopyAll => self.copy_all(tx),
            Command::Reload => self.reload_now(tx),
            Command::ToggleTheme => self.toggle_theme(),
            Command::ToggleDensity => self.toggle_density(),
            Command::ToggleChaptersQueue => {
                self.status = "Chapters and the queue are groups in the list now".into();
            }
            Command::ToggleInspector => {
                self.status = "No inspector: notes sit in the diff, the story above it".into();
            }
            Command::Escape => return self.escape(tx),
        }
        true
    }

    /// Whether one of the app's own text fields has the keyboard.
    pub fn typing(&self) -> bool {
        self.composer_edit.focused
            || self.brief.edit.focused
            || self.reply_edit.focused
            || self.find.edit.focused
            || self.palette.edit.focused
            || self.tree.filter_edit.focused
            || self.review.summary.focused
            || (self.picker.edit.focused && self.layout.is_open(Overlay::Picker))
    }
}
