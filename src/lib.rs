//! Evil Fast Cut: removes silences (and optionally breaths) from a video. Writes a cut-down
//! sequence FilmCraft, Resolve and Premiere import (FCP7 XML), an FCPXML for Final Cut Pro, and
//! can render the trimmed video with ffmpeg. Used by the command line and the window.

pub mod detect;
pub mod fcp7;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What to write; `None` skips that output.
#[derive(Default)]
pub struct Outputs {
    pub xml: Option<PathBuf>,
    pub fcpxml: Option<PathBuf>,
    pub video: Option<PathBuf>,
}

/// The usual output names, next to the video: `X-fastcut.xml`, `X-fastcut.fcpxml`, `X-cut.<ext>`.
pub fn default_outputs(video: &Path) -> Outputs {
    let stem = video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = video.extension().map_or("mp4".into(), |e| e.to_string_lossy().into_owned());
    Outputs {
        xml: Some(video.with_file_name(format!("{stem}-fastcut.xml"))),
        fcpxml: Some(video.with_file_name(format!("{stem}-fastcut.fcpxml"))),
        video: Some(video.with_file_name(format!("{stem}-cut.{ext}"))),
    }
}

/// Finds what to cut in `given` and writes `out`, reporting progress lines to `log`.
/// `track` is the sound track to listen to (0 = first); `None` listens to all of them mixed.
pub fn cut(given: &Path, s: &detect::Settings, track: Option<usize>, out: &Outputs, log: &mut dyn FnMut(String)) -> Result<(), String> {
    // ffmpeg gets the path as given: Windows' canonical form (\\?\C:\...) is only for the file URL.
    let input = std::fs::canonicalize(given).map_err(|e| format!("can't open {}: {e}", given.display()))?;
    for f in [&out.xml, &out.fcpxml, &out.video].into_iter().flatten() {
        if std::fs::canonicalize(f).is_ok_and(|f| f == input) {
            return Err(format!("{} is the video itself; pick another name so it isn't overwritten", f.display()));
        }
    }

    let p = probe(given)?;
    let n = p.audio.len();
    let Some(&audio) = p.audio.first() else { return Err("this file has no sound to look at".into()) };
    let chosen: Vec<usize> = match track {
        Some(t) if t >= n => return Err(format!("this file has {n} sound track{}, so there is no track {}", if n == 1 { "" } else { "s" }, t + 1)),
        Some(t) => vec![t],
        None => (0..n).collect(),
    };
    if n > 1 && track.is_none() {
        log(format!("This file has {n} sound tracks; listening to all of them together. If one is only music or game sound, pick the voice track."));
    }
    let mut samples = decode(given, &chosen)?;
    // Times in the edit count from the start of the picture. Sound that starts later (screen
    // recorders, camcorders) is shifted to line up, so the start of each word isn't cut off.
    let audio_start = chosen.iter().map(|&t| p.audio[t].2).fold(f64::INFINITY, f64::min);
    let offset = ((audio_start - p.start) * detect::RATE as f64).round() as i64;
    if offset > 0 {
        samples.splice(0..0, std::iter::repeat_n(0.0, offset as usize));
    } else {
        samples.drain(..(offset.unsigned_abs() as usize).min(samples.len()));
    }
    let keeps = detect::keep_ranges(&samples, s);

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
        return Err(format!("the whole file is quieter than {} dB; try a lower threshold", s.threshold_db));
    }

    let kept: u64 = cuts.iter().map(|(a, b)| b - a).sum();
    let secs = |f: u64| f as f64 / fps;
    log(format!(
        "Kept {:.1} s of {:.1} s ({} pieces, {:.1} s removed).",
        secs(kept),
        secs(frames),
        cuts.len(),
        secs(frames - kept)
    ));

    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = input.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let media = fcp7::Media {
        name,
        url: fcp7::file_url(&input.to_string_lossy()),
        timebase: (fps.round() as u32).max(1),
        ntsc: p.fps.1 == 1001,
        frames,
        timecode: p.timecode.as_deref().and_then(|tc| timecode_frames(tc, p.fps)).unwrap_or(0),
        video: p.video,
        audio: Some((audio.0, audio.1)),
    };
    if let Some(xml) = &out.xml {
        std::fs::write(xml, fcp7::write(&media, &format!("{stem} (fast cut)"), &cuts)).map_err(|e| format!("can't write {}: {e}", xml.display()))?;
        log(format!("Wrote {}", xml.display()));
    }

    if let Some(f) = &out.fcpxml {
        std::fs::write(f, fcp7::write_fcpxml(&media, &format!("{stem} (fast cut)"), &cuts, p.fps)).map_err(|e| format!("can't write {}: {e}", f.display()))?;
        log(format!("Wrote {}", f.display()));
    }
    if let Some(f) = &out.video {
        render(given, f, &cuts, fps, &p, track)?;
        log(format!("Wrote {}", f.display()));
    }
    Ok(())
}

struct Probe {
    fps: (u64, u64),
    frames: u64,
    /// Start time of the picture (or of the sound, without picture), seconds.
    start: f64,
    video: Option<(u32, u32)>,
    /// Each sound track: sample rate, channels, start time.
    audio: Vec<(u32, u32, f64)>,
    timecode: Option<String>,
}

fn ffmpeg() -> Command {
    let mut c = command("ffmpeg");
    c.args(["-nostdin", "-v", "error"]);
    c
}

fn command(name: &str) -> Command {
    // An app opened from the Finder doesn't get Homebrew's folders on its PATH.
    #[cfg(target_os = "macos")]
    if let Some(p) = ["/opt/homebrew/bin", "/usr/local/bin"].iter().map(|d| Path::new(d).join(name)).find(|p| p.exists()) {
        return Command::new(p);
    }
    #[allow(unused_mut)]
    let mut c = Command::new(name);
    // Without this, every ffmpeg run flashes a console window over the GUI on Windows.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW
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
    let mut c = command("ffprobe");
    c.args([
        "-v",
        "error",
        "-show_entries",
        "stream=codec_type,width,height,avg_frame_rate,r_frame_rate,sample_rate,channels,start_time,duration:stream_disposition=attached_pic:stream_tags=timecode:format=duration,start_time:format_tags=timecode",
        "-of",
        "default",
    ])
    .arg(input);
    let text = String::from_utf8_lossy(&output(c, "read this file")?).into_owned();
    // Sections: [STREAM] ... [/STREAM] per stream, then [FORMAT] ... [/FORMAT].
    let mut sections: Vec<HashMap<&str, &str>> = Vec::new();
    for line in text.lines() {
        match line.split_once('=') {
            Some((k, v)) => {
                if let Some(s) = sections.last_mut() {
                    s.insert(k, v);
                }
            }
            None if line.starts_with("[/") => {}
            None => sections.push([("section", line)].into()),
        }
    }
    let streams: Vec<&HashMap<&str, &str>> = sections.iter().filter(|s| s["section"] == "[STREAM]").collect();
    let format = sections.iter().find(|s| s["section"] == "[FORMAT]");
    let num = |s: Option<&&HashMap<&str, &str>>, k| s.and_then(|s| s.get(k)).and_then(|v| v.parse::<f64>().ok()).filter(|x| x.is_finite());
    let fnum = |k| format.and_then(|s| s.get(k)).and_then(|v| v.parse::<f64>().ok()).filter(|x| x.is_finite());
    let int = |s: &HashMap<&str, &str>, k| s.get(k).and_then(|v| v.parse::<u32>().ok());
    // Cover art in a sound file shows up as a video stream; it isn't video.
    let v = streams.iter().find(|s| s["codec_type"] == "video" && s.get("DISPOSITION:attached_pic") != Some(&"1"));
    let audio: Vec<(u32, u32, f64)> = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .map(|s| (int(s, "sample_rate").unwrap_or(48_000), int(s, "channels").unwrap_or(2), num(Some(s), "start_time").unwrap_or(0.0)))
        .collect();
    let rate = |k| v.and_then(|s| s.get(k)).and_then(|r| ratio(r));
    let fps = standard_rate(rate("avg_frame_rate").or(rate("r_frame_rate")).unwrap_or(30.0));
    // The picture's own length: the sound often runs a little longer than the container says it should.
    let duration = num(v, "duration").or(fnum("duration")).unwrap_or(0.0);
    let start = match v {
        Some(_) => num(v, "start_time").unwrap_or(0.0),
        None => audio.iter().map(|a| a.2).fold(f64::INFINITY, f64::min).min(fnum("start_time").unwrap_or(0.0)),
    };
    let timecode = sections.iter().find_map(|s| s.get("TAG:timecode")).map(|t| t.to_string());
    Ok(Probe {
        fps,
        frames: (duration * fps.0 as f64 / fps.1 as f64).round() as u64,
        start: if start.is_finite() { start } else { 0.0 },
        video: v.and_then(|s| Some((int(s, "width")?, int(s, "height")?))),
        audio,
        timecode,
    })
}

/// Frames since midnight of a camera timecode like `01:00:00:00` (`;` before the frames = drop-frame).
fn timecode_frames(tc: &str, fps: (u64, u64)) -> Option<u64> {
    let f: Vec<u64> = tc.split([':', ';', '.']).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let [h, m, s, fr] = f[..] else { return None };
    let base = (fps.0 as f64 / fps.1 as f64).round() as u64;
    let mins = h * 60 + m;
    let mut n = (mins * 60 + s) * base + fr;
    if tc.contains(';') && fps.1 == 1001 {
        n -= base / 15 * (mins - mins / 10); // drop-frame skips 2 (4 at 59.94) numbers a minute, except every tenth
    }
    Some(n)
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

/// The chosen sound tracks, mixed to mono at detect::RATE.
// shortcut: holds the whole track in memory (~230 MB per hour), fine for normal videos.
fn decode(input: &Path, tracks: &[usize]) -> Result<Vec<f32>, String> {
    let mut c = ffmpeg();
    c.arg("-i").arg(input);
    if tracks.len() > 1 {
        c.args(["-filter_complex", &format!("{}amix=inputs={}:normalize=0[a]", pads(tracks), tracks.len()), "-map", "[a]"]);
    } else {
        c.args(["-map", &format!("0:a:{}", tracks[0])]);
    }
    c.args(["-ac", "1", "-ar", &detect::RATE.to_string(), "-f", "f32le", "pipe:1"]);
    let bytes = output(c, "read the sound")?;
    Ok(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn pads(tracks: &[usize]) -> String {
    tracks.iter().map(|t| format!("[0:a:{t}]")).collect()
}

/// Renders the kept pieces. The cut list goes to ffmpeg in a file, so there is no limit on how
/// many pieces there are (a command line on Windows holds only ~500).
fn render(input: &Path, out: &Path, cuts: &[(u64, u64)], fps: f64, p: &Probe, track: Option<usize>) -> Result<(), String> {
    // Filters see each stream's own timestamps, which count from the file's start, not the picture's.
    let expr: Vec<String> = cuts.iter().map(|(a, b)| format!("gte(t,{:.5})*lt(t,{:.5})", p.start + *a as f64 / fps, p.start + *b as f64 / fps)).collect();
    let expr = expr.join("+");
    let tracks: Vec<usize> = track.map_or((0..p.audio.len()).collect(), |t| vec![t]);
    let mix = if tracks.len() > 1 { format!("{}amix=inputs={}:normalize=0,", pads(&tracks), tracks.len()) } else { pads(&tracks) };
    let mut graph = format!("{mix}aselect='{expr}',asetpts=N/SR/TB[a]");
    if p.video.is_some() {
        graph = format!("[0:v:0]select='{expr}',setpts=N/FRAME_RATE/TB[v];\n{graph}");
    }
    let script = std::env::temp_dir().join(format!("evil-fastcut-{}.txt", std::process::id()));
    std::fs::write(&script, graph).map_err(|e| format!("can't write a temporary file: {e}"))?;
    let run = |option: &str| {
        let mut c = ffmpeg();
        c.arg("-y").arg("-i").arg(input).arg(option).arg(&script);
        if p.video.is_some() {
            // High quality whichever encoder ffmpeg picks (-crf for H.264/H.265, -q:v for the others).
            c.args(["-map", "[v]", "-crf", "18", "-q:v", "2"]);
        }
        c.args(["-map", "[a]", "-b:a", "192k"]).arg(out);
        output(c, "render the trimmed video")
    };
    // ffmpeg 7+ reads a filter file with -/filter_complex; older ones only know -filter_complex_script.
    let result = match run("-/filter_complex") {
        Err(e) if e.contains("nrecognized option") => run("-filter_complex_script"),
        r => r,
    };
    let _ = std::fs::remove_file(&script);
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecodes() {
        assert_eq!(timecode_frames("01:00:00:00", (25, 1)), Some(90_000));
        assert_eq!(timecode_frames("00:01:00;02", (30000, 1001)), Some(1800)); // first frame of minute 1 in drop-frame
        assert_eq!(timecode_frames("01:00:00;00", (30000, 1001)), Some(107_892));
        assert_eq!(timecode_frames("00:00:10:00", (30000, 1001)), Some(300));
        assert_eq!(timecode_frames("bad", (25, 1)), None);
    }
}
