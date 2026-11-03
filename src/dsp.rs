//! The ZB-1 sound engine. Pure DSP, no plugin framework code, so it can be
//! unit-tested and rendered offline.

use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Saw,
    Square,
    Sine,
    Organ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubWave {
    Sine,
    Square,
}

/// Every control the engine reads. Filled from the plugin parameters each block.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub osc1_wave: Wave,
    pub osc2_wave: Wave,
    pub osc2_octave: i32,
    pub osc2_detune_cents: f32,
    pub osc_mix: f32,
    pub sub_level: f32,
    pub sub_wave: SubWave,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub env_amount: f32,
    pub key_track: f32,
    pub f_attack: f32,
    pub f_decay: f32,
    pub f_sustain: f32,
    pub f_release: f32,
    pub a_attack: f32,
    pub a_decay: f32,
    pub a_sustain: f32,
    pub a_release: f32,
    pub velocity_amount: f32,
    pub glide_ms: f32,
    pub legato: bool,
    pub drive: f32,
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            osc1_wave: Wave::Saw,
            osc2_wave: Wave::Square,
            osc2_octave: 0,
            osc2_detune_cents: 7.0,
            osc_mix: 0.3,
            sub_level: 0.5,
            sub_wave: SubWave::Sine,
            cutoff_hz: 400.0,
            resonance: 0.3,
            env_amount: 0.5,
            key_track: 0.5,
            f_attack: 0.001,
            f_decay: 0.25,
            f_sustain: 0.2,
            f_release: 0.15,
            a_attack: 0.002,
            a_decay: 0.4,
            a_sustain: 0.8,
            a_release: 0.12,
            velocity_amount: 0.5,
            glide_ms: 0.0,
            legato: true,
            drive: 0.2,
            volume: 0.7,
        }
    }
}

// ---------------------------------------------------------------- oscillator

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[derive(Default, Clone, Copy)]
struct Osc {
    phase: f32,
}

impl Osc {
    #[inline]
    fn next(&mut self, wave: Wave, dt: f32) -> f32 {
        let t = self.phase;
        let out = match wave {
            Wave::Saw => (2.0 * t - 1.0) - poly_blep(t, dt),
            Wave::Square => {
                let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                v -= poly_blep((t + 0.5).fract(), dt);
                v * 0.8
            }
            Wave::Sine => (TAU * t).sin(),
            Wave::Organ => {
                // Drawbar-ish additive organ bass: fundamental plus a few
                // harmonics, the classic late-80s workstation bass flavour.
                let p = TAU * t;
                let v = p.sin() + 0.55 * (2.0 * p).sin() + 0.32 * (3.0 * p).sin()
                    + 0.16 * (4.0 * p).sin()
                    + 0.08 * (6.0 * p).sin();
                v * 0.62
            }
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

// ------------------------------------------------------------------ envelope

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy)]
struct Env {
    stage: Stage,
    level: f32,
}

impl Default for Env {
    fn default() -> Self {
        Self { stage: Stage::Idle, level: 0.0 }
    }
}

#[inline]
fn coef(seconds: f32, sr: f32) -> f32 {
    // Reaches ~99% of the target within `seconds`.
    1.0 - (-4.6 / (seconds.max(0.0005) * sr)).exp()
}

impl Env {
    fn gate_on(&mut self) {
        // Attack starts from wherever we are now so retriggers never click.
        self.stage = Stage::Attack;
    }
    fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }
    #[inline]
    fn next(&mut self, a: f32, d: f32, s: f32, r: f32, sr: f32) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                // Aim past 1.0 so the curve is snappy rather than asymptotic.
                self.level += (1.25 - self.level) * coef(a, sr);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => self.level += (s - self.level) * coef(d, sr),
            Stage::Release => {
                self.level += (0.0 - self.level) * coef(r, sr);
                if self.level < 1e-5 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

// -------------------------------------------------------------------- filter

/// 24 dB/oct zero-delay-feedback transistor-ladder-style low-pass with a
/// saturating feedback path.
#[derive(Default, Clone, Copy)]
struct Ladder {
    s: [f32; 4],
}

impl Ladder {
    #[inline]
    fn process(&mut self, x: f32, cutoff: f32, res: f32, sr: f32) -> f32 {
        let fc = cutoff.clamp(20.0, sr * 0.45);
        let g = (PI * fc / sr).tan();
        let big_g = g / (1.0 + g);
        let beta = 1.0 / (1.0 + g);
        let k = res * 3.9;

        let s_sum = big_g * big_g * big_g * beta * self.s[0]
            + big_g * big_g * beta * self.s[1]
            + big_g * beta * self.s[2]
            + beta * self.s[3];
        let g4 = big_g * big_g * big_g * big_g;
        // Partial bass compensation so resonance doesn't hollow out the low end.
        let comp = 1.0 + 0.6 * k;
        let mut u = (x * comp - k * s_sum) / (1.0 + k * g4);
        u = fast_tanh(u);

        let mut inp = u;
        for s in self.s.iter_mut() {
            let v = (inp - *s) * big_g;
            let y = v + *s;
            *s = y + v;
            inp = y;
        }
        inp
    }
}

#[inline]
fn fast_tanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

// -------------------------------------------------------------------- engine

const MAX_HELD: usize = 16;

pub struct Engine {
    sr: f32,
    osc1: Osc,
    osc2: Osc,
    sub: Osc,
    amp_env: Env,
    filt_env: Env,
    filter: Ladder,
    held: [u8; MAX_HELD],
    held_len: usize,
    current_note: f32,
    target_note: f32,
    velocity: f32,
    dc_x: f32,
    dc_y: f32,
}

impl Engine {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            osc1: Osc::default(),
            osc2: Osc::default(),
            sub: Osc::default(),
            amp_env: Env::default(),
            filt_env: Env::default(),
            filter: Ladder::default(),
            held: [0; MAX_HELD],
            held_len: 0,
            current_note: 36.0,
            target_note: 36.0,
            velocity: 1.0,
            dc_x: 0.0,
            dc_y: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        *self = Self::new(sr);
    }

    pub fn reset(&mut self) {
        let sr = self.sr;
        *self = Self::new(sr);
    }

    pub fn is_silent(&self) -> bool {
        self.amp_env.stage == Stage::Idle
    }

    pub fn note_on(&mut self, note: u8, velocity: f32, s: &Settings) {
        let was_held = self.held_len > 0;
        self.remove_held(note);
        if self.held_len < MAX_HELD {
            self.held[self.held_len] = note;
            self.held_len += 1;
        }
        self.target_note = note as f32;

        let gliding = s.glide_ms > 0.5 && (was_held || !s.legato);
        if !gliding || self.amp_env.stage == Stage::Idle {
            self.current_note = self.target_note;
        }

        if !(s.legato && was_held) {
            self.velocity = velocity;
            if self.amp_env.stage == Stage::Idle {
                // Fresh note: line up the oscillators for a consistent, punchy start.
                self.osc1.phase = 0.0;
                self.osc2.phase = 0.0;
                self.sub.phase = 0.0;
            }
            self.amp_env.gate_on();
            self.filt_env.gate_on();
        }
    }

    pub fn note_off(&mut self, note: u8) {
        self.remove_held(note);
        if self.held_len > 0 {
            // Fall back to the most recent still-held key (last-note priority).
            self.target_note = self.held[self.held_len - 1] as f32;
        } else {
            self.amp_env.gate_off();
            self.filt_env.gate_off();
        }
    }

    pub fn all_notes_off(&mut self) {
        self.held_len = 0;
        self.amp_env.gate_off();
        self.filt_env.gate_off();
    }

    fn remove_held(&mut self, note: u8) {
        let mut j = 0;
        for i in 0..self.held_len {
            if self.held[i] != note {
                self.held[j] = self.held[i];
                j += 1;
            }
        }
        self.held_len = j;
    }

    #[inline]
    pub fn next_sample(&mut self, s: &Settings) -> f32 {
        let sr = self.sr;

        // Glide in the pitch domain so it sounds even across the keyboard.
        if s.glide_ms > 0.5 {
            self.current_note += (self.target_note - self.current_note) * coef(s.glide_ms * 0.001, sr);
        } else {
            self.current_note = self.target_note;
        }

        let freq = 440.0 * 2f32.powf((self.current_note - 69.0) / 12.0);
        let f2 = freq * 2f32.powf(s.osc2_octave as f32 + s.osc2_detune_cents / 1200.0);
        let dt1 = (freq / sr).min(0.49);
        let dt2 = (f2 / sr).min(0.49);
        let dts = (freq * 0.5 / sr).min(0.49);

        let o1 = self.osc1.next(s.osc1_wave, dt1);
        let o2 = self.osc2.next(s.osc2_wave, dt2);
        let sub_raw = self.sub.next(
            match s.sub_wave {
                SubWave::Sine => Wave::Sine,
                SubWave::Square => Wave::Square,
            },
            dts,
        );

        let osc = o1 * (1.0 - s.osc_mix) + o2 * s.osc_mix;

        let fenv = self.filt_env.next(s.f_attack, s.f_decay, s.f_sustain, s.f_release, sr);
        let aenv = self.amp_env.next(s.a_attack, s.a_decay, s.a_sustain, s.a_release, sr);

        let vel_scale = 1.0 - s.velocity_amount + s.velocity_amount * self.velocity;

        // Envelope sweeps up to 7 octaves; key tracking follows the keyboard around C2.
        let octaves = s.env_amount * 7.0 * fenv * vel_scale
            + s.key_track * (self.current_note - 36.0) / 12.0;
        let cutoff = s.cutoff_hz * 2f32.powf(octaves);

        let filtered = self.filter.process(osc * 0.8, cutoff, s.resonance, sr);

        // Sub goes around the filter so the low end stays solid at any cutoff.
        let mut x = filtered + sub_raw * s.sub_level * 0.9;

        let dgain = 1.0 + s.drive * 6.0;
        x = fast_tanh(x * dgain) / fast_tanh(dgain).max(0.5);

        x *= aenv * vel_scale;

        // DC blocker (~10 Hz).
        let r = 1.0 - (TAU * 10.0 / sr);
        let y = x - self.dc_x + r * self.dc_y;
        self.dc_x = x;
        self.dc_y = y;

        y * s.volume
    }
}
