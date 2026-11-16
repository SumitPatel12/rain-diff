use crate::diff::{FileDiff, LineKind};

/// blake3 hex over each line's kind byte and bytes; changes when the file's diff changes, so a
/// viewed mark can expire.
pub fn diff_content_hash(f: &FileDiff) -> String {
    // The same bytes as one update per piece, gathered so blake3 gets them in big chunks.
    const CHUNK: usize = 64 * 1024;
    let mut h = blake3::Hasher::new();
    let mut buf: Vec<u8> = Vec::with_capacity(CHUNK.min(f.text.len() + 2 * f.lines.len()));
    for (i, rec) in f.lines.iter().enumerate() {
        let kind = match rec.kind {
            LineKind::Context => b' ',
            LineKind::Added => b'+',
            LineKind::Removed => b'-',
        };
        let text = f.line_text(i as u32).as_bytes();
        if buf.len() + text.len() + 2 > CHUNK {
            h.update(&buf);
            buf.clear();
        }
        if text.len() + 2 > CHUNK {
            h.update(&[kind]);
            h.update(text);
            h.update(b"\n");
            continue;
        }
        buf.push(kind);
        buf.extend_from_slice(text);
        buf.push(b'\n');
    }
    h.update(&buf);
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

    /// The hash as first written: three updates per line. Viewed marks store it.
    fn per_line(f: &FileDiff) -> String {
        let mut h = blake3::Hasher::new();
        for (i, rec) in f.lines.iter().enumerate() {
            h.update(&[match rec.kind {
                LineKind::Context => b' ',
                LineKind::Added => b'+',
                LineKind::Removed => b'-',
            }]);
            h.update(f.line_text(i as u32).as_bytes());
            h.update(b"\n");
        }
        h.finalize().to_hex().to_string()
    }

    #[test]
    fn hash_is_the_per_line_hash_across_chunks_and_long_lines() {
        let mut b = FileDiffBuilder::new(0, "a", FileStatus::Modified);
        let long = "x".repeat(200_000);
        let kinds = [LineKind::Context, LineKind::Added, LineKind::Removed];
        for n in 1..20_000u32 {
            let text = match n {
                7 | 9_000 => long.clone(),
                _ => format!("line {n} {}", "ab".repeat((n % 50) as usize)),
            };
            b.push_line(kinds[(n % 3) as usize], n, n, &text);
        }
        let f = b.finish();
        assert_eq!(diff_content_hash(&f), per_line(&f));
        let empty = FileDiffBuilder::new(0, "e", FileStatus::Added).finish();
        assert_eq!(diff_content_hash(&empty), per_line(&empty));
    }
}
