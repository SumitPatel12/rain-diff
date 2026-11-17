//! The Map overlay: the overview's diagrams over the reader, nodes shaded by how much of their
//! files is viewed, badged with their threads, and commentable.

use std::collections::BTreeMap;

use rain_core::overview::GraphFileRef;
use rain_diagram::{DiagramEvent, ItemId, NodeMark, Zoom};
use rain_diffview::LineRef;
use rune::Tx;

use super::App;
use super::comments::revision_side;
use super::layout::Overlay;

pub struct DiagramPage {
    pub target: usize,
    pub zoom: Zoom,
    pub hover: Option<ItemId>,
    /// The node the side pane shows and `c`/`?` comment on.
    pub selected: Option<String>,
    /// The diagram body's width when last laid out; a change re-fits.
    pub seen_width: f32,
}

impl Default for DiagramPage {
    fn default() -> DiagramPage {
        DiagramPage {
            target: 0,
            zoom: Zoom::Fit,
            hover: None,
            selected: None,
            seen_width: 0.0,
        }
    }
}

impl App {
    /// Whether the session has a diagram to show.
    pub fn has_map(&self) -> bool {
        self.layouts.iter().any(Option::is_some)
    }

    /// `m` and the toolbar's Map button; opens fitted to the window.
    pub fn toggle_map(&mut self) {
        if !self.has_map() {
            self.status = "No overview diagram for this session".into();
            return;
        }
        if self.toggle_overlay(Overlay::Map) {
            self.diagram.zoom = Zoom::Fit;
            self.status = "Map · hover a node to see it · click to open its first hunk · c comment · ? question".into();
        }
    }

    /// The Map body was laid out `w` points wide: a new width fits the diagram again.
    pub fn map_resized(&mut self, w: f32) {
        if (w - self.diagram.seen_width).abs() >= 1.0 {
            self.diagram.seen_width = w;
            self.diagram.zoom = Zoom::Fit;
        }
    }

    pub fn pick_map_view(&mut self, i: usize) {
        self.diagram.target = i;
        self.diagram.hover = None;
        self.diagram.zoom = Zoom::Fit;
    }

    /// `}` / `{`: the next or previous Map view that draws, wrapping round.
    pub fn step_map_view(&mut self, forward: bool) {
        if !self.layout.is_open(Overlay::Map) || !self.has_map() {
            self.status = "Map views: m opens the Map".into();
            return;
        }
        let shown: Vec<usize> = (0..self.layouts.len()).filter(|&i| self.layouts[i].is_some()).collect();
        let at = shown.iter().position(|&i| i == self.diagram.target).unwrap_or(0);
        let n = shown.len();
        let next = if forward { (at + 1) % n } else { (at + n - 1) % n };
        self.pick_map_view(shown[next]);
    }

    /// Viewed share and open threads per graph node, from the files each node cites.
    pub fn map_marks(&self) -> BTreeMap<String, NodeMark> {
        let Some(l) = &self.overview else { return BTreeMap::new() };
        l.document
            .graph
            .nodes
            .iter()
            .map(|n| {
                let files: Vec<usize> = n.files.iter().filter_map(|f| self.index_of(&f.path)).collect();
                let viewed = if files.is_empty() {
                    0.0
                } else {
                    files.iter().filter(|&&g| self.is_viewed(g)).count() as f32 / files.len() as f32
                };
                let threads = self.node_threads(&n.id).len();
                (n.id.clone(), NodeMark { viewed, threads })
            })
            .collect()
    }

    /// The file references of a pressed item.
    fn refs_of(&self, e: &DiagramEvent) -> Vec<GraphFileRef> {
        let Some(l) = &self.overview else { return Vec::new() };
        let g = &l.document.graph;
        let tables = g.data_model.as_ref().map(|m| m.tables.as_slice()).unwrap_or_default();
        match e {
            DiagramEvent::Node(id) => g.nodes.iter().find(|n| &n.id == id).map(|n| n.files.clone()),
            DiagramEvent::Edge(id) => g.edges.iter().find(|x| &x.id == id).map(|x| x.files.clone()),
            DiagramEvent::Message { flow, id } => g
                .flows
                .iter()
                .find(|f| &f.id == flow)
                .and_then(|f| f.messages.iter().find(|m| &m.id == id))
                .map(|m| m.files.clone()),
            DiagramEvent::Table(t) | DiagramEvent::Column { table: t, .. } | DiagramEvent::Index { table: t, .. } => {
                tables.iter().find(|x| &x.id == t).map(|x| x.files.clone())
            }
            _ => None,
        }
        .unwrap_or_default()
    }

    /// A hover selects the node under the pointer for the side pane and `c`/`?` (it stays
    /// selected after the pointer leaves); a press opens the item's first cited hunk.
    pub fn diagram_event(&mut self, e: DiagramEvent, tx: &mut Tx) {
        match e {
            DiagramEvent::Hover(h) => {
                if let Some(node) = h.as_ref().and_then(rain_diagram::node_of) {
                    self.diagram.selected = Some(node.to_owned());
                }
                self.diagram.hover = h;
            }
            DiagramEvent::Zoom(z) => self.diagram.zoom = Zoom::Custom(z),
            _ => self.open_refs(&e, tx),
        }
    }

    /// Opens the first hunk the selected node cites.
    pub fn open_selected_node(&mut self, tx: &mut Tx) {
        if let Some(id) = self.diagram.selected.clone() {
            self.open_refs(&DiagramEvent::Node(id), tx);
        }
    }

    fn open_refs(&mut self, e: &DiagramEvent, tx: &mut Tx) {
        let refs = self.refs_of(e);
        let target = refs.iter().find_map(|r| self.index_of(&r.path).map(|g| (g, r.clone())));
        match target {
            Some((file, r)) => {
                self.close_overlay(Overlay::Map);
                match r.start_line {
                    Some(n) => self.goto_line(file, LineRef::new(revision_side(r.revision), n), tx),
                    None => self.goto_file(file, tx),
                }
                self.status = format!("From the diagram: {}", self.keys[file]);
            }
            None => self.status = format!("{e:?} cites no file in this diff"),
        }
    }
}
