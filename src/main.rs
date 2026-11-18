//! Evil Fast Cut: removes silences (and optionally breaths) from a video. It writes a cut-down
//! sequence FilmCraft imports (File ▸ Import), and can also render the trimmed video with ffmpeg.

mod detect;
mod fcp7;

use std::path::{Path, PathBuf};
use std::process::Command;

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
    let (mut input, mut xml, mut video) = (None, None, None);
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
            "--video" => video = Some(PathBuf::from(value()?)),
            _ if a.starts_with('-') => return Err(format!("unknown option {a} (try --help)")),
            _ => input = Some(PathBuf::from(a)),
        }
    }
    let Some(input) = input else {
        print!("{HELP}");
        return Err("no video given".into());
    };
    let input = std::fs::canonicalize(&input).map_err(|e| format!("can't open {}: {e}", input.display()))?;

    let p = probe(&input)?;
    let Some(audio) = p.audio else { return Err("this file has no sound to look at".into()) };
    let samples = decode(&input)?;
    let keeps = detect::keep_ranges(&samples, &s);

    // Snap to whole frames, never past the end, merging ranges that now touch.
    let fps = p.fps.0 as f64 / p.fps.1 as f64;
    let frames = p.frames.max(1);
    let mut cuts: Vec<(u64, u64)> = Vec::new();
    for (a, b) in keeps {
        let (a, b) = (((a * fps).floor() as u64).min(frames), ((b * fps).ceil() as u64).min(frames));
        match cuts.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ if b > a => cuts.push((a, b)),
            _ => {}
        }
    }
    if cuts.is_empty() {
        return Err(format!("the whole file is quieter than {} dB; try a lower --threshold", s.threshold_db));
    }

    let kept: u64 = cuts.iter().map(|(a, b)| b - a).sum();
    let secs = |f: u64| f as f64 / fps;
    println!(
        "Kept {:.1} s of {:.1} s ({} pieces, {:.1} s removed).",
        secs(kept),
        secs(frames),
        cuts.len(),
        secs(frames - kept)
    );

    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = input.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let xml = xml.unwrap_or_else(|| input.with_file_name(format!("{stem}-fastcut.xml")));
    let media = fcp7::Media {
        name,
        url: fcp7::file_url(&input.to_string_lossy()),
        timebase: (fps.round() as u32).max(1),
        ntsc: p.fps.1 == 1001,
        frames,
        video: p.video,
        audio: Some(audio),
    };
    std::fs::write(&xml, fcp7::write(&media, &format!("{stem} (fast cut)"), &cuts)).map_err(|e| format!("can't write {}: {e}", xml.display()))?;
    println!("Wrote {}", xml.display());

    if let Some(out) = video {
        render(&input, &out, &cuts, fps, p.video.is_some())?;
        println!("Wrote {}", out.display());
    }
    Ok(())
}

fn number(opt: &str, v: &str, lo: f64, hi: f64) -> Result<f64, String> {
    v.parse::<f64>().ok().filter(|x| (lo..=hi).contains(x)).ok_or(format!("{opt} must be a number from {lo} to {hi}, not {v}"))
}

struct Probe {
    fps: (u64, u64),
    frames: u64,
    video: Option<(u32, u32)>,
    audio: Option<(u32, u32)>,
}

fn ffmpeg() -> Command {
    let mut c = Command::new("ffmpeg");
    c.args(["-nostdin", "-v", "error"]);
    c
}

fn output(mut c: Command, what: &str) -> Result<Vec<u8>, String> {
    let name = c.get_program().to_string_lossy().into_owned();
    let o = c.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{name} isn't installed (or isn't on PATH). Get it from https://ffmpeg.org/download.html")
        } else {
            format!("couldn't run {name}: {e}")
        }
    })?;
    if !o.status.success() {
        return Err(format!("{name} couldn't {what}: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(o.stdout)
}

fn probe(input: &Path) -> Result<Probe, String> {
    let mut c = Command::new("ffprobe");
    c.args(["-v", "error", "-show_entries", "stream=codec_type,width,height,avg_frame_rate,r_frame_rate,sample_rate,channels:stream_disposition=attached_pic:format=duration", "-of", "default=nw=1"]).arg(input);
    let text = String::from_utf8_lossy(&output(c, "read this file")?).into_owned();
    let mut streams: Vec<std::collections::HashMap<&str, &str>> = Vec::new();
    let mut duration = 0.0;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "codec_type" => streams.push([(k, v)].into()),
            "duration" => duration = v.parse().unwrap_or(0.0), // only the format section has one
            _ => {
                if let Some(s) = streams.last_mut() {
                    s.insert(k, v);
                }
            }
        }
    }
    let int = |s: &std::collections::HashMap<&str, &str>, k| s.get(k).and_then(|v| v.parse::<u32>().ok());
    // Cover art in a sound file shows up as a video stream; it isn't video.
    let v = streams.iter().find(|s| s["codec_type"] == "video" && s.get("DISPOSITION:attached_pic") != Some(&"1"));
    let a = streams.iter().find(|s| s["codec_type"] == "audio");
    let rate = |k| v.and_then(|s| s.get(k)).and_then(|r| ratio(r));
    let fps = standard_rate(rate("avg_frame_rate").or(rate("r_frame_rate")).unwrap_or(30.0));
    Ok(Probe {
        fps,
        frames: (duration * fps.0 as f64 / fps.1 as f64).round() as u64,
        video: v.and_then(|s| Some((int(s, "width")?, int(s, "height")?))),
        audio: a.map(|s| (int(s, "sample_rate").unwrap_or(48_000), int(s, "channels").unwrap_or(2))),
    })
}

fn ratio(r: &str) -> Option<f64> {
    let (n, d) = r.split_once('/').unwrap_or((r, "1"));
    let x = n.parse::<f64>().ok()? / d.parse::<f64>().ok()?;
    (x.is_finite() && x > 0.0).then_some(x)
}

/// Phones record at a variable rate (e.g. 29.87 fps on average): edit on the nearest standard rate.
fn standard_rate(fps: f64) -> (u64, u64) {
    const RATES: [(u64, u64); 10] = [(24000, 1001), (24, 1), (25, 1), (30000, 1001), (30, 1), (48, 1), (50, 1), (60000, 1001), (60, 1), (120, 1)];
    *RATES.iter().min_by(|a, b| (a.0 as f64 / a.1 as f64 - fps).abs().total_cmp(&(b.0 as f64 / b.1 as f64 - fps).abs())).unwrap_or(&(30, 1))
}

/// First audio stream as mono at detect::RATE.
// shortcut: holds the whole track in memory (~230 MB per hour), fine for normal videos.
fn decode(input: &Path) -> Result<Vec<f32>, String> {
    let mut c = ffmpeg();
    c.arg("-i").arg(input).args(["-map", "0:a:0", "-ac", "1", "-ar", &detect::RATE.to_string(), "-f", "f32le", "pipe:1"]);
    let bytes = output(c, "read the sound")?;
    Ok(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

// shortcut: the kept ranges go on the command line (~60 chars each), so Windows' 32k limit caps this
// at ~500 pieces; past that, import the XML in FilmCraft and export from there.
fn render(input: &Path, out: &Path, cuts: &[(u64, u64)], fps: f64, has_video: bool) -> Result<(), String> {
    let expr: Vec<String> = cuts.iter().map(|(a, b)| format!("gte(t,{:.4})*lt(t,{:.4})", *a as f64 / fps, *b as f64 / fps)).collect();
    let expr = expr.join("+");
    let mut c = ffmpeg();
    c.arg("-y").arg("-i").arg(input);
    if has_video {
        c.arg("-vf").arg(format!("select='{expr}',setpts=N/FRAME_RATE/TB"));
    }
    c.arg("-af").arg(format!("aselect='{expr}',asetpts=N/SR/TB")).arg(out);
    output(c, "render the trimmed video").map(|_| ())
}
