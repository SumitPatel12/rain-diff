//! Git CLI driver and streaming unified-diff parser.

mod cmd;
mod error;
pub mod generated;
pub mod parse;
mod repo;
pub mod watch;

pub use error::GitError;
pub use generated::generated;
pub use parse::{FileMeta, Patch, parse_reader, parse_str};
pub use repo::{
    DiffOpts, Moved, NameStatus, Resolved, TargetKind, UNTRACKED_MAX_BYTES, added_file, blob, changed_paths, changed_under_w, diff,
    diff_patch, formatting_only, git_dirs, head_sha, header_from_diff, line_map, line_maps, repository_id, resolve, side_commit,
    target_moved, toplevel, untracked,
};
pub use watch::{GitWatch, watch_git};
