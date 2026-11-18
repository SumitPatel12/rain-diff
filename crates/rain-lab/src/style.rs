//! A diagram style: what it is called, what it reads, how it lays itself out and animates (its
//! [`Rig`]) and how it draws (its view). [`registry`] lists every style in order.

use rain_diagram::DiagramStyle;
use rune::El;

use crate::motion::Rig;
use crate::schema::{Samples, SchemaId};

/// What a style's press asks of the playground (see [`Cx::act`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// The style's own selection (a frame to zoom into, a node to open); the rig is rebuilt
    /// for it.
    Pick(Option<String>),
    /// Jump the timeline to these seconds.
    Seek(f32),
    /// Step back (-1) or forward (+1).
    Step(i32),
}

/// What a style builds with.
pub struct Cx<'a> {
    pub samples: &'static Samples,
    /// The schema picked for this style (one of its [`Style::schemas`]).
    pub schema: SchemaId,
    /// Rain's component kit for the scheme on screen: text colours, surfaces, states.
    pub kit: &'a rune_kit::Kit,
    /// Rain's diagram colours for the scheme on screen: cards, edges, added, modified, removed.
    pub ink: &'a DiagramStyle,
    /// The canvas, in points.
    pub size: [f32; 2],
    /// The OS asks for reduced motion: draw end states, no springs.
    pub reduced: bool,
    /// The style's own selection ([`Act::Pick`]).
    pub pick: Option<&'a str>,
    /// Makes `e` send `act` to the playground when pressed.
    pub act: &'a dyn Fn(El, Act) -> El,
}

impl Cx<'_> {
    pub fn act(&self, e: El, act: Act) -> El {
        (self.act)(e, act)
    }
}

pub trait Style: Sync {
    /// Its place in the rail, from 1: the registry's order.
    fn number(&self) -> u8 {
        registry().iter().position(|s| s.id() == self.id()).map_or(0, |i| i as u8 + 1)
    }
    /// Stable, for keys and scenes: `tour`, `lineage`.
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// One line on what it shows.
    fn pitch(&self) -> &'static str;
    /// The schemas it reads, the default first; the playground offers a switch when there are
    /// more than one.
    fn schemas(&self) -> &'static [SchemaId];
    /// A few sentences on how to read it.
    fn how_to_read(&self) -> Option<&'static str> {
        None
    }
    /// False for a placeholder still to be built.
    fn ready(&self) -> bool {
        true
    }
    /// Lays the style out for `cx.size` and puts its motion on a timeline. Called once per
    /// style, schema, pick and size, never per frame.
    fn rig(&self, cx: &Cx) -> Rig;
    /// Draws it, binding the rig's values. Called when the view rebuilds (a pick, a resize, a
    /// theme change), never per frame.
    fn view(&self, cx: &Cx, rig: &Rig) -> El;
}

/// Every style, in the rail's order.
pub fn registry() -> &'static [&'static dyn Style] {
    crate::styles::ALL
}
