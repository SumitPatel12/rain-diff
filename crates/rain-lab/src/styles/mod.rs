//! The styles, in the order the playground lists them. A style is one file here and one line
//! in [`ALL`].

mod beats;
mod bend;
mod flowchart;
mod icicle;
mod lineage;
pub mod paint;
mod tour;
mod tour_map;

use crate::style::Style;

pub(crate) static ALL: &[&dyn Style] = &[&tour::Tour, &flowchart::Flowchart, &lineage::Lineage, &icicle::Icicle];

#[cfg(test)]
pub(crate) use lineage::Flow;
