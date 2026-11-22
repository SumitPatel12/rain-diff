//! Rain's data model: diffs, comment threads, sessions, the overview document, the brief, review
//! tags and triage groups, launch options and the command keymap. No UI and no I/O beyond serde.

pub mod brief;
pub mod brief_md;
pub mod command;
pub mod comments;
pub mod diff;
pub mod formatting;
pub mod fuzzy;
pub mod hash;
pub mod keymap;
pub mod launch;
pub mod moves;
pub mod notes;
pub mod overview;
pub mod paths;
pub mod session;
pub mod tags;
pub mod verify;

pub use brief::{BriefDoc, Cite};
pub use command::Command;
pub use comments::{
    Anchor, AnchorHow, AnchorState, Answer, AnswerStatus, CodeSnapshot, CommentImport, DismissReason, Disposition, Draft, HunkViewed,
    ImportBody, Message, Orphan, OrphanReason, Target, TargetKind, Thread, ThreadKind, ThreadState, ViewedRecord,
};
pub use diff::{DiffSet, FileDiff, FileDiffBuilder, FileStatus, Hunk, LineKind, LineRec, RepoInfo, Side};
pub use hash::{diff_content_hash, short_sha};
pub use keymap::{Chord, ChordKey, Keymap, Profile, Shortcut};
pub use launch::{Launch, RepoLaunch};
pub use notes::{Note, NoteKind};
pub use overview::OverviewDocument;
pub use session::{
    AcceptedGroup, AskCounts, Event, EventKind, LayoutMode, Presence, PresenceState, ReaderMode, ReadingPlace, RepoFiles, ReviewRound,
    Session, SessionRepo, SessionUi, Shared, UiRequest, UiState, Verdict,
};
pub use tags::{FileTag, Group, Item, Reason, Shape};
