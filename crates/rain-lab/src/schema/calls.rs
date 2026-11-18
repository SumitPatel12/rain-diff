//! Schema C: where the time goes. Call trees with each frame's own work before and after the
//! change: the change's entry points, and more trees to compare such as one query's under a
//! given load. Spec: `schema/c.schema.json`.

use serde::{Deserialize, Serialize};

use super::Status;

pub const RAW: &str = include_str!("../../samples/c.json");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CallTrees {
    pub title: String,
    /// The call tree under the entry points, with each frame's own work before and after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<CallFrame>,
    /// More call trees to compare before and after, such as one query's under a given load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<Profile>,
}

/// A named call tree: what was measured, and under what conditions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub title: String,
    pub calls: CallFrame,
}

/// One frame of a call tree: its own work (self time, in milliseconds per request) before and
/// after the change; 0 before is a new call, 0 after a call that is gone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CallFrame {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub before: f32,
    #[serde(default)]
    pub after: f32,
    #[serde(default)]
    pub children: Vec<CallFrame>,
}

impl CallFrame {
    /// Its work and its callees', before and after.
    pub fn total(&self) -> (f32, f32) {
        self.children
            .iter()
            .map(CallFrame::total)
            .fold((self.before, self.after), |a, b| (a.0 + b.0, a.1 + b.1))
    }

    pub fn status(&self) -> Status {
        let (b, a) = self.total();
        match (b > 0.0, a > 0.0) {
            (false, true) => Status::Added,
            (true, false) => Status::Removed,
            _ if (a - b).abs() > 1e-3 => Status::Changed,
            _ => Status::Same,
        }
    }

    /// Depth-first, parents before children, with each frame's depth.
    pub fn walk(&self) -> Vec<(usize, &CallFrame)> {
        fn go<'a>(f: &'a CallFrame, d: usize, out: &mut Vec<(usize, &'a CallFrame)>) {
            out.push((d, f));
            for c in &f.children {
                go(c, d + 1, out);
            }
        }
        let mut out = Vec::new();
        go(self, 0, &mut out);
        out
    }
}
