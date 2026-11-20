//! File and folder icons from the Material Icon Theme: the icon the theme gives a file name, by
//! its own rules, drawn from the theme's full-colour SVG. A repository root keeps a solid folder
//! glyph in the repo's colour.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use rune::{El, Icon, Image, ImageCache, SurfaceFit, Waker, el};

use crate::material::{FILE, FILE_EXTENSIONS, FILE_NAMES, FOLDER, FOLDER_OPEN, ICONS};

/// A closed folder, solid.
pub static REPO: Icon = Icon {
    name: "rain-folder",
    view: 16.0,
    body: r#"<path d="M1.5 4a1 1 0 0 1 1-1h3.7l1.5 1.6h5.8a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z"/>"#,
};

/// An open folder: the back tinted, the front solid.
pub static REPO_OPEN: Icon = Icon {
    name: "rain-folder-open",
    view: 16.0,
    body: r#"<path opacity="0.5" d="M1.5 4a1 1 0 0 1 1-1h3.7l1.5 1.6h4.8a1 1 0 0 1 1 1v1H1.5z"/><path d="M1.6 7.2h13.2a.5.5 0 0 1 .5.6l-1.1 4.5a1 1 0 0 1-1 .7H2.5a1 1 0 0 1-1-.8z"/>"#,
};

fn find(table: &[(&str, u8)], key: &str) -> Option<u8> {
    table.binary_search_by(|(k, _)| (*k).cmp(key)).ok().map(|i| table[i].1)
}

/// `name` (a file name, not a path) as the theme matches it: the whole name, then each dotted
/// suffix from the longest (`a.component.ts` tries `component.ts`, then `ts`), else a plain page.
fn icon_of(name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    find(&FILE_NAMES, &lower)
        .or_else(|| {
            lower
                .match_indices('.')
                .find_map(|(at, _)| find(&FILE_EXTENSIONS, &lower[at + 1..]))
        })
        .unwrap_or(FILE)
}

/// The theme's name for the icon of file `name`, as `rust` or `angular-component`.
pub fn icon_name(name: &str) -> &'static str {
    ICONS[icon_of(name) as usize].name
}

/// The icons as one window draws them: each SVG rasterized once per size, display scale and
/// light or dark variant, off the UI thread; a pending one draws nothing until its pixels land.
#[derive(Default)]
pub struct FileIcons {
    cache: RefCell<Option<ImageCache>>,
    scale: Cell<f32>,
    light: Cell<bool>,
    images: RefCell<HashMap<(u8, bool, u32), Image>>,
}

impl FileIcons {
    /// Rasterizes on a cache that wakes the window through `waker` when an icon is ready.
    pub fn start(&mut self, waker: Waker) {
        *self.cache.get_mut() = Some(ImageCache::new(waker));
        self.images.get_mut().clear();
    }

    /// The window's device pixels per point, and whether its background is light.
    pub fn set_env(&self, scale: f32, light: bool) {
        self.scale.set(scale);
        self.light.set(light);
    }

    /// The icon of file `name`, `size` points square.
    pub fn file(&self, name: &str, size: f32) -> El {
        self.draw(icon_of(name), size).key("file-icon")
    }

    /// A folder, open or closed, `size` points square.
    pub fn folder(&self, open: bool, size: f32) -> El {
        self.draw(if open { FOLDER_OPEN } else { FOLDER }, size).key("folder-icon")
    }

    /// The decoded image behind [`FileIcons::file`], for checks that need its pixels.
    pub fn file_image(&self, name: &str, size: f32) -> Image {
        self.image(icon_of(name), size)
    }

    fn draw(&self, icon: u8, size: f32) -> El {
        let image = self.image(icon, size);
        el().size(size, size).pixels_fit(image.pixels(), SurfaceFit::Fit).a11y_hidden()
    }

    fn image(&self, icon: u8, size: f32) -> Image {
        let scale = if self.scale.get() > 0.0 { self.scale.get() } else { 1.0 };
        let svg = &ICONS[icon as usize];
        let light = self.light.get() && svg.light.is_some();
        let px = (size * scale).ceil() as u32;
        self.images
            .borrow_mut()
            .entry((icon, light, px))
            .or_insert_with(|| {
                let bytes = if light { svg.light.unwrap_or(svg.dark) } else { svg.dark };
                let mut cache = self.cache.borrow_mut();
                cache
                    .get_or_insert_with(|| ImageCache::new(std::sync::Arc::new(|| {})))
                    .svg(bytes, size, scale)
            })
            .clone()
    }
}
