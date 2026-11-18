//! Finds the parts of a recording to keep: everything except silences (and, optionally, breaths).
//!
//! Works on mono audio at [`RATE`]. The signal is measured in 20 ms windows every 10 ms.

pub const RATE: u32 = 16_000;
const HOP: usize = 160; // 10 ms
const WIN: usize = 320; // 20 ms loudness window
const PITCH_WIN: usize = 480; // 30 ms: room for two periods of a 70 Hz voice
const MIN_LAG: usize = 40; // 400 Hz
const MAX_LAG: usize = 228; // 70 Hz
const BREATH_MIN: usize = 15; // frames: a breath is 0.15 ..
const BREATH_MAX: usize = 120; // .. 1.2 s long
const VOICED: f32 = 0.45; // normalised autocorrelation above this means a voice is sounding

#[derive(Clone, Copy)]
pub struct Settings {
    /// Quieter than this (dBFS) counts as silence.
    pub threshold_db: f32,
    /// Silences shorter than this (seconds) are kept.
    pub min_silence: f64,
    /// Sound kept on each side of a cut (seconds), so words don't start or end clipped.
    pub padding: f64,
    /// Also cut breaths: quiet, unvoiced sounds between phrases.
    pub breaths: bool,
    /// A breath is at least this many dB quieter than the loud parts of the speech.
    pub breath_drop_db: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { threshold_db: -40.0, min_silence: 0.5, padding: 0.1, breaths: false, breath_drop_db: 12.0 }
    }
}

/// Time ranges (seconds, start < end, in order) to keep.
pub fn keep_ranges(samples: &[f32], s: &Settings) -> Vec<(f64, f64)> {
    let total = samples.len() as f64 / RATE as f64;
    let n = samples.len().div_ceil(HOP);
    let db: Vec<f32> = (0..n).map(|i| level_db(window(samples, i, WIN))).collect();
    let silent: Vec<bool> = db.iter().map(|d| *d < s.threshold_db).collect();
    let breath = if s.breaths { find_breaths(samples, &db, &silent, s) } else { vec![false; n] };

    // Cut every run of silent/breath frames that is long enough or holds a breath.
    let mut gaps = Vec::new();
    let mut i = 0;
    while i < n {
        if !(silent[i] || breath[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && (silent[i] || breath[i]) {
            i += 1;
        }
        let (g0, g1) = (start as f64 * 0.01, (i as f64 * 0.01).min(total));
        if g1 - g0 >= s.min_silence || breath[start..i].contains(&true) {
            // Pad inwards, except at the very start and end of the recording.
            let a = if start == 0 { 0.0 } else { g0 + s.padding };
            let b = if i == n { total } else { g1 - s.padding };
            if b > a {
                gaps.push((a, b));
            }
        }
    }

    let mut keeps = Vec::new();
    let mut t = 0.0;
    for (a, b) in gaps {
        if a > t {
            keeps.push((t, a));
        }
        t = b;
    }
    if total > t {
        keeps.push((t, total));
    }
    keeps
}

fn window(samples: &[f32], frame: usize, len: usize) -> &[f32] {
    let a = (frame * HOP).min(samples.len());
    &samples[a..(a + len).min(samples.len())]
}

fn level_db(w: &[f32]) -> f32 {
    let ms = w.iter().map(|x| x * x).sum::<f32>() / w.len().max(1) as f32;
    10.0 * (ms + 1e-12).log10()
}

/// Breaths: runs of 0.15-1.2 s that are audible, well below the speech level and have no pitch.
fn find_breaths(samples: &[f32], db: &[f32], silent: &[bool], s: &Settings) -> Vec<bool> {
    let mut loud: Vec<f32> = db.iter().zip(silent).filter(|(_, s)| !**s).map(|(d, _)| *d).collect();
    let n = db.len();
    if loud.is_empty() {
        return vec![false; n];
    }
    loud.sort_by(f32::total_cmp);
    let speech = loud[(loud.len() - 1) * 95 / 100];
    let limit = speech - s.breath_drop_db;
    // Pitch is only measured on quiet, non-silent frames, which keeps this fast on long files.
    let mut cand: Vec<bool> = (0..n).map(|i| !silent[i] && db[i] < limit && voicing(window(samples, i, PITCH_WIN)) < VOICED).collect();

    // Close holes of up to 3 frames, then keep runs of breath length.
    let mut i = 0;
    while i < n {
        if cand[i] {
            let mut j = i + 1;
            while j < n && (cand[j] || (j + 3 < n && cand[j + 1..=j + 3].contains(&true) && !silent[j])) {
                j += 1;
            }
            let keep = (BREATH_MIN..=BREATH_MAX).contains(&(j - i));
            cand[i..j].iter_mut().for_each(|c| *c = keep);
            i = j;
        } else {
            i += 1;
        }
    }
    cand
}

/// Highest normalised autocorrelation over voice pitches (70-400 Hz): near 1 for a voiced sound.
fn voicing(w: &[f32]) -> f32 {
    let mut best = 0.0f32;
    for lag in (MIN_LAG..=MAX_LAG.min(w.len().saturating_sub(MIN_LAG))).step_by(2) {
        let (a, b) = (&w[..w.len() - lag], &w[lag..]);
        let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
        for (x, y) in a.iter().zip(b) {
            ab += x * y;
            aa += x * x;
            bb += y * y;
        }
        best = best.max(ab / (aa * bb).sqrt().max(1e-12));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f64, hz: f32, amp: f32) -> Vec<f32> {
        (0..(secs * RATE as f64) as usize).map(|i| amp * (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin()).collect()
    }
    fn noise(secs: f64, amp: f32) -> Vec<f32> {
        let mut x = 12345u32;
        (0..(secs * RATE as f64) as usize)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amp * ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }
    fn near(got: &[(f64, f64)], want: &[(f64, f64)]) {
        assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
        for ((a, b), (c, d)) in got.iter().zip(want) {
            assert!((a - c).abs() < 0.03 && (b - d).abs() < 0.03, "{got:?} vs {want:?}");
        }
    }

    #[test]
    fn cuts_long_silence_with_padding() {
        let a = [tone(1.0, 150.0, 0.5), vec![0.0; 16_000], tone(1.0, 150.0, 0.5)].concat();
        near(&keep_ranges(&a, &Settings::default()), &[(0.0, 1.1), (1.9, 3.0)]);
    }

    #[test]
    fn keeps_short_silence_and_trims_ends() {
        let a = [vec![0.0; 16_000], tone(1.0, 150.0, 0.5), vec![0.0; 4_800], tone(1.0, 150.0, 0.5), vec![0.0; 16_000]].concat();
        near(&keep_ranges(&a, &Settings::default()), &[(0.9, 3.4)]);
    }

    #[test]
    fn threshold_decides_what_is_silent() {
        let a = [tone(1.0, 150.0, 0.5), tone(1.0, 150.0, 0.003), tone(1.0, 150.0, 0.5)].concat(); // -53 dB middle
        assert_eq!(keep_ranges(&a, &Settings::default()).len(), 2);
        assert_eq!(keep_ranges(&a, &Settings { threshold_db: -60.0, ..Default::default() }).len(), 1);
    }

    #[test]
    fn breaths_cut_only_when_asked() {
        let a = [tone(1.0, 150.0, 0.5), noise(0.4, 0.05), tone(1.0, 150.0, 0.5)].concat(); // breath ~ -31 dB
        near(&keep_ranges(&a, &Settings::default()), &[(0.0, 2.4)]);
        let s = Settings { breaths: true, ..Default::default() };
        near(&keep_ranges(&a, &s), &[(0.0, 1.1), (1.3, 2.4)]);
    }

    #[test]
    fn quiet_speech_is_not_a_breath() {
        // A quiet voiced phrase (pitched) between loud ones stays.
        let a = [tone(1.0, 150.0, 0.5), tone(0.4, 180.0, 0.05), tone(1.0, 150.0, 0.5)].concat();
        let s = Settings { breaths: true, ..Default::default() };
        near(&keep_ranges(&a, &s), &[(0.0, 2.4)]);
    }

    #[test]
    fn all_silent_keeps_nothing() {
        assert!(keep_ranges(&vec![0.0; 32_000], &Settings::default()).is_empty());
        assert!(keep_ranges(&[], &Settings::default()).is_empty());
    }
}
