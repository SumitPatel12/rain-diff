//! Schema S: a hand-written explanation as an explicit timeline, in the manner of Manim. Shapes
//! placed in the scene's own points, beats that act on them at given seconds, and camera keys.
//! Spec: `schema/s.schema.json`.

use serde::{Deserialize, Serialize};

use super::Status;

pub const RAW: &str = include_str!("../../samples/s.json");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub title: String,
    /// The scene's own width and height, in points.
    pub size: [f32; 2],
    pub shapes: Vec<SceneShape>,
    pub beats: Vec<Beat>,
    #[serde(default)]
    pub camera: Vec<SceneCamera>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeKind {
    Box,
    Text,
    Path,
    Dot,
    Arrow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneShape {
    pub id: String,
    pub kind: ShapeKind,
    /// Top-left for boxes and text, the start for paths, the centre for dots.
    pub at: [f32; 2],
    #[serde(default)]
    pub props: Props,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Props {
    pub w: Option<f32>,
    pub h: Option<f32>,
    pub r: Option<f32>,
    pub text: Option<String>,
    /// For paths and arrows, in scene points.
    pub points: Vec<[f32; 2]>,
    /// What the change did to it; picks the colour. Absent is the same as `same`.
    pub tone: Option<Status>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verb {
    /// Grows in.
    Create,
    /// Strokes draw on; text types on.
    Write,
    /// Changes size or shape.
    Transform,
    /// Travels to `to`.
    Move,
    /// Opacity from `to[0]` to `to[1]`.
    Fade,
    /// The camera frames it and the rest dims; an empty target ends focus.
    Focus,
    /// A dot runs along it.
    Pulse,
    /// Its label becomes `into`.
    Morph,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    /// Seconds from the start.
    pub t: f32,
    pub actions: Vec<Action>,
    /// What the beat shows, in a sentence; `[[id]]` names a shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Action {
    #[serde(rename = "do")]
    pub verb: Verb,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
    #[serde(default = "half")]
    pub dur: f32,
}

fn half() -> f32 {
    0.5
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneCamera {
    pub t: f32,
    /// In scene points.
    pub center: [f32; 2],
    pub zoom: f32,
}

impl Scene {
    pub fn shape(&self, id: &str) -> Option<&SceneShape> {
        self.shapes.iter().find(|s| s.id == id)
    }

    /// When the last action ends.
    pub fn duration(&self) -> f32 {
        self.beats
            .iter()
            .flat_map(|b| b.actions.iter().map(move |a| b.t + a.dur))
            .fold(0.0, f32::max)
    }
}
