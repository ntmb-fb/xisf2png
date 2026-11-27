//! xisf2png command-line interface.

use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use xisf2png::{FileStatus, Options};

enum Action {
    Help,
    Version,
    Run { opts: Options, ask: bool },
}

fn has_image_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "xisf" | "fits" | "fit" | "fts" | "png"
        )
    })
}

fn parse_positive_usize(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err("-j requires a positive integer".into()),
    }
}

fn parse_args<I>(args: I) -> Result<Action, String>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = args.into_iter().collect();

    let mut opts = Options {
        lookup: true,
        ..Options::default()
    };
    let mut input_dir: Option<PathBuf> = None;
    let mut output_dir: Option<PathBuf> = None;
    let mut ask = true;
    let mut after_dashdash = false;

    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        let as_str = raw.to_str();

        if !after_dashdash {
            if let Some(a) = as_str {
                match a {
                    "--" => {
                        after_dashdash = true;
                        i += 1;
                        continue;
                    }
                    "--recursive" | "-r" => {
                        opts.recursive = true;
                        i += 1;
                        continue;
                    }
                    "--overwrite" => {
                        opts.overwrite = true;
                        i += 1;
                        continue;
                    }
                    "--resize4k" | "-resize4k" => {
                        opts.resize4k = true;
                        i += 1;
                        continue;
                    }
                    "--png-only" => {
                        opts.png_only = true;
                        i += 1;
                        continue;
                    }
                    "--filename" | "--no-lookup" | "--offline" => {
                        opts.lookup = false;
                        i += 1;
                        continue;
                    }
                    "--no-ask" => {
                        ask = false;
                        i += 1;
                        continue;
                    }
                    "-j" | "--concurrency" => {
                        i += 1;
                        let val = args
                            .get(i)
                            .and_then(|s| s.to_str())
                            .ok_or_else(|| "-j requires a positive integer".to_string())?;
                        opts.concurrency = parse_positive_usize(val)?;
                        i += 1;
                        continue;
                    }
                    _ if a.starts_with("-j") => {
                        opts.concurrency = parse_positive_usize(&a[2..])?;
                        i += 1;
                        continue;
                    }
                    _ if a.starts_with("--concurrency=") => {
                        opts.concurrency = parse_positive_usize(&a["--concurrency=".len()..])?;
                        i += 1;
                        continue;
                    }
                    "--font" => {
                        i += 1;
                        match args.get(i) {
                            Some(f) if !f.is_empty() => opts.font = Some(PathBuf::from(f)),
                            _ => return Err("--font requires a path to a .ttf/.otf file".into()),
                        }
                        i += 1;
                        continue;
                    }
                    _ if a.starts_with("--font=") => {
                        let f = &a["--font=".len()..];
                        if f.is_empty() {
                            return Err("--font requires a path to a .ttf/.otf file".into());
                        }
                        opts.font = Some(PathBuf::from(f));
                        i += 1;
                        continue;
                    }
                    "--version" | "-V" => return Ok(Action::Version),
                    "--help" | "-h" | "/?" => return Ok(Action::Help),
                    _ if a.starts_with('-') => return Err(format!("Unknown option: {a}")),
                    _ => {}
                }
            }
        }

        // Positional: existing files are inputs to process; anything else is a
        // folder (input first, then output) unless it looks like a missing
        // image file.
        let path = PathBuf::from(raw);
        if path.is_file() {
            opts.files.push(path);
        } else if !path.exists() && has_image_extension(&path) {
            return Err(format!("File not found: {}", path.display()));
        } else if input_dir.is_none() && opts.files.is_empty() {
            input_dir = Some(path);
        } else if output_dir.is_none() {
            output_dir = Some(path);
        } else {
            return Err(format!("Unexpected argument: {}", path.display()));
        }
        i += 1;
    }

    // "xisf2png out_dir a.xisf": with explicit files the only folder that
    // makes sense is the output folder.
    if !opts.files.is_empty() && output_dir.is_none() {
        output_dir = input_dir.take();
    }
    // No input folder given: work on the current directory.
    opts.input_dir = input_dir.unwrap_or_else(|| PathBuf::from("."));
    opts.output_dir = output_dir;

    Ok(Action::Run { opts, ask })
}

fn main() -> ExitCode {
    let (opts, ask) = match parse_args(std::env::args_os().skip(1)) {
        Ok(Action::Version) => {
            println!("xisf2png {}", xisf2png::VERSION);
            return ExitCode::SUCCESS;
        }
        Ok(Action::Help) => {
            eprintln!("{}", usage_text());
            return ExitCode::SUCCESS;
        }
        Ok(Action::Run { opts, ask }) => (opts, ask),
        Err(msg) => {
            eprintln!("{msg}");
            eprintln!("{}", usage_text());
            return ExitCode::from(2);
        }
    };

    let kind = opts.input_kind();
    let verb = if opts.png_only {
        "Processed"
    } else {
        "Converted"
    };

    // Only ask for nicknames when there is someone at the terminal to answer.
    let ask = ask && io::stdin().is_terminal() && io::stdout().is_terminal();
    let ask: Option<&xisf2png::AskName> = if ask { Some(&ask_name) } else { None };

    let summary = xisf2png::run_asking(&opts, &AtomicBool::new(false), ask, &mut |p| {
        let rel = p.rel.display();
        match &p.status {
            FileStatus::Ok => match &p.label {
                Some(label) => println!("OK    {rel}  ->  {label}"),
                None => println!("OK    {rel}"),
            },
            FileStatus::Skipped => println!("SKIP  {rel}"),
            FileStatus::Failed(e) => println!("ERROR {rel}: {e}"),
        }
        if let Some(note) = &p.note {
            println!("      note: {note}");
        }
    });

    let summary = match summary {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };

    if summary.total == 0 {
        println!("No {kind} files found.");
        return ExitCode::SUCCESS;
    }

    println!();
    println!(
        "{verb}: {}   Skipped: {}   Failed: {}",
        summary.converted, summary.skipped, summary.failed
    );
    for w in &summary.warnings {
        println!("WARNING: {w}");
    }

    if summary.failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Ask on the terminal for a nickname for an object that has none. Just
/// pressing Enter skips it.
fn ask_name(designation: &str, kind: &str) -> Option<String> {
    let what = if kind.is_empty() {
        designation.to_string()
    } else {
        format!("{designation} ({kind})")
    };
    print!("No nickname found for {what}. Enter one, or press Enter to skip: ");
    io::stdout().flush().ok()?;
    let mut line = String::new();
    let n = io::stdin().read_line(&mut line).ok()?;
    if n == 0 {
        return None;
    }
    Some(line)
}

fn usage_text() -> String {
    format!(
        "xisf2png {} - batch convert XISF and FITS astronomical images to PNG\n\n\
         Usage:\n\
         \x20 xisf2png [input_dir] [output_dir] [--recursive|-r] [--overwrite] [--resize4k] [--filename] [--no-ask] [--font <file>] [-j N]\n\
         \x20 xisf2png [input_dir] [output_dir] --png-only [--recursive|-r] [--overwrite] [--filename] [--no-ask] [--font <file>] [-j N]\n\
         \x20 xisf2png <file>... [output_dir] [--overwrite] [--resize4k] [--filename] [--no-ask] [--font <file>] [-j N]\n\n\
         Converts every .xisf, .fits, .fit and .fts file found in input_dir, or\n\
         exactly the files given (.png files are only resized/stamped).\n\
         If input_dir is omitted, the current folder is used.\n\
         If output_dir is omitted, PNGs are written next to their source files.\n\n\
         Options:\n\
         \x20 -r, --recursive   recurse into subfolders (output mirrors structure)\n\
         \x20     --overwrite    overwrite existing .png files (default: skip)\n\
         \x20     --resize4k     scale each PNG to exactly 3840x2160 (aspect kept,\n\
         \x20                    center-cropped, no padding) and stamp the object\n\
         \x20                    name bottom-right. The object is identified from\n\
         \x20                    the header OBJECT keyword and/or a catalogue id in\n\
         \x20                    the file name (M31, NGC_7000, Sh2-155, ...), looked\n\
         \x20                    up via CDS Sesame/SIMBAD, and stamped as e.g.\n\
         \x20                    \"Andromeda Galaxy (M 31)\" plus NGC/IC ids, type\n\
         \x20                    and coordinates. Falls back to the file name.\n\
         \x20     --filename     stamp the plain file name: no header OBJECT, no\n\
         \x20                    online lookup (aliases: --no-lookup, --offline)\n\
         \x20     --no-ask       do not ask for a nickname when an object has none\n\
         \x20                    or lookup fails (never asked outside a terminal).\n\
         \x20                    Nicknames you enter are saved to your names file.\n\
         \x20     --png-only     skip XISF/FITS conversion: take existing .png files in\n\
         \x20                    input_dir and only resize/annotate them (implies\n\
         \x20                    --resize4k). Edited in place when output_dir is\n\
         \x20                    omitted, otherwise copied there first.\n\
         \x20     --font <file>  .ttf/.otf font file for the file-name stamp\n\
         \x20                    (default: bundled DejaVu Sans Condensed Bold).\n\
         \x20                    Also accepts --font=<file>.\n\
         \x20 -j, --concurrency N  convert N files in parallel\n\
         \x20                    (default: number of CPUs, capped at 8)\n\
         \x20 -V, --version     print version\n\
         \x20 -h, --help        show this help",
        xisf2png::VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os_args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_flags_and_defaults() {
        let Action::Run { opts, ask } = parse_args(os_args(&[
            "-r",
            "--overwrite",
            "--resize4k",
            "--filename",
            "--no-ask",
            "--concurrency=4",
            "--font=myfont.ttf",
        ]))
        .unwrap() else {
            panic!("expected Run");
        };
        assert!(opts.recursive);
        assert!(opts.overwrite);
        assert!(opts.resize4k);
        assert!(!opts.lookup);
        assert!(!ask);
        assert_eq!(opts.concurrency, 4);
        assert_eq!(opts.font, Some(PathBuf::from("myfont.ttf")));
        assert_eq!(opts.input_dir, PathBuf::from("."));
        assert_eq!(opts.output_dir, None);
    }

    #[test]
    fn parses_short_and_long_concurrency() {
        let Action::Run { opts, .. } = parse_args(os_args(&["-j3"])).unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(opts.concurrency, 3);

        let Action::Run { opts, .. } = parse_args(os_args(&["-j", "6"])).unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(opts.concurrency, 6);

        assert!(parse_args(os_args(&["-j0"])).is_err());
        assert!(parse_args(os_args(&["--concurrency=abc"])).is_err());
    }

    #[test]
    fn missing_image_file_is_not_treated_as_output_dir() {
        let err = parse_args(os_args(&["nonexistent_sub.xisf"]))
            .err()
            .unwrap();
        assert!(err.contains("File not found"), "got: {err}");

        let err = parse_args(os_args(&["nonexistent_sub.FITS"]))
            .err()
            .unwrap();
        assert!(err.contains("File not found"), "got: {err}");
    }

    #[test]
    fn double_dash_allows_dash_prefixed_paths() {
        let dir = std::env::temp_dir().join(format!("xisf2png-cli-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("-20C_light.fits");
        std::fs::write(&file, b"").unwrap();

        // Pass the dash-prefixed filename relative to a temp check using a
        // leading dash via `--`.
        let res = parse_args(os_args(&["--", "-out-dir"]));
        let Action::Run { opts, .. } = res.unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(opts.input_dir, PathBuf::from("-out-dir"));

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn usage_lines_are_properly_aligned() {
        let text = usage_text();
        assert!(
            text.contains("\n      --png-only     "),
            "misaligned --png-only in usage:\n{text}"
        );
        assert!(
            text.contains("\n                     or lookup fails"),
            "misaligned --no-ask continuation in usage:\n{text}"
        );
    }
}
