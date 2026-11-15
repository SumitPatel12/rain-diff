//! `rain overview render`: every view of an overview drawn headless to PNGs, dark and light, so an
//! agent can check a diagram's layout without a window or a screen capture.

use std::path::{Path, PathBuf};

use rain_diagram::{Diagram, DiagramStyle, Layout, Zoom};
use rain_overview::Loaded;
use rune::{AppSpec, Cx, El, col, row, text};
use rune_kit::Kit;

const HEADER: f32 = 40.0;

struct Snap {
    title: String,
    layout: Layout,
    size: (f32, f32),
}

fn view(s: &Option<Snap>, cx: &mut Cx<Option<Snap>>) -> El {
    let dark = cx.system_dark();
    let style = DiagramStyle::for_theme(dark);
    let Some(s) = s else { return col() };
    let k = Kit::neutral(dark);
    let header = row()
        .w_full()
        .h(HEADER)
        .px(16.0)
        .items_center()
        .fill(k.c.layer_01)
        .child(text(s.title.clone()).font(k.ty.ui_medium).color(k.c.text_base));
    let body = Diagram::<Option<Snap>>::new(&s.layout)
        .style(style)
        .zoom(Zoom::Fit)
        .viewport(s.size.0, s.size.1 - HEADER)
        .build(&k);
    col()
        .w_full()
        .h_full()
        .fill(style.background)
        .child(header)
        .child(col().w_full().grow(1.0).min_h(0.0).child(body))
}

/// `01-architecture-dark.png`: the view's position, its title in lowercase ASCII words, the theme.
fn file_name(index: usize, title: &str, theme: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let slug = if words.is_empty() { "view".to_owned() } else { words.join("-") };
    format!("{:02}-{slug}-{theme}.png", index + 1)
}

/// Draws each view `loaded` can render at `size` points, dark then light, into `out`; the PNGs
/// written, in order.
pub fn render_overview(loaded: &Loaded, out: &Path, size: (u32, u32)) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let (fonts, families) = crate::fonts();
    let mut written = Vec::new();
    for (i, target) in loaded.targets.iter().enumerate() {
        let Some(layout) = rain_diagram::render(&loaded.document.graph, target) else {
            continue;
        };
        for theme in ["dark", "light"] {
            let path = out.join(file_name(i, &target.title, theme));
            let snap = Snap {
                title: format!("{} · {}", loaded.document.title, target.title),
                layout: layout.clone(),
                size: (size.0 as f32, size.1 as f32),
            };
            let spec = AppSpec {
                fonts,
                families,
                ..AppSpec::new("Rain overview", snap.size, || None, view)
            };
            let args = [
                "--snap".to_owned(),
                path.to_string_lossy().into_owned(),
                "--size".to_owned(),
                format!("{}x{}", size.0, size.1),
                "--theme".to_owned(),
                theme.to_owned(),
            ];
            rune::App::new(spec)
                .init(move || Some(snap))
                .args(args)
                .try_run()
                .map_err(|e| format!("cannot draw {}: {e}", path.display()))?;
            if !path.is_file() {
                return Err(format!("drawing {} wrote nothing", path.display()));
            }
            written.push(path);
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_ordered_ascii_slugs() {
        assert_eq!(file_name(0, "Architecture", "dark"), "01-architecture-dark.png");
        assert_eq!(
            file_name(11, "Data flow: Add voucher → account", "light"),
            "12-data-flow-add-voucher-account-light.png"
        );
        assert_eq!(file_name(2, "→", "dark"), "03-view-dark.png");
    }

    #[test]
    fn every_view_of_the_sample_is_drawn_in_both_themes() {
        let path = crate::tests::workspace("fixtures/overview/sample-overview-schema.json");
        let loaded = rain_overview::load(&path).unwrap();
        let out = crate::tests::temp_dir();
        let written = render_overview(&loaded, &out, (800, 500)).unwrap();
        let drawable = loaded
            .targets
            .iter()
            .filter(|t| rain_diagram::render(&loaded.document.graph, t).is_some())
            .count();
        assert!(drawable > 0);
        assert_eq!(written.len(), drawable * 2);
        for p in &written {
            let png = std::fs::read(p).unwrap();
            assert!(png.starts_with(b"\x89PNG"), "{}", p.display());
        }
        let _ = std::fs::remove_dir_all(&out);
    }
}
