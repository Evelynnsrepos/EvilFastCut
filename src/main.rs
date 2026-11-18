//! Evil Fast Cut on the command line (the window is evil-fastcut-gui).

use evil_fastcut::{Outputs, cut, default_outputs, detect};

use std::path::PathBuf;

const HELP: &str = "\
Evil Fast Cut: cut the silent parts (and breaths) out of a video.

Usage: evil-fastcut VIDEO [options]

Writes VIDEO-fastcut.xml next to the video. Import it in FilmCraft with File > Import
(it also opens in Premiere Pro and DaVinci Resolve) to get a sequence with the gaps removed.

Options:
  --threshold DB       quieter than this counts as silence, in dB        [default: -40]
                       (noisy room: try -35; very quiet voice: try -50)
  --min-silence SEC    only cut silences at least this long, in seconds  [default: 0.5]
  --padding SEC        sound kept before and after each cut, in seconds  [default: 0.1]
  --breaths            also cut breaths between sentences
  --breath-drop DB     how much quieter than the speech a breath is      [default: 12]
                       (lower it to catch louder breaths)
  --xml FILE           where to write the sequence                       [default: VIDEO-fastcut.xml]
  --fcpxml FILE        also write an FCPXML for Final Cut Pro
  --video FILE         also render the trimmed video to FILE with ffmpeg
  -h, --help           show this help

Needs ffmpeg and ffprobe installed (https://ffmpeg.org/download.html).
";

fn main() {
    if let Err(e) = run() {
        eprintln!("evil-fastcut: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut input, mut xml, mut fcpxml, mut video) = (None, None, None, None);
    let mut s = detect::Settings::default();
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "--threshold" => s.threshold_db = number(&a, &value()?, -120.0, 0.0)? as f32,
            "--min-silence" => s.min_silence = number(&a, &value()?, 0.0, 3600.0)?,
            "--padding" => s.padding = number(&a, &value()?, 0.0, 10.0)?,
            "--breaths" => s.breaths = true,
            "--breath-drop" => s.breath_drop_db = number(&a, &value()?, 0.0, 60.0)? as f32,
            "--xml" => xml = Some(PathBuf::from(value()?)),
            "--fcpxml" => fcpxml = Some(PathBuf::from(value()?)),
            "--video" => video = Some(PathBuf::from(value()?)),
            _ if a.starts_with('-') => return Err(format!("unknown option {a} (try --help)")),
            _ => input = Some(PathBuf::from(a)),
        }
    }
    let Some(input) = input else {
        print!("{HELP}");
        return Err("no video given".into());
    };
    let out = Outputs { xml: Some(xml.unwrap_or_else(|| default_outputs(&input).xml.unwrap_or_default())), fcpxml, video };
    cut(&input, &s, &out, &mut |line| println!("{line}"))
}

fn number(opt: &str, v: &str, lo: f64, hi: f64) -> Result<f64, String> {
    v.parse::<f64>().ok().filter(|x| (lo..=hi).contains(x)).ok_or(format!("{opt} must be a number from {lo} to {hi}, not {v}"))
}
