//! Question mode's answerer: the prompt a question becomes, which paths never leave the window,
//! and a local command that answers on stdout. No network call is made here; whatever the
//! configured command does is up to it.

pub mod excerpt;
pub mod prompt;
pub mod redact;
pub mod runner;

pub use prompt::{Excerpt, Msg, PromptCtx, RepoShas};
pub use runner::{Handle, Job, Outcome, Request, Subs};
