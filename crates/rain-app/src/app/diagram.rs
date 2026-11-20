//! The Map overlay: the overview's diagrams over the reader, nodes shaded by how much of their
//! files is viewed, badged with their threads, and commentable.

use std::collections::BTreeMap;

use rain_core::command::Command;
use rain_core::keymap::{Chord, ChordKey};
use rain_core::overview::{GraphFileRef, OverviewDiagrams};
use rain_diagram::{DiagramEvent, ItemId, NodeMark, Zoom};
use rain_diffview::LineRef;
use rain_lab::{Lab, Tick, motion::FRAME};
use rune::Tx;

use super::App;
use super::comments::revision_side;
use super::layout::Overlay;

pub struct DiagramPage {
    pub target: usize,
    pub zoom: Zoom,
    pub hover: Option<ItemId>,
    /// The node the side pane shows and `c`/`?` comment on: set only by pressing a node.
    pub selected: Option<String>,
    /// The diagram body's width when last laid out; a change re-fits.
    pub seen_width: f32,
    /// The overview's animated diagrams, read when it loads; each lays itself out the first
    /// time its tab shows.
    pub lab: Option<Lab>,
    /// The registry indices of the styles the overview carries, in tab order.
    pub lab_styles: Vec<usize>,
    /// What `lab` was read from, so a reload with the same diagrams keeps it.
    lab_source: Option<OverviewDiagrams>,
    /// An animated diagram's tab is showing rather than a view of the graph.
    pub lab_on: bool,
    /// Its canvas as last laid out.
    pub lab_size: [f32; 2],
}

impl Default for DiagramPage {
    fn default() -> DiagramPage {
        DiagramPage {
            target: 0,
            zoom: Zoom::Fit,
            hover: None,
            selected: None,
            seen_width: 0.0,
            lab: None,
            lab_styles: Vec::new(),
            lab_source: None,
            lab_on: false,
            lab_size: [0.0, 0.0],
        }
    }
}

/// One Map tab: an animated diagram (a registry index) or a view of the graph (a target).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapTab {
    Lab(usize),
    View(usize),
}

impl App {
    /// Whether the session has a diagram to show.
    pub fn has_map(&self) -> bool {
        !self.diagram.lab_styles.is_empty() || self.layouts.iter().any(Option::is_some)
    }

    /// The Map's tabs in order: the animated diagrams, then the graph's views that draw.
    pub fn map_tabs(&self) -> Vec<MapTab> {
        let labs = self.diagram.lab_styles.iter().map(|&i| MapTab::Lab(i));
        let views = (0..self.layouts.len()).filter(|&i| self.layouts[i].is_some()).map(MapTab::View);
        labs.chain(views).collect()
    }

    /// The tab showing.
    pub fn map_tab(&self) -> MapTab {
        match &self.diagram.lab {
            Some(l) if self.diagram.lab_on => MapTab::Lab(l.style_index()),
            _ => MapTab::View(self.diagram.target),
        }
    }

    /// Reads the overview's animated diagrams; the same ones again keep their lab and tab. A
    /// first set opens the Map on its first diagram.
    pub(crate) fn rebuild_map_lab(&mut self) {
        let src = self.overview.as_ref().and_then(|l| l.document.diagrams.clone());
        let d = &mut self.diagram;
        if src == d.lab_source && (d.lab.is_some() || src.is_none()) {
            return;
        }
        let fresh = d.lab_source.is_none();
        d.lab_source = src;
        d.lab = None;
        d.lab_styles.clear();
        if let Some(src) = &d.lab_source {
            let (data, styles) = rain_lab::review_diagrams(src);
            if let Some(&first) = styles.first() {
                d.lab = Some(Lab::over(data, first));
                d.lab_styles = styles;
            }
        }
        d.lab_on = d.lab.is_some() && (fresh || d.lab_on);
    }

    /// The animated diagram's canvas was laid out at `size`; a new size lays it out again.
    pub fn map_lab_resized(&mut self, size: [f32; 2]) {
        let old = self.diagram.lab_size;
        if (size[0] - old[0]).abs() >= 1.0 || (size[1] - old[1]).abs() >= 1.0 {
            self.diagram.lab_size = size;
        }
    }

    /// Shows animated diagram `style` (a registry index), from its start.
    pub fn pick_map_lab(&mut self, style: usize) {
        let Some(lab) = &mut self.diagram.lab else { return };
        if !self.diagram.lab_styles.contains(&style) {
            return;
        }
        if lab.style_index() != style || !self.diagram.lab_on {
            lab.select(style);
        }
        self.diagram.lab_on = true;
        self.diagram.hover = None;
    }

    pub fn pick_map_tab(&mut self, t: MapTab) {
        match t {
            MapTab::Lab(i) => self.pick_map_lab(i),
            MapTab::View(i) => self.pick_map_view(i),
        }
    }

    /// One frame of a playing diagram on the Map: its values move without a new view. Off the
    /// Map, or on another tab, it pauses and asks for nothing.
    pub(crate) fn map_lab_poll(&mut self, tx: &mut Tx) -> bool {
        let shown = self.diagram.lab_on && self.layout.is_open(Overlay::Map) && self.phase == super::Phase::Ready;
        let Some(lab) = &mut self.diagram.lab else { return false };
        if !lab.playing() {
            return false;
        }
        if !shown {
            lab.pause();
            return true;
        }
        match lab.tick(self.rest.clock) {
            Tick::Idle => false,
            Tick::Moved | Tick::Stepped => {
                tx.wake_after(FRAME);
                false
            }
            Tick::Ended => true,
        }
    }

    /// Keys on an animated diagram: Space plays or pauses, Left and Right step, Home restarts.
    pub(crate) fn map_lab_key(&mut self, chord: Option<Chord>) -> bool {
        let Some(c) = chord.filter(|c| !c.ctrl && !c.alt && !c.shift) else {
            return false;
        };
        if !self.diagram.lab_on || self.layout.top() != Some(Overlay::Map) {
            return false;
        }
        let Some(lab) = &mut self.diagram.lab else { return false };
        match c.key {
            ChordKey::Space => lab.toggle(),
            ChordKey::Left => lab.step_by(false),
            ChordKey::Right => lab.step_by(true),
            ChordKey::Home => lab.restart(),
            _ => return false,
        }
        true
    }

    /// `m` and the toolbar's Map button; opens fitted to the window.
    pub fn toggle_map(&mut self) {
        if !self.has_map() {
            self.status = "No overview diagram for this session".into();
            return;
        }
        if self.toggle_overlay(Overlay::Map) {
            self.diagram.zoom = Zoom::Fit;
            self.status = crate::keys::with_hints(
                "Map · click a node to select it · click it again to open its first hunk",
                &[(Command::Comment, "comment"), (Command::Ask, "question")],
            );
        }
    }

    /// Runs a jump the reader didn't ask for (a deferred goto, a restored place): an open Map
    /// stays open, at its place among the overlays.
    pub(crate) fn keep_map(&mut self, jump: impl FnOnce(&mut App)) {
        let at = self.layout.overlays.iter().position(|o| *o == Overlay::Map);
        jump(self);
        if let Some(i) = at
            && !self.layout.is_open(Overlay::Map)
        {
            let i = i.min(self.layout.overlays.len());
            self.layout.overlays.insert(i, Overlay::Map);
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
        self.diagram.lab_on = false;
        if let Some(l) = &mut self.diagram.lab {
            l.pause();
        }
    }

    /// `}` / `{`: the next or previous Map tab, wrapping round.
    pub fn step_map_view(&mut self, forward: bool) {
        if !self.layout.is_open(Overlay::Map) || !self.has_map() {
            self.status = "Map views: m opens the Map".into();
            return;
        }
        let tabs = self.map_tabs();
        let at = tabs.iter().position(|&t| t == self.map_tab()).unwrap_or(0);
        let n = tabs.len();
        let next = if forward { (at + 1) % n } else { (at + n - 1) % n };
        self.pick_map_tab(tabs[next]);
    }

    /// The side pane's ✕ and Escape: the selection goes and stays gone until a node is pressed.
    pub fn close_map_pane(&mut self) -> bool {
        self.diagram.selected.take().is_some()
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

    /// A hover only lights; pressing a node selects it for the side pane and `c`/`?` (pressing
    /// it again opens its first cited hunk); pressing any other item opens its first cited hunk.
    pub fn diagram_event(&mut self, e: DiagramEvent, tx: &mut Tx) {
        match e {
            DiagramEvent::Hover(h) => self.diagram.hover = h,
            DiagramEvent::Zoom(z) => self.diagram.zoom = Zoom::Custom(z),
            DiagramEvent::Node(id) if self.diagram.selected.as_deref() != Some(id.as_str()) => {
                self.diagram.selected = Some(id);
            }
            _ => self.open_refs(&e, tx),
        }
    }

    /// Opens the first hunk node `id` cites (the picker's node rows).
    pub fn open_node(&mut self, id: String, tx: &mut Tx) {
        self.open_refs(&DiagramEvent::Node(id), tx);
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
