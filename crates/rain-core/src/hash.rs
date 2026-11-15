use crate::diff::{FileDiff, LineKind};

/// blake3 hex over each line's kind byte and bytes; changes when the file's diff changes, so a
/// viewed mark can expire.
pub fn diff_content_hash(f: &FileDiff) -> String {
    let mut h = blake3::Hasher::new();
    for (i, rec) in f.lines.iter().enumerate() {
        let kind = match rec.kind {
            LineKind::Context => b' ',
            LineKind::Added => b'+',
            LineKind::Removed => b'-',
        };
        h.update(&[kind]);
        h.update(f.line_text(i as u32).as_bytes());
        h.update(b"\n");
    }
    h.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileDiffBuilder, FileStatus};

    fn file(kind: LineKind, text: &str) -> FileDiff {
        let mut b = FileDiffBuilder::new(0, "a", FileStatus::Modified);
        b.push_line(LineKind::Context, 1, 1, "x");
        b.push_line(kind, 2, 2, text);
        b.finish()
    }

    #[test]
    fn hash_tracks_kind_and_text() {
        let a = diff_content_hash(&file(LineKind::Added, "y"));
        assert_eq!(a.len(), 64);
        assert_eq!(a, diff_content_hash(&file(LineKind::Added, "y")));
        assert_ne!(a, diff_content_hash(&file(LineKind::Removed, "y")));
        assert_ne!(a, diff_content_hash(&file(LineKind::Added, "z")));
    }
}
