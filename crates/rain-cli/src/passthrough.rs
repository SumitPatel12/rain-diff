//! Rune's own tooling flags (`--snap`, `--scene`, `--tree`, ...). Rune reads them from the process
//! arguments itself, so they are only taken out of what clap sees.

const VALUE: &[&str] = &[
    "--snap",
    "--scene",
    "--variant",
    "--at",
    "--size",
    "--scale",
    "--theme",
    "--scroll-bars",
    "--script",
    "--frames",
    "--start",
    "--fps",
    "--out",
    "--video",
    "--duration",
    "--keys-theme",
    "--desktop",
    "--tree",
    "--trace",
    "--strip",
    "--crop",
    "--cols",
    "--every",
    "--window",
    "--snap-on-patch",
    "--hot-control",
    // Rain's `--chrome zed|islands`; the app reads it from the process arguments.
    "--chrome",
];
const FLAG: &[&str] = &[
    "--whole-window",
    "--pointer",
    "--native",
    "--trace-all",
    "--list-scenes",
    "--first-pixels",
    "--list-windows",
    "--hot",
];
/// Flags whose value is optional.
const OPTIONAL: &[&str] = &["--keys", "--playground"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuneArgs {
    pub args: Vec<String>,
    /// Rune renders offscreen or replays a script instead of opening a window (`Args::headless`).
    pub headless: bool,
}

impl RuneArgs {
    fn has(&self, flag: &str) -> bool {
        self.args.iter().any(|a| a == flag)
    }
}

/// Splits argv (without the program) into clap's arguments and Rune's.
pub fn split(argv: &[String]) -> (Vec<String>, RuneArgs) {
    let mut ours = Vec::new();
    let mut rune = RuneArgs::default();
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        let takes = if VALUE.contains(&a) {
            1
        } else if OPTIONAL.contains(&a) {
            usize::from(argv.get(i + 1).is_some_and(|v| !v.starts_with('-')))
        } else if FLAG.contains(&a) {
            0
        } else {
            ours.push(argv[i].clone());
            i += 1;
            continue;
        };
        let end = (i + 1 + takes).min(argv.len());
        rune.args.extend_from_slice(&argv[i..end]);
        i = end;
    }
    let windowless = [
        "--snap",
        "--tree",
        "--video",
        "--strip",
        "--list-scenes",
        "--first-pixels",
        "--list-windows",
        "--desktop",
        "--snap-on-patch",
    ];
    let scripted = ["--script", "--trace", "--frames", "--duration", "--out"];
    rune.headless = windowless.iter().any(|f| rune.has(f)) || (!rune.has("--native") && scripted.iter().any(|f| rune.has(f)));
    (ours, rune)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn rune_flags_are_split_off_with_their_values() {
        let (ours, rune) = split(&s(&[
            "HEAD~1",
            "--snap",
            "out.png",
            "--scene",
            "light",
            "--merge-base",
            "--scale",
            "1.5",
            "HEAD",
        ]));
        assert_eq!(ours, s(&["HEAD~1", "--merge-base", "HEAD"]));
        assert_eq!(rune.args, s(&["--snap", "out.png", "--scene", "light", "--scale", "1.5"]));
        assert!(rune.headless);
    }

    #[test]
    fn headless_only_for_offscreen_or_scripted_runs() {
        assert!(!split(&s(&["--theme", "dark", "--size", "1366x768"])).1.headless);
        assert!(split(&s(&["--script", "a.json"])).1.headless);
        assert!(!split(&s(&["--script", "a.json", "--native"])).1.headless);
        let (ours, rune) = split(&s(&["--keys", "--fixture", "m.patch"]));
        assert_eq!(ours, s(&["--fixture", "m.patch"]));
        assert_eq!(rune.args, s(&["--keys"]));
    }

    #[test]
    fn chrome_takes_its_value_out_of_what_clap_sees() {
        let (ours, rune) = split(&s(&["--snap", "a.png", "--chrome", "islands", "HEAD"]));
        assert_eq!(ours, s(&["HEAD"]));
        assert_eq!(rune.args, s(&["--snap", "a.png", "--chrome", "islands"]));
    }
}
