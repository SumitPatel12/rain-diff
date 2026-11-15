//! `cargo xtask icon [--svg <big.svg>] [--small <small.svg>]`: copies the given sources to
//! `assets/icon/rain.svg` and `rain-small.svg`, then renders `assets/icon/rain.ico`. The small
//! variant draws the 16 and 24 px images; without one the big source draws every size.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use resvg::{tiny_skia, usvg};

const SMALL_SIZES: [u32; 2] = [16, 24];
const BIG_SIZES: [u32; 5] = [32, 48, 64, 128, 256];

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut svg = None;
    let mut small = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--svg" => svg = it.next().map(PathBuf::from),
            "--small" => small = it.next().map(PathBuf::from),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let dir = root.join("assets").join("icon");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let big_dest = dir.join("rain.svg");
    let small_dest = dir.join("rain-small.svg");
    if let Some(src) = svg {
        copy(&src, &big_dest)?;
        if small.is_none() {
            let _ = fs::remove_file(&small_dest);
        }
    }
    if let Some(src) = small {
        copy(&src, &small_dest)?;
    }
    let big = load(&big_dest)?;
    let small = if small_dest.exists() { load(&small_dest)? } else { big.clone() };

    let mut icon = ico::IconDir::new(ico::ResourceType::Icon);
    for (tree, size) in SMALL_SIZES.iter().map(|s| (&small, *s)).chain(BIG_SIZES.iter().map(|s| (&big, *s))) {
        let png = render(tree, size)?;
        let image = ico::IconImage::read_png(Cursor::new(png)).map_err(|e| e.to_string())?;
        icon.add_entry(ico::IconDirEntry::encode(&image).map_err(|e| e.to_string())?);
    }
    let out = dir.join("rain.ico");
    let file = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    icon.write(file).map_err(|e| e.to_string())?;
    println!("wrote {} ({} sizes); rebuild Rain to embed it", out.display(), icon.entries().len());
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if fs::canonicalize(from).ok() == fs::canonicalize(to).ok() {
        return Ok(());
    }
    fs::copy(from, to)
        .map(drop)
        .map_err(|e| format!("copy {} to {}: {e}", from.display(), to.display()))
}

fn load(path: &Path) -> Result<usvg::Tree, String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    usvg::Tree::from_data(&data, &usvg::Options::default()).map_err(|e| format!("{}: {e}", path.display()))
}

fn render(tree: &usvg::Tree, size: u32) -> Result<Vec<u8>, String> {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("empty icon size")?;
    let view = tree.size();
    let scale = size as f32 / view.width().max(view.height());
    let (dx, dy) = (
        (size as f32 - view.width() * scale) / 2.0,
        (size as f32 - view.height() * scale) / 2.0,
    );
    let transform = tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, dx, dy);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}
