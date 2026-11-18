//! The Diagram playground: a few ways to draw one code change, side by side, each animated over
//! one shared example ("Vouchers at checkout") and read from one of four data schemas.
//!
//! - [`schema`]: the four schemas and their samples; specs in `schema/`.
//! - [`motion`]: the timeline clock and the values it moves without rebuilding views.
//! - [`style`]: the [`Style`] trait and the [`registry`] of styles, in the rail's order.
//! - [`Lab`]: the page's state, which a host app draws and ticks.

mod lab;
pub mod motion;
pub mod schema;
pub mod style;
pub mod styles;

#[cfg(test)]
mod tests;

pub use lab::{Lab, SCRUB_W, SPEEDS, Scrub};
pub use motion::{Clock, Rig, Tick};
pub use schema::{SchemaId, samples};
pub use style::{Act, Cx, Style, registry};
