//! The soundtrack, and getting it out of the speakers.
//!
//! Like the picture, the music is a function of the film clock: [`render`]
//! synthesizes all of it from the cue times in [`score`], with no samples and
//! no audio dependency. Playing it is deliberately crude — the PCM is written
//! to a temporary WAV file from wherever the film is, and handed to whichever
//! command-line player the system has. A machine with none of them gets a
//! silent film.

use std::{
    f64::consts::TAU,
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, channel},
    thread,
    time::Duration,
};

use crate::score::{self, BEAT};

const RATE: f64 = 44_100.0;
/// The film, and its last chord ringing out.
const LENGTH: f64 = score::END + 8.0;

// The hirajoshi scale on D, which is what makes it sound like a koto.
const D2: f64 = 73.42;
const A2: f64 = 110.0;
const BB2: f64 = 116.54;
const D3: f64 = 146.83;
const E3: f64 = 164.81;
const F3: f64 = 174.61;
const A3: f64 = 220.0;
const BB3: f64 = 233.08;
const D4: f64 = 293.66;
const E4: f64 = 329.63;
const F4: f64 = 349.23;
const A4: f64 = 440.0;
const BB4: f64 = 466.16;
const D5: f64 = 587.33;
const E5: f64 = 659.26;
const F5: f64 = 698.46;
const A5: f64 = 880.0;
const BB5: f64 = 932.33;
const D6: f64 = 1174.66;

/// The theme: `(beat, pitch)` over a four-bar phrase.
const THEME: [(f64, f64); 19] = [
    (0.0, D4),
    (1.0, A4),
    (1.5, BB4),
    (2.0, A4),
    (3.0, F4),
    (4.0, E4),
    (4.5, F4),
    (5.0, E4),
    (6.0, D4),
    (7.0, A3),
    (8.0, D4),
    (9.0, F4),
    (9.5, A4),
    (10.0, D5),
    (11.0, BB4),
    (12.0, A4),
    (13.0, F4),
    (13.5, E4),
    (14.0, D4),
];

/// Xorshift noise in `-1.0..1.0`.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        f64::from(self.0) / f64::from(u32::MAX) * 2.0 - 1.0
    }
}

/// The stereo mix every voice is added into.
struct Mix {
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Mix {
    /// Add `seconds` of a voice starting at `at`. `voice` is given the time
    /// since the voice began and returns its sample; `pan` runs from -1 on the
    /// left to 1 on the right.
    fn play(&mut self, at: f64, seconds: f64, pan: f64, mut voice: impl FnMut(f64) -> f64) {
        self.sweep(
            at,
            seconds,
            |_| pan,
            |since| {
                let sample = voice(since);
                (sample, sample)
            },
        );
    }

    /// As [`play`](Self::play), for a voice with two channels of its own and a
    /// pan that moves.
    fn sweep(
        &mut self,
        at: f64,
        seconds: f64,
        pan: impl Fn(f64) -> f64,
        mut voice: impl FnMut(f64) -> (f64, f64),
    ) {
        let start = (at * RATE) as usize;
        for index in 0..(seconds * RATE) as usize {
            let Some(left) = self.left.get_mut(start + index) else {
                break;
            };
            let since = index as f64 / RATE;
            let (l, r) = voice(since);
            let pan = pan(since / seconds);
            *left += (l * (1.0 - pan).min(1.0)) as f32;
            self.right[start + index] += (r * (1.0 + pan).min(1.0)) as f32;
        }
    }

    /// A taiko: a sine falling in pitch, with the stick's crack on the front.
    fn taiko(&mut self, at: f64, gain: f64) {
        let mut phase = 0.0;
        let mut noise = Noise(0x9E37_79B9);
        let mut skin = 0.0;
        self.play(at, 1.1, 0.0, |t| {
            phase += TAU * (46.0 + 95.0 * (-t * 24.0).exp()) / RATE;
            skin += 0.2 * (noise.next() - skin);
            gain * (phase.sin() * (-t * 4.5).exp() + skin * (-t * 55.0).exp() * 0.7)
        });
    }

    /// A plucked string, Karplus-Strong: a burst of noise going round a delay
    /// line that averages it away.
    fn koto(&mut self, at: f64, pitch: f64, gain: f64, pan: f64) {
        let mut noise = Noise(pitch.to_bits() as u32 | 1);
        let length = (RATE / pitch - 0.5).round().max(2.0) as usize;
        let mut string: Vec<f64> = (0..length).map(|_| noise.next()).collect();
        // A softer pluck: take the edge off the burst.
        for index in 1..length {
            string[index] = (string[index] + string[index - 1]) / 2.0;
        }
        let mut index = 0;
        self.play(at, 2.4, pan, |_| {
            let next = (index + 1) % length;
            let sample = string[index];
            string[index] = 0.997 * (string[index] + string[next]) / 2.0;
            index = next;
            gain * sample
        });
    }

    /// A struck bell: three partials that are not quite harmonics.
    fn chime(&mut self, at: f64, pitch: f64, gain: f64, pan: f64) {
        self.play(at, 1.4, pan, |t| {
            let partial = |ratio: f64, level: f64, decay: f64| {
                (TAU * pitch * ratio * t).sin() * level * (-t * decay).exp()
            };
            gain * (partial(1.0, 1.0, 5.0) + partial(2.76, 0.4, 9.0) + partial(5.4, 0.2, 14.0))
        });
    }

    /// Slow voices holding a chord.
    fn pad(&mut self, at: f64, seconds: f64, chord: &[f64], gain: f64) {
        for (voice, pitch) in chord.iter().enumerate() {
            let pan = if voice % 2 == 0 { -0.5 } else { 0.5 };
            self.play(at, seconds, pan, |t| {
                let envelope = (t / 0.9).min(1.0) * ((seconds - t) / 1.4).min(1.0);
                let tone = (TAU * pitch * 1.003 * t).sin()
                    + (TAU * pitch * 0.997 * t).sin()
                    + 0.3 * (TAU * pitch * 2.0 * t).sin();
                gain * envelope * tone * (1.0 + 0.2 * (t * 2.1 + voice as f64).sin())
            });
        }
    }

    /// The floor under everything: two saws a hair apart, heavily filtered.
    fn drone(&mut self, at: f64, seconds: f64, gain: impl Fn(f64) -> f64) {
        let mut filtered = 0.0;
        self.play(at, seconds, 0.0, |t| {
            let saw = |pitch: f64| (t * pitch).fract() * 2.0 - 1.0;
            let raw = saw(D2) + saw(D2 * 1.006) + 1.5 * (TAU * D2 / 2.0 * t).sin();
            filtered += 0.03 * (raw - filtered);
            filtered * gain(at + t)
        });
    }

    /// Wind: noise through a filter that will not hold still.
    fn wind(&mut self, at: f64, seconds: f64, gain: impl Fn(f64) -> f64) {
        let mut noise = Noise(0xBADC_0FFE);
        let mut low = [0.0; 2];
        let mut floor = [0.0; 2];
        self.sweep(
            at,
            seconds,
            |_| 0.0,
            |t| {
                let mut channel = |side: usize| {
                    let gust = 0.035 + 0.025 * (t * (0.31 + side as f64 * 0.07)).sin();
                    low[side] += gust * (noise.next() - low[side]);
                    floor[side] += 0.004 * (low[side] - floor[side]);
                    (low[side] - floor[side]) * gain(at + t) * (1.0 + 0.4 * (t * 0.5).sin())
                };
                (channel(0), channel(1))
            },
        );
    }

    /// Something going past: noise through a band that climbs or falls, moving
    /// from one side to the other.
    fn whoosh(&mut self, at: f64, seconds: f64, gain: f64, band: (f64, f64), pan: (f64, f64)) {
        let mut noise = Noise(0x1234_5677);
        let (mut low, mut mid) = (0.0, 0.0);
        self.sweep(
            at,
            seconds,
            |x| pan.0 + (pan.1 - pan.0) * x,
            |t| {
                let x = t / seconds;
                let cutoff = band.0 * (band.1 / band.0).powf(x);
                let f = 2.0 * (std::f64::consts::PI * cutoff / RATE).sin();
                low += f * mid;
                mid += f * (noise.next() - low - 0.6 * mid);
                let sample = gain * mid * (std::f64::consts::PI * x).sin().powi(2);
                (sample, sample)
            },
        );
    }

    /// The explosion: a sub falling away under a long burst of dark noise.
    fn boom(&mut self, at: f64, gain: f64) {
        let mut phase = 0.0;
        let mut noise = Noise(0x0BAD_5EED);
        let mut dark = 0.0;
        self.play(at, 4.0, 0.0, |t| {
            phase += TAU * (30.0 + 75.0 * (-t * 5.0).exp()) / RATE;
            dark += (0.02 + 0.5 * (-t * 7.0).exp()) * (noise.next() - dark);
            gain * (phase.sin() * (-t * 1.5).exp() + dark * (-t * 2.2).exp() * 0.9)
        });
    }

    /// A mouse button.
    fn click(&mut self, at: f64, gain: f64) {
        let mut noise = Noise(0x00C1_1C4B);
        self.play(at, 0.05, 0.0, |t| {
            gain * (noise.next() * (-t * 900.0).exp()
                + (TAU * 1800.0 * t).sin() * (-t * 220.0).exp())
        });
    }

    /// One square-wave note from an 8-bit sound chip.
    fn blip(&mut self, at: f64, seconds: f64, pitch: f64, gain: f64) {
        self.play(at, seconds, 0.0, |t| {
            let square = if (t * pitch).fract() < 0.5 { 1.0 } else { -1.0 };
            gain * square * ((seconds - t) / 0.01).min(1.0)
        });
    }

    /// Part of the theme: the notes from beat `from` to beat `to`, the phrase
    /// starting at `at`.
    fn theme(&mut self, at: f64, beats: (f64, f64), octave: f64, gain: f64) {
        for (index, (beat, pitch)) in THEME.iter().enumerate() {
            if *beat >= beats.0 && *beat < beats.1 {
                let pan = if index % 2 == 0 { -0.25 } else { 0.25 };
                self.koto(at + (beat - beats.0) * BEAT, pitch * octave, gain, pan);
            }
        }
    }

    /// A bar of drums starting at `at`.
    fn drums(&mut self, at: f64, gain: f64) {
        for (beat, level) in [(0.0, 1.0), (1.5, 0.55), (2.5, 0.7), (3.0, 0.5)] {
            self.taiko(at + beat * BEAT, gain * level);
        }
    }
}

/// The whole soundtrack, as interleaved 16-bit stereo, little-endian.
fn render() -> Vec<u8> {
    let samples = (LENGTH * RATE) as usize;
    let mut mix = Mix {
        left: vec![0.0; samples],
        right: vec![0.0; samples],
    };
    let bar = |bar: f64| bar * score::BAR;

    // Boot: keys, a program chanting, and the tube switching off.
    for (from, to, keys) in [(0.9, 2.7, 31), (2.9, 3.4, 10), (3.7, 4.0, 3)] {
        for key in 0..keys {
            let at = from + (to - from) * f64::from(key) / f64::from(keys);
            mix.blip(at, 0.012, 1_400.0 + f64::from(key % 5) * 90.0, 0.05);
        }
    }
    let mut at = 4.3;
    let mut step = 0;
    while at < 6.5 {
        let rot = (at - 4.3) / 2.2;
        let pitch =
            [D4, F4, A4, D5][step % 4] * (1.0 + rot * rot * 0.09 * f64::from(step as i32 % 3 - 1));
        mix.blip(at, 0.05, pitch, 0.05 + 0.03 * rot);
        at += 0.16 - 0.1 * rot;
        step += 1;
    }
    mix.blip(6.5, 0.4, 55.0, 0.14);
    mix.play(7.0, 0.45, 0.0, |t| {
        0.12 * (TAU * (2_400.0 * (-t * 9.0).exp() + 50.0) * t).sin() * (1.0 - t / 0.45)
    });
    mix.click(7.0, 0.5);

    mix.drone(4.3, LENGTH - 4.3, |t| {
        let swell = ((t - 4.3) / 4.0).min(1.0);
        let level = if t < score::BOOM {
            0.035 + 0.05 * ((t - score::CHAOS) / 10.0).max(0.0)
        } else {
            0.04
        };
        swell * level * ((LENGTH - t) / 4.0).min(1.0)
    });
    mix.wind(score::CHAOS, LENGTH - score::CHAOS, |t| {
        let level = if t > score::FINALE { 0.09 } else { 0.05 };
        level * ((t - score::CHAOS) / 2.0).min(1.0) * ((LENGTH - t) / 5.0).min(1.0)
    });

    // The old way: a pulse that quickens, wires that clash, and alarms.
    let mut at = score::CHAOS;
    while at < score::BOOM - 0.05 {
        let near = (at - score::CHAOS) / 10.0;
        mix.taiko(at, 0.35 + 0.5 * near * near);
        at += match at {
            at if at < 12.5 => 2.0 * BEAT,
            at if at < 15.0 => BEAT,
            at if at < 16.25 => BEAT / 2.0,
            _ => BEAT / 4.0,
        };
    }
    mix.click(score::CHAOS_CLICK, 0.5);
    for (wire, pitch) in [E4, BB4, F4, 493.88, 311.13, A4, E5, BB3, 554.37, 311.13]
        .iter()
        .enumerate()
    {
        let pan = if wire % 2 == 0 { -0.6 } else { 0.6 };
        mix.koto(score::CHAOS + 2.8 + wire as f64 * 0.42, *pitch, 0.22, pan);
    }
    for (alarm, at) in [3.4, 4.1, 4.8, 5.5, 6.1, 6.6, 7.4, 7.9, 8.3, 8.6, 8.9, 9.1]
        .iter()
        .enumerate()
    {
        let at = score::CHAOS + at;
        mix.chime(at, BB4, 0.1, 0.5);
        mix.chime(at + 0.02, 493.88 * (1.0 + alarm as f64 * 0.01), 0.1, -0.5);
    }
    mix.whoosh(score::BOOM - 2.6, 2.6, 0.5, (200.0, 5_000.0), (0.0, 0.0));

    // The blast, and the title coming together out of it.
    mix.boom(score::BOOM, 1.3);
    mix.taiko(score::BOOM, 1.0);
    let run = [D3, E3, F3, A3, BB3, D4, E4, F4, A4, BB4, D5, E5, F5, A5];
    for (index, pitch) in run.iter().enumerate() {
        let x = index as f64 / (run.len() - 1) as f64;
        let at = score::BOOM + 1.5 + 2.1 * (1.0 - (1.0 - x).powf(1.6));
        mix.koto(at, *pitch, 0.16 + 0.1 * x, x - 0.5);
    }
    mix.taiko(score::TITLE_LANDS, 0.95);
    mix.boom(score::TITLE_LANDS, 0.45);
    mix.pad(score::TITLE_LANDS, 4.2, &[D3, A3, D4, F4], 0.035);
    for (beat, pitch) in [(2.0, A4), (3.0, D5), (4.0, A4), (5.0, F4)] {
        mix.koto(score::TITLE_LANDS + beat * BEAT, pitch, 0.24, 0.0);
    }

    // The monk: the theme, alone.
    mix.pad(score::MONK, 5.2, &[D3, A3, F4], 0.03);
    mix.theme(score::MONK, (0.0, 8.0), 1.0, 0.3);
    mix.taiko(score::MONK, 0.45);
    mix.taiko(score::MONK + score::BAR, 0.35);
    mix.chime(score::MONK + 1.4, A5, 0.06, -0.4);
    // The rat: it picks up.
    mix.whoosh(score::RAT + 0.1, 0.8, 0.3, (500.0, 2_500.0), (0.8, -0.3));
    mix.pad(score::RAT, 5.2, &[BB2, D3, F3, D4], 0.03);
    mix.theme(score::RAT, (8.0, 16.0), 1.0, 0.3);
    mix.drums(score::RAT, 0.4);
    mix.drums(score::RAT + score::BAR, 0.4);
    mix.whoosh(score::RAT + 1.7, 1.3, 0.14, (800.0, 4_000.0), (0.0, 0.6));
    mix.chime(score::RAT + 3.1, D6, 0.06, 0.4);
    // The ninja: a hush, a click, a blade.
    mix.click(score::NINJA_CLICK, 0.5);
    mix.whoosh(
        score::NINJA_SLASH - 0.12,
        0.3,
        0.9,
        (6_000.0, 900.0),
        (0.7, -0.7),
    );
    mix.taiko(score::NINJA_SLASH, 0.95);
    mix.theme(score::NINJA + score::BAR, (0.0, 4.0), 0.5, 0.36);
    mix.taiko(score::NINJA + score::BAR + 2.0 * BEAT, 0.5);
    mix.taiko(score::NINJA + score::BAR + 3.0 * BEAT, 0.6);
    mix.taiko(score::NINJA + score::BAR + 3.5 * BEAT, 0.7);

    // The loop: drums under the theme, and every turn heard.
    for index in 0..6 {
        let at = score::LOOP + bar(f64::from(index));
        mix.drums(at, 0.45);
        let chord: &[f64] = if index % 4 < 2 {
            &[D3, A3, F4]
        } else {
            &[BB2, F3, D4]
        };
        mix.pad(at, score::BAR + 0.4, chord, 0.028);
    }
    mix.theme(score::LOOP, (0.0, 16.0), 1.0, 0.28);
    mix.theme(score::LOOP + bar(4.0), (0.0, 8.0), 1.0, 0.28);
    let climb = [D5, E5, F5, A5, BB5, D6, 1318.51, 1396.91, 1760.0];
    for (index, turn) in score::TURNS.windows(2).enumerate() {
        let length = turn[1] - turn[0];
        mix.click(turn[0], 0.4);
        mix.whoosh(
            turn[0] + 0.12 * length,
            0.3 * length,
            0.3,
            (700.0, 3_000.0),
            (0.6, -0.6),
        );
        mix.chime(turn[0] + score::UPDATE_AT * length, D4, 0.14, -0.6);
        mix.chime(
            turn[0] + score::REPAINT_AT * length,
            climb[index],
            0.09,
            0.6,
        );
    }

    // What it makes possible: everything playing.
    mix.taiko(score::POWERS, 0.9);
    mix.boom(score::POWERS, 0.35);
    let chords: [&[f64]; 4] = [
        &[D3, A3, F4],
        &[BB2, F3, D4],
        &[F3, A3, 261.63],
        &[A2, E3, A3, E4],
    ];
    for index in 0..8 {
        let at = score::POWERS + bar(f64::from(index));
        mix.drums(at, 0.5);
        mix.taiko(at + 3.5 * BEAT, 0.3);
        mix.pad(at, score::BAR + 0.4, chords[index as usize / 2], 0.03);
    }
    mix.theme(score::POWERS, (0.0, 16.0), 1.0, 0.28);
    mix.theme(score::POWERS + bar(4.0), (0.0, 16.0), 1.0, 0.24);
    mix.theme(score::POWERS + bar(4.0), (0.0, 16.0), 2.0, 0.12);
    // Rewind: the tape heard moving, and a test passing where it stops.
    for pair in score::REWIND.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if (from.1 - to.1).abs() > 0.5 {
            let band = if to.1 < from.1 {
                (3_000.0, 500.0)
            } else {
                (500.0, 3_000.0)
            };
            mix.whoosh(
                score::POWERS + from.0,
                to.0 - from.0,
                0.22,
                band,
                (-0.5, 0.5),
            );
            mix.chime(score::POWERS + to.0 + 0.05, A5, 0.07, 0.5);
        }
    }
    for (slot, pitch) in [D4, F4, A4, D5, F5].iter().enumerate() {
        mix.koto(
            score::POWERS + 5.7 + slot as f64 * 0.5,
            *pitch,
            0.3,
            slot as f64 * 0.4 - 0.8,
        );
    }
    let layers = score::POWERS + 10.0;
    mix.chime(layers + 1.0, A5, 0.09, -0.3);
    mix.chime(layers + 2.0, A5, 0.09, 0.6);
    mix.chime(layers + 2.12, D6, 0.09, 0.6);
    mix.taiko(layers + 3.2, 0.8);
    let yours = score::POWERS + 15.0;
    for key in 0..22 {
        mix.blip(yours + 0.2 + f64::from(key) * 0.022, 0.01, 1_500.0, 0.035);
    }
    for (edit, pitch) in [D5, F5, A5, D6].iter().enumerate() {
        mix.chime(yours + 1.5 + edit as f64 * 0.7, *pitch, 0.1, 0.3);
    }

    // The finale: the drums stop, the line lands, the theme comes home.
    mix.pad(score::FINALE, 10.5, &[D3, A3, D4, F4, A4], 0.03);
    mix.pad(score::FINALE_DROP, 9.0, &[D2, A2, D3], 0.05);
    mix.taiko(score::FINALE, 0.6);
    mix.taiko(score::FINALE_DROP, 1.0);
    mix.boom(score::FINALE_DROP, 0.7);
    for (beat, pitch) in [
        (2.0, A4),
        (3.0, F4),
        (4.0, E4),
        (6.0, D4),
        (6.0, D3),
        (8.0, D5),
        (10.0, D2),
    ] {
        mix.koto(score::FINALE_DROP + beat * BEAT, pitch, 0.3, 0.0);
    }

    // A half-beat echo, bounced from side to side: the room.
    let delay = (BEAT / 2.0 * RATE) as usize;
    for index in delay..samples {
        mix.left[index] += 0.26 * mix.right[index - delay];
        mix.right[index] += 0.26 * mix.left[index - delay];
    }

    let mut pcm = Vec::with_capacity(samples * 4);
    for index in 0..samples {
        for sample in [mix.left[index], mix.right[index]] {
            // A soft ceiling rather than a hard one.
            let level = (f64::from(sample) * 1.15).tanh();
            pcm.extend_from_slice(&((level * 30_000.0) as i16).to_le_bytes());
        }
    }
    pcm
}

/// A 16-bit stereo WAV header for `bytes` of PCM.
fn wav_header(bytes: usize) -> Vec<u8> {
    let rate = RATE as u32;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + bytes as u32).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&rate.to_le_bytes());
    header.extend_from_slice(&(rate * 4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(bytes as u32).to_le_bytes());
    header
}

/// Players to try, in order, each with the arguments that keep it quiet.
const PLAYERS: [(&str, &[&str]); 5] = [
    ("pw-play", &[]),
    ("paplay", &[]),
    ("aplay", &["-q"]),
    ("afplay", &[]),
    ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
];

/// How far the player may drift from the film before it is restarted.
const DRIFT: f64 = 0.2;
/// The shortest time between two restarts, so scrubbing does not start a
/// player for every cell the pointer crosses.
const SETTLE: Duration = Duration::from_millis(180);

/// The soundtrack, kept in step with the film.
///
/// There is nothing to tell it: [`follow`](Self::follow) is handed the film's
/// position every frame and makes the player agree, restarting it when the
/// film was paused, resumed, or moved.
pub struct Sound {
    /// The synthesizer, until it has delivered.
    rendering: Option<Receiver<Vec<u8>>>,
    pcm: Vec<u8>,
    muted: bool,
    player: Option<Child>,
    /// Which of [`PLAYERS`] to use. Past the end means none of them ran.
    choice: usize,
    /// When the player was started, and where in the film it started from.
    started: Option<(Duration, f64)>,
    path: PathBuf,
}

impl Sound {
    /// Start synthesizing in the background. The film does not wait for it.
    pub fn new(enabled: bool) -> Self {
        let rendering = enabled.then(|| {
            let (sender, receiver) = channel();
            thread::spawn(move || {
                let _ = sender.send(render());
            });
            receiver
        });
        Self {
            rendering,
            pcm: Vec::new(),
            muted: !enabled,
            player: None,
            choice: 0,
            started: None,
            path: std::env::temp_dir().join(format!("ui-f-state-{}.wav", std::process::id())),
        }
    }

    pub const fn muted(&self) -> bool {
        self.muted
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if !self.muted && self.rendering.is_none() && self.pcm.is_empty() {
            *self = Self::new(true);
        }
    }

    /// Make the player agree with a film that is at `film`, and `playing` or
    /// not. `now` is the clock the film runs on.
    pub fn follow(&mut self, film: f64, playing: bool, now: Duration) {
        if let Some(receiver) = &self.rendering
            && let Ok(pcm) = receiver.try_recv()
        {
            self.pcm = pcm;
            self.rendering = None;
        }
        let wanted = playing && !self.muted && !self.pcm.is_empty() && film < LENGTH;
        let on_time = self.started.is_some_and(|(at, from)| {
            (from + now.saturating_sub(at).as_secs_f64() - film).abs() < DRIFT
        });
        if wanted && on_time {
            return;
        }
        if !wanted {
            self.stop();
            self.started = None;
            return;
        }
        if self
            .started
            .is_some_and(|(at, _)| now.saturating_sub(at) < SETTLE)
        {
            return;
        }
        self.stop();
        self.started = Some((now, film));
        // Whole frames only: four bytes each.
        let offset = ((film.max(0.0) * RATE) as usize * 4).min(self.pcm.len());
        let rest = &self.pcm[offset..];
        let mut wav = wav_header(rest.len());
        wav.extend_from_slice(rest);
        if fs::write(&self.path, wav).is_err() {
            return;
        }
        while let Some((program, arguments)) = PLAYERS.get(self.choice) {
            let spawned = Command::new(program)
                .args(*arguments)
                .arg(&self.path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if let Ok(child) = spawned {
                self.player = Some(child);
                return;
            }
            self.choice += 1;
        }
    }

    fn stop(&mut self) {
        if let Some(mut player) = self.player.take() {
            let _ = player.kill();
            let _ = player.wait();
        }
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mix has to leave headroom: a soundtrack that sits on the ceiling
    /// is distorted, and one that never leaves the floor is not there.
    #[test]
    fn the_soundtrack_is_audible_and_not_clipped() {
        let pcm = render();
        let samples: Vec<f64> = pcm
            .chunks_exact(2)
            .map(|bytes| f64::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0)
            .collect();
        let second = RATE as usize * 2;
        for (index, chunk) in samples.chunks(second).enumerate() {
            let rms = (chunk.iter().map(|s| s * s).sum::<f64>() / chunk.len() as f64).sqrt();
            let peak = chunk.iter().fold(0.0f64, |peak, s| peak.max(s.abs()));
            println!("{index:>3}s  rms {rms:.3}  peak {peak:.3}");
            assert!(peak < 0.93, "second {index} is on the ceiling");
        }
        let rms = (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt();
        assert!(rms > 0.03, "the mix is too quiet to hear: {rms}");
    }
}
