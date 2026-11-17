//! Finish review and rounds: the dialog's state and checks, the round it records, and the
//! "since round N" lens that narrows the reader to hunks changed after a round.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use rain_core::session::ThreadOutcome;
use rain_core::{Event, EventKind, FileDiff, FileDiffBuilder, FileTag, ReviewRound, Session, Side, ThreadKind, Verdict};
use rain_diffview::DiffState;
use rune::{TextEdit, Tx};

use super::{App, ME, Overlay, Subset};
use crate::load::LARGE_FILE_LINES;

/// The lens on screen: the round it compares with and the subset it built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lens {
    pub round: u32,
    pub files: Vec<usize>,
    pub hunks: usize,
}

#[derive(Default)]
pub struct ReviewState {
    pub verdict: Option<Verdict>,
    pub summary: TextEdit,
    /// Each repo's loaded target when the dialog opened.
    opened: Vec<(String, String)>,
    /// Why the last Submit was refused.
    pub refused: Option<String>,
    pub lens: Option<Lens>,
    /// The round whose lens git is still working out.
    lens_pending: Option<u32>,
}

/// Per repo index, git's line map of each of its files since a round.
pub type LineMaps = BTreeMap<usize, Vec<Vec<[u32; 4]>>>;

/// One repo's `line_maps` call, owned so it can run on another thread.
pub(crate) struct LensJob {
    repo: usize,
    dir: std::path::PathBuf,
    resolved: rain_git::Resolved,
    from: String,
    paths: Vec<String>,
}

fn run_lens_jobs(jobs: Vec<LensJob>) -> LineMaps {
    jobs.into_iter()
        .filter_map(|j| {
            let paths: Vec<&str> = j.paths.iter().map(String::as_str).collect();
            let maps = rain_git::line_maps(&j.dir, &j.resolved, Side::New, &j.from, &paths).ok()?;
            Some((j.repo, maps))
        })
        .collect()
}

/// One line of the dialog's checklist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub text: String,
    /// Offers "Go to first": the first Must file not viewed.
    pub go_to_must: bool,
}

pub fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Approve => "Approve",
        Verdict::RequestChanges => "Request changes",
        Verdict::Comment => "Comment",
    }
}

pub const VERDICTS: [Verdict; 3] = [Verdict::Approve, Verdict::RequestChanges, Verdict::Comment];

impl App {
    /// `Shift+F` and the toolbar's accent button: opens the dialog with no verdict picked.
    pub fn finish_review(&mut self, _tx: &mut Tx) {
        self.open_finish();
    }

    pub fn open_finish(&mut self) {
        self.review.verdict = None;
        self.review.refused = None;
        self.review.opened = self.loaded_shas();
        self.review.summary = TextEdit::default();
        self.review.summary.text = self.summary_draft();
        self.open_overlay(Overlay::Finish);
        self.status = format!("Finish review · round {}", self.mirror.round());
    }

    pub fn close_finish(&mut self) {
        self.review.summary.focused = false;
        self.close_overlay(Overlay::Finish);
    }

    fn loaded_shas(&self) -> Vec<(String, String)> {
        self.repos.iter().map(|r| (r.id.clone(), r.resolved_target.clone())).collect()
    }

    pub fn must_unviewed(&self) -> Vec<usize> {
        (0..self.keys.len())
            .filter(|&g| self.tags.get(&self.keys[g]) == Some(&FileTag::MustReview) && !self.is_viewed(g))
            .collect()
    }

    fn open_questions(&self) -> usize {
        self.mirror
            .threads
            .iter()
            .filter(|t| t.is_open() && t.kind() == ThreadKind::Question)
            .count()
    }

    /// "Viewed 12 of 25 files · 3 comments · 1 open question".
    pub fn summary_draft(&self) -> String {
        let viewed = (0..self.keys.len()).filter(|&g| self.is_viewed(g)).count();
        let mine = self
            .mirror
            .threads
            .iter()
            .filter(|t| t.messages.first().is_some_and(|m| m.author.as_deref().unwrap_or(ME) == ME))
            .count();
        let mut parts = vec![format!("Viewed {viewed} of {} files", self.keys.len())];
        if mine > 0 {
            parts.push(format!("{mine} comment{}", plural(mine)));
        }
        let q = self.open_questions();
        if q > 0 {
            parts.push(format!("{q} open question{}", plural(q)));
        }
        parts.join(" · ")
    }

    /// What is left before the review is done, in the dialog's order.
    pub fn finish_warnings(&self) -> Vec<Warning> {
        let mut out = Vec::new();
        let must = self.must_unviewed().len();
        if must > 0 {
            out.push(Warning {
                text: format!("{must} Must file{} not viewed", plural(must)),
                go_to_must: true,
            });
        }
        let waiting = self.waiting_count();
        if waiting > 0 {
            out.push(warn(format!("{waiting} thread{} waiting on you", plural(waiting))));
        }
        let drafts = self.mirror.drafts.len();
        if drafts > 0 {
            out.push(warn(format!("{drafts} unsent draft{}", plural(drafts))));
        }
        let q = self.open_questions();
        if q > 0 {
            out.push(warn(format!("{q} open question{}", plural(q))));
        }
        for a in self.mirror.accepted.iter().filter(|a| !a.undone) {
            out.push(warn(format!("Accepted without reading: {}", a.reason)));
        }
        out
    }

    /// Nothing left on the work list (the accept log is a record, not work).
    pub fn finish_ready(&self) -> bool {
        self.must_unviewed().is_empty() && self.waiting_count() == 0 && self.mirror.drafts.is_empty() && self.open_questions() == 0
    }

    pub fn goto_first_must(&mut self, tx: &mut Tx) {
        if let Some(&g) = self.must_unviewed().first() {
            self.close_finish();
            self.goto_file(g, tx);
        }
    }

    pub fn set_verdict(&mut self, v: Verdict) {
        self.review.verdict = Some(v);
        self.review.refused = None;
    }

    /// Records the round, unless no verdict is picked or the loaded diff changed since the
    /// dialog opened; true when it was recorded.
    pub fn submit_review(&mut self, _tx: &mut Tx) -> bool {
        self.record_round()
    }

    pub fn record_round(&mut self) -> bool {
        let Some(verdict) = self.review.verdict else {
            self.review.refused = Some("Pick a verdict first".into());
            return false;
        };
        if self.loaded_shas() != self.review.opened {
            let now = self.repos.first().map_or("", |r| r.resolved_target.as_str());
            self.review.refused = Some(format!(
                "The diff was reloaded (now at {}) since you opened this. Look at what changed, then finish again.",
                rain_core::short_sha(now)
            ));
            return false;
        }
        let summary = self.review.summary.text.trim().to_owned();
        let round = self.mutate(|s| {
            let r = round_record(s, verdict, summary);
            let n = r.n;
            s.rounds.push(r);
            s.version += 1;
            n
        });
        self.shared.push_event(Event {
            at: rain_comments::clock::now_iso(),
            kind: EventKind::Submitted,
            data: serde_json::json!({ "round": round, "verdict": verdict }),
            ..Event::default()
        });
        self.shared.notify();
        self.close_finish();
        self.status = format!("Round {round} submitted: {}", verdict_label(verdict));
        true
    }

    // ----- the since-round lens -----

    /// Narrows the reader to the hunks changed after round `n` (`None`: every hunk).
    pub fn set_lens(&mut self, n: Option<u32>, tx: &mut Tx) {
        let Some(n) = n else {
            self.review.lens_pending = None;
            if self.review.lens.take().is_some() {
                self.filter.subset = None;
                self.rebuild_annotations();
                self.status = "Showing every change since the base".into();
                if let Some(g) = self.current() {
                    self.goto_file(g, tx);
                }
            }
            return;
        };
        self.request_lens(n, true, tx);
    }

    /// Asks git what changed since round `n`, off the UI thread when the window runs; the lens
    /// goes up in [`App::finish_lens`]. `announce` scrolls to it and says what it shows.
    fn request_lens(&mut self, n: u32, announce: bool, tx: &mut Tx) {
        let Some(round) = self.mirror.rounds.iter().find(|r| r.n == n).cloned() else {
            return;
        };
        let jobs = self.lens_jobs(&round);
        self.review.lens_pending = Some(n);
        match (self.texts_tx.clone(), self.wake.clone()) {
            (Some(sender), Some(wake)) if !jobs.is_empty() => {
                let installs = self.reloader.installs();
                self.status = format!("Finding what changed since round {n}");
                let _ = std::thread::Builder::new().name("rain-lens".into()).spawn(move || {
                    let maps = run_lens_jobs(jobs);
                    let _ = sender.send(crate::load::Msg::Lens {
                        round: n,
                        installs,
                        announce,
                        maps,
                    });
                    wake();
                });
            }
            _ => {
                let maps = run_lens_jobs(jobs);
                self.finish_lens(n, maps, announce, tx);
            }
        }
    }

    /// Puts up the lens git answered for, unless the user moved on or a reload replaced the diff.
    pub(crate) fn lens_answered(&mut self, round: u32, installs: u32, announce: bool, maps: LineMaps, tx: &mut Tx) {
        if self.review.lens_pending == Some(round) && installs == self.reloader.installs() {
            self.finish_lens(round, maps, announce, tx);
        }
    }

    fn finish_lens(&mut self, n: u32, maps: LineMaps, announce: bool, tx: &mut Tx) {
        self.review.lens_pending = None;
        if !self.install_lens(n, &maps) || !announce {
            return;
        }
        self.active_mut().scroll_to_file(0, tx);
        let (files, hunks) = self.review.lens.as_ref().map_or((0, 0), |l| (l.files.len(), l.hunks));
        self.status = format!(
            "Since round {n}: {hunks} hunk{} in {files} file{} · Show all resets",
            plural(hunks),
            plural(files)
        );
    }

    /// Builds the lens subset for round `n` on this thread (scenes); false (with a status) when
    /// nothing changed since.
    pub fn build_lens(&mut self, n: u32) -> bool {
        let Some(round) = self.mirror.rounds.iter().find(|r| r.n == n) else {
            return false;
        };
        let maps = run_lens_jobs(self.lens_jobs(round));
        self.install_lens(n, &maps)
    }

    fn install_lens(&mut self, n: u32, maps: &LineMaps) -> bool {
        let Some(round) = self.mirror.rounds.iter().find(|r| r.n == n).cloned() else {
            return false;
        };
        let picks = self.changed_since_round(&round, maps);
        let files: Vec<usize> = picks.keys().copied().collect();
        let hunks = picks.values().map(Vec::len).sum();
        if files.is_empty() {
            self.status = format!("Nothing changed since round {n}");
            return false;
        }
        self.tree.apply(&rain_tree::TreeFilter::default());
        let diffs: Vec<FileDiff> = picks.iter().map(|(&g, hs)| only_hunks(&self.diff.files()[g], hs)).collect();
        let mut diff = DiffState::new(diffs);
        diff.set_repo_ids(self.repos.iter().map(|r| r.id.clone()).collect());
        if let Some(palette) = self.hl_palette {
            diff.set_highlighter(rain_syntax::highlighter(palette));
        }
        diff.set_mode(self.diff.mode());
        diff.set_collapsed_where(true, |f| f.lines.len() > LARGE_FILE_LINES);
        self.filter.subset = Some(Subset {
            diff,
            files: files.clone(),
        });
        self.review.lens = Some(Lens { round: n, files, hunks });
        self.rebuild_annotations();
        true
    }

    /// The full-list files of repo `r`, in list order.
    fn repo_files(&self, r: usize) -> Vec<usize> {
        (0..self.keys.len()).filter(|&g| self.diff.files()[g].repo as usize == r).collect()
    }

    /// The git line maps the lens needs: one per repo with both the round's and the loaded target.
    fn lens_jobs(&self, round: &ReviewRound) -> Vec<LensJob> {
        let mut jobs = Vec::new();
        for (r, repo) in self.repos.iter().enumerate() {
            let from = round
                .shas
                .iter()
                .find(|s| s.id == repo.id || round.shas.len() == 1)
                .map(|s| s.target.clone());
            if let (Some(from), Some(Some(res))) = (from, self.resolved.get(r)) {
                if from.is_empty() {
                    continue;
                }
                jobs.push(LensJob {
                    repo: r,
                    dir: repo.dir.clone(),
                    resolved: res.clone(),
                    from,
                    paths: self.repo_files(r).iter().map(|&g| self.diff.files()[g].path.clone()).collect(),
                });
            }
        }
        jobs
    }

    /// Per file, the hunks changed after `round`: by git between the round's target and the
    /// loaded one where `maps` has the repo, else hunks whose text the round's viewed marks lack.
    fn changed_since_round(&self, round: &ReviewRound, line_maps: &LineMaps) -> BTreeMap<usize, Vec<usize>> {
        let mut out = BTreeMap::new();
        for r in 0..self.repos.len() {
            let files = self.repo_files(r);
            let maps = line_maps.get(&r);
            for (i, &g) in files.iter().enumerate() {
                let f = &self.diff.files()[g];
                let picked: Vec<usize> = match &maps {
                    Some(m) => {
                        let m = m.get(i).map(Vec::as_slice).unwrap_or_default();
                        (0..f.hunks.len()).filter(|&h| touches(&f.hunks[h], m)).collect()
                    }
                    None => {
                        let seen: HashSet<&str> = round
                            .viewed
                            .iter()
                            .filter(|v| v.file_path == f.path && v.repo == self.repo_tag(g))
                            .flat_map(|v| v.hunks.iter().map(|h| h.hash.as_str()))
                            .collect();
                        (0..f.hunks.len())
                            .filter(|&h| self.diff.hunk_hash(g, h).is_none_or(|x| !seen.contains(x.as_str())))
                            .collect()
                    }
                };
                if !picked.is_empty() {
                    out.insert(g, picked);
                }
            }
        }
        out
    }

    /// Rebuilds the lens after a reload replaced the diff.
    pub(crate) fn reapply_lens(&mut self) {
        if let Some(n) = self.review.lens.take().map(|l| l.round).or(self.review.lens_pending) {
            self.request_lens(n, false, &mut Tx::default());
        }
    }

    /// Drops the lens when a filter replaced its subset; true when it did.
    pub(crate) fn check_lens(&mut self) -> bool {
        let gone = self
            .review
            .lens
            .as_ref()
            .is_some_and(|l| self.filter.subset.as_ref().is_none_or(|s| s.files != l.files));
        if gone {
            self.review.lens = None;
        }
        gone
    }
}

/// The round a submit records: the verdict and summary given, and the session as it is now.
pub fn round_record(s: &Session, verdict: Verdict, summary: String) -> ReviewRound {
    ReviewRound {
        n: s.round(),
        at: rain_comments::clock::now_iso(),
        verdict,
        summary,
        shas: s.repos.clone(),
        viewed: s.viewed.clone(),
        dispositions: s
            .threads
            .iter()
            .filter(|t| t.disposition.is_some() || !t.is_open())
            .map(|t| ThreadOutcome {
                thread: t.id.clone(),
                disposition: t.disposition,
                state: t.state,
                dismiss_reason: t.dismiss_reason,
            })
            .collect(),
        questions_open: s
            .threads
            .iter()
            .filter(|t| t.is_open() && t.kind() == ThreadKind::Question)
            .map(|t| t.id.clone())
            .collect(),
        drafts_unsent: s.drafts.len(),
        accepted: s.accepted.iter().filter(|a| !a.undone).cloned().collect(),
    }
}

/// Whether hunk `h`'s new-side lines meet a range `-U0` reported changed (`[old, n, new, m]`).
fn touches(h: &rain_core::Hunk, map: &[[u32; 4]]) -> bool {
    let (lo, hi) = (h.new_start, h.new_start + h.new_count.max(1));
    map.iter().any(|&[_, _, start, count]| {
        let (a, b) = (start, start + count.max(1));
        a < hi && lo < b
    })
}

/// `f` with only the hunks `keep`.
fn only_hunks(f: &Arc<FileDiff>, keep: &[usize]) -> FileDiff {
    let mut b = FileDiffBuilder::new(f.repo, f.path.clone(), f.status);
    {
        let m = b.file_mut();
        m.old_path = f.old_path.clone();
        m.binary = f.binary;
        m.generated = f.generated;
        m.contents_unavailable = f.contents_unavailable;
    }
    for &h in keep {
        let Some(hunk) = f.hunks.get(h) else { continue };
        b.push_hunk(hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count, f.hunk_section(hunk));
        for i in hunk.lines.clone() {
            let rec = f.lines[i as usize];
            b.push_line(rec.kind, rec.old, rec.new, f.line_text(i));
            if rec.no_newline {
                b.mark_no_newline();
            }
        }
    }
    b.finish()
}

fn warn(text: String) -> Warning {
    Warning { text, go_to_must: false }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
