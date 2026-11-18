//! Writes the cut as a Final Cut Pro 7 XML (`xmeml` 4) sequence, which FilmCraft, Premiere Pro and
//! DaVinci Resolve import: one clip per kept range, back to back, video and audio linked.

use std::fmt::Write;

pub struct Media {
    pub name: String,
    pub url: String,
    pub timebase: u32,
    pub ntsc: bool,
    /// Length of the source in frames.
    pub frames: u64,
    /// The source's starting timecode in frames (camera files; 0 for phones and screen recordings).
    pub timecode: u64,
    pub video: Option<(u32, u32)>,
    /// Sample rate and channel count.
    pub audio: Option<(u32, u32)>,
}

/// `keeps` are source frame ranges `[in, out)`.
pub fn write(m: &Media, seq_name: &str, keeps: &[(u64, u64)]) -> String {
    let rate = format!("<rate><timebase>{}</timebase><ntsc>{}</ntsc></rate>", m.timebase, if m.ntsc { "TRUE" } else { "FALSE" });
    let total: u64 = keeps.iter().map(|(a, b)| b - a).sum();
    let mut x = String::new();
    let _ = write!(x, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"4\">\n<sequence id=\"sequence-1\">\n");
    let _ = writeln!(x, "<name>{}</name><duration>{total}</duration>{rate}", esc(seq_name));
    x.push_str("<media>\n");
    let mut file_written = false;
    let kinds = [("video", m.video.is_some()), ("audio", m.audio.is_some())];
    for (kind, present) in kinds {
        if !present {
            continue;
        }
        let _ = write!(x, "<{kind}><format><samplecharacteristics>");
        match (kind, m.video, m.audio) {
            ("video", Some((w, h)), _) => {
                let _ = write!(x, "{rate}<width>{w}</width><height>{h}</height><pixelaspectratio>square</pixelaspectratio>");
            }
            (_, _, Some((sr, _))) => {
                let _ = write!(x, "<depth>16</depth><samplerate>{sr}</samplerate>");
            }
            _ => {}
        }
        x.push_str("</samplecharacteristics></format>\n<track>\n");
        let mut start = 0;
        for (i, (a, b)) in keeps.iter().enumerate() {
            let end = start + (b - a);
            let tag = &kind[..1];
            let _ = write!(
                x,
                "<clipitem id=\"clipitem-{tag}{n}\"><name>{name}</name><enabled>TRUE</enabled><duration>{d}</duration>{rate}<start>{start}</start><end>{end}</end><in>{a}</in><out>{b}</out>",
                n = i + 1,
                name = esc(&m.name),
                d = m.frames,
            );
            if file_written {
                x.push_str("<file id=\"file-1\"/>");
            } else {
                file_written = true;
                let _ = write!(x, "<file id=\"file-1\"><name>{}</name><pathurl>{}</pathurl>{rate}<duration>{}</duration><media>", esc(&m.name), esc(&m.url), m.frames);
                if let Some((w, h)) = m.video {
                    let _ = write!(x, "<video><samplecharacteristics>{rate}<width>{w}</width><height>{h}</height></samplecharacteristics></video>");
                }
                if let Some((sr, ch)) = m.audio {
                    let _ = write!(x, "<audio><samplecharacteristics><depth>16</depth><samplerate>{sr}</samplerate></samplecharacteristics><channelcount>{ch}</channelcount></audio>");
                }
                x.push_str("</media></file>");
            }
            if kind == "audio" {
                x.push_str("<sourcetrack><mediatype>audio</mediatype><trackindex>1</trackindex></sourcetrack>");
            }
            if m.video.is_some() && m.audio.is_some() {
                for (k, _) in kinds {
                    let _ = write!(
                        x,
                        "<link><linkclipref>clipitem-{}{}</linkclipref><mediatype>{k}</mediatype><trackindex>1</trackindex><clipindex>{}</clipindex></link>",
                        &k[..1],
                        i + 1,
                        i + 1
                    );
                }
            }
            x.push_str("</clipitem>\n");
            start = end;
        }
        let _ = writeln!(x, "</track>\n</{kind}>");
    }
    x.push_str("</media>\n</sequence>\n</xmeml>\n");
    x
}

/// The same cut as FCPXML 1.9, for Final Cut Pro. `fps` is the exact rate as (numerator, denominator).
pub fn write_fcpxml(m: &Media, seq_name: &str, keeps: &[(u64, u64)], fps: (u64, u64)) -> String {
    let t = |f: u64| if f == 0 { "0s".to_string() } else { format!("{}/{}s", f * fps.1, fps.0) };
    let (w, h) = m.video.unwrap_or((1920, 1080));
    let (sr, ch) = m.audio.unwrap_or((48_000, 2));
    let total: u64 = keeps.iter().map(|(a, b)| b - a).sum();
    let name = esc(&m.name);
    let mut x = String::new();
    let _ = writeln!(x, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.9\">\n<resources>");
    let _ = writeln!(x, "<format id=\"r1\" frameDuration=\"{}/{}s\" width=\"{w}\" height=\"{h}\"/>", fps.1, fps.0);
    let _ = writeln!(
        x,
        "<asset id=\"r2\" name=\"{name}\" start=\"{}\" duration=\"{}\" hasVideo=\"{}\" hasAudio=\"{}\" format=\"r1\" audioSources=\"1\" audioChannels=\"{ch}\" audioRate=\"{sr}\"><media-rep kind=\"original-media\" src=\"{}\"/></asset>",
        t(m.timecode),
        t(m.frames),
        u8::from(m.video.is_some()),
        u8::from(m.audio.is_some()),
        esc(&m.url)
    );
    let _ = writeln!(x, "</resources>\n<library><event name=\"Evil Fast Cut\"><project name=\"{}\">", esc(seq_name));
    let _ = writeln!(x, "<sequence format=\"r1\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"NDF\"><spine>", t(total));
    let mut offset = 0;
    for (a, b) in keeps {
        let _ = writeln!(x, "<asset-clip ref=\"r2\" name=\"{name}\" offset=\"{}\" start=\"{}\" duration=\"{}\" format=\"r1\" tcFormat=\"NDF\"/>", t(offset), t(m.timecode + a), t(b - a));
        offset += b - a;
    }
    x.push_str("</spine></sequence></project></event></library>\n</fcpxml>\n");
    x
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `file://` URL of an absolute path (Windows paths become `file:///C:/...`).
pub fn file_url(path: &str) -> String {
    let p = path.replace('\\', "/");
    // Windows long paths: \\?\C:\x is C:/x, and \\?\UNC\server\share\x is //server/share/x.
    let p = match p.strip_prefix("//?/UNC/") {
        Some(unc) => format!("//{unc}"),
        None => p.strip_prefix("//?/").unwrap_or(&p).to_string(),
    };
    // file:// + /local/path, file:///C:/path, or file://server/share/path for a network share.
    let mut out = String::from(if p.starts_with("//") { "file:" } else if p.starts_with('/') { "file://" } else { "file:///" });
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(file_url("/home/me/My Clip #1.mp4"), "file:///home/me/My%20Clip%20%231.mp4");
        assert_eq!(file_url(r"\\?\C:\Users\me\a b.mov"), "file:///C:/Users/me/a%20b.mov");
        assert_eq!(file_url(r"\\?\UNC\nas\videos\a.mp4"), "file://nas/videos/a.mp4");
    }

    #[test]
    fn clips_are_back_to_back_and_file_defined_once() {
        let m = Media { name: "a&b.mp4".into(), url: "file:///a".into(), timebase: 30, ntsc: true, frames: 300, timecode: 0, video: Some((1920, 1080)), audio: Some((48_000, 2)) };
        let x = write(&m, "a (auto cut)", &[(0, 33), (57, 90)]);
        assert!(x.contains("<start>0</start><end>33</end><in>0</in><out>33</out>"));
        assert!(x.contains("<start>33</start><end>66</end><in>57</in><out>90</out>"));
        assert_eq!(x.matches("<pathurl>").count(), 1);
        assert_eq!(x.matches("<clipitem ").count(), 4);
        assert!(x.contains("<name>a&amp;b.mp4</name>"));
    }

    #[test]
    fn fcpxml_uses_exact_frame_times() {
        let m = Media { name: "a.mp4".into(), url: "file:///a.mp4".into(), timebase: 30, ntsc: true, frames: 300, timecode: 0, video: Some((1920, 1080)), audio: Some((48_000, 2)) };
        let x = write_fcpxml(&m, "a (fast cut)", &[(0, 33), (57, 90)], (30000, 1001));
        assert!(x.contains("frameDuration=\"1001/30000s\""));
        assert!(x.contains("offset=\"0s\" start=\"0s\" duration=\"33033/30000s\""));
        assert!(x.contains("offset=\"33033/30000s\" start=\"57057/30000s\" duration=\"33033/30000s\""));
        assert!(x.contains("<sequence format=\"r1\" duration=\"66066/30000s\""));
        assert_eq!(x.matches("<asset-clip ").count(), 2);
        let m = Media { timecode: 107_892, ..m };
        let x = write_fcpxml(&m, "a", &[(57, 90)], (30000, 1001));
        assert!(x.contains("start=\"107999892/30000s\" duration=\"300300/30000s\""), "{x}");
        assert!(x.contains(&format!("start=\"{}/30000s\" duration=\"33033/30000s\"", (107_892 + 57) * 1001)), "{x}");
    }
}
