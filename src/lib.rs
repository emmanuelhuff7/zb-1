use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub mod dsp;
mod editor;
mod font;
pub mod presets;

use dsp::{Engine, Settings, SubWave, Wave};

pub const SCOPE_LEN: usize = 512;

/// Shared between the audio thread and the GUI for the little oscilloscope.
pub struct Scope {
    pub samples: Vec<AtomicF32>,
    pub write: AtomicUsize,
}

impl Scope {
    fn new() -> Self {
        Self {
            samples: (0..SCOPE_LEN).map(|_| AtomicF32::new(0.0)).collect(),
            write: AtomicUsize::new(0),
        }
    }
}

#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum OscWave {
    #[name = "SAW"]
    Saw,
    #[name = "SQUARE"]
    Square,
    #[name = "SINE"]
    Sine,
    #[name = "ORGAN"]
    Organ,
}

impl From<OscWave> for Wave {
    fn from(w: OscWave) -> Self {
        match w {
            OscWave::Saw => Wave::Saw,
            OscWave::Square => Wave::Square,
            OscWave::Sine => Wave::Sine,
            OscWave::Organ => Wave::Organ,
        }
    }
}
impl From<Wave> for OscWave {
    fn from(w: Wave) -> Self {
        match w {
            Wave::Saw => OscWave::Saw,
            Wave::Square => OscWave::Square,
            Wave::Sine => OscWave::Sine,
            Wave::Organ => OscWave::Organ,
        }
    }
}

#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SubWaveParam {
    #[name = "SINE"]
    Sine,
    #[name = "SQUARE"]
    Square,
}

fn time_param(name: &str, default_s: f32, max_s: f32) -> FloatParam {
    FloatParam::new(
        name,
        default_s,
        FloatRange::Skewed { min: 0.001, max: max_s, factor: FloatRange::skew_factor(-2.2) },
    )
    .with_value_to_string(Arc::new(|v| {
        if v < 1.0 {
            format!("{:.0} MS", v * 1000.0)
        } else {
            format!("{v:.2} S")
        }
    }))
}

fn pct_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_value_to_string(Arc::new(|v| format!("{:.0}%", v * 100.0)))
}

#[derive(Params)]
pub struct ZbParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,
    /// Which factory patch the cursor sits on; saved with the project.
    #[persist = "patch"]
    pub current_preset: Arc<AtomicUsize>,

    #[id = "o1w"]
    pub osc1_wave: EnumParam<OscWave>,
    #[id = "o2w"]
    pub osc2_wave: EnumParam<OscWave>,
    #[id = "o2oct"]
    pub osc2_octave: IntParam,
    #[id = "o2det"]
    pub osc2_detune: FloatParam,
    #[id = "omix"]
    pub osc_mix: FloatParam,
    #[id = "sub"]
    pub sub_level: FloatParam,
    #[id = "subw"]
    pub sub_wave: EnumParam<SubWaveParam>,

    #[id = "cut"]
    pub cutoff: FloatParam,
    #[id = "res"]
    pub resonance: FloatParam,
    #[id = "envamt"]
    pub env_amount: FloatParam,
    #[id = "ktrk"]
    pub key_track: FloatParam,
    #[id = "fa"]
    pub f_attack: FloatParam,
    #[id = "fd"]
    pub f_decay: FloatParam,
    #[id = "fs"]
    pub f_sustain: FloatParam,
    #[id = "fr"]
    pub f_release: FloatParam,

    #[id = "aa"]
    pub a_attack: FloatParam,
    #[id = "ad"]
    pub a_decay: FloatParam,
    #[id = "as"]
    pub a_sustain: FloatParam,
    #[id = "ar"]
    pub a_release: FloatParam,
    #[id = "vel"]
    pub velocity_amount: FloatParam,
    #[id = "glide"]
    pub glide: FloatParam,
    #[id = "legato"]
    pub legato: BoolParam,
    #[id = "drive"]
    pub drive: FloatParam,
    #[id = "vol"]
    pub volume: FloatParam,
}

impl Default for ZbParams {
    fn default() -> Self {
        let p = &presets::all()[0].s;
        Self {
            editor_state: EguiState::from_size(editor::WIDTH, editor::HEIGHT),
            current_preset: Arc::new(AtomicUsize::new(0)),
            osc1_wave: EnumParam::new("Osc 1 Wave", p.osc1_wave.into()),
            osc2_wave: EnumParam::new("Osc 2 Wave", p.osc2_wave.into()),
            osc2_octave: IntParam::new("Osc 2 Octave", p.osc2_octave, IntRange::Linear { min: -1, max: 1 })
                .with_value_to_string(Arc::new(|v| format!("{v:+}"))),
            osc2_detune: FloatParam::new("Osc 2 Detune", p.osc2_detune_cents, FloatRange::Linear { min: 0.0, max: 50.0 })
                .with_value_to_string(Arc::new(|v| format!("{v:.1} CT"))),
            osc_mix: pct_param("Osc Mix", p.osc_mix),
            sub_level: pct_param("Sub Level", p.sub_level),
            sub_wave: EnumParam::new("Sub Wave", SubWaveParam::Sine),

            cutoff: FloatParam::new(
                "Cutoff",
                p.cutoff_hz,
                FloatRange::Skewed { min: 40.0, max: 12000.0, factor: FloatRange::skew_factor(-2.0) },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_value_to_string(Arc::new(|v| {
                if v >= 1000.0 { format!("{:.2} KHZ", v / 1000.0) } else { format!("{v:.0} HZ") }
            })),
            resonance: pct_param("Resonance", p.resonance).with_smoother(SmoothingStyle::Linear(20.0)),
            env_amount: FloatParam::new("Filter Env Amount", p.env_amount, FloatRange::Linear { min: -1.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| format!("{:+.0}%", v * 100.0))),
            key_track: pct_param("Key Track", p.key_track),
            f_attack: time_param("Filter Attack", p.f_attack, 3.0),
            f_decay: time_param("Filter Decay", p.f_decay, 4.0),
            f_sustain: pct_param("Filter Sustain", p.f_sustain),
            f_release: time_param("Filter Release", p.f_release, 4.0),

            a_attack: time_param("Amp Attack", p.a_attack, 3.0),
            a_decay: time_param("Amp Decay", p.a_decay, 4.0),
            a_sustain: pct_param("Amp Sustain", p.a_sustain),
            a_release: time_param("Amp Release", p.a_release, 4.0),
            velocity_amount: pct_param("Velocity", p.velocity_amount),
            glide: FloatParam::new(
                "Glide",
                p.glide_ms,
                FloatRange::Skewed { min: 0.0, max: 1000.0, factor: FloatRange::skew_factor(-1.5) },
            )
            .with_value_to_string(Arc::new(|v| if v < 0.5 { "OFF".into() } else { format!("{v:.0} MS") })),
            legato: BoolParam::new("Legato", p.legato)
                .with_value_to_string(Arc::new(|v| if v { "ON".into() } else { "OFF".into() })),
            drive: pct_param("Drive", p.drive),
            volume: FloatParam::new(
                "Volume",
                p.volume,
                FloatRange::Skewed { min: 0.0, max: 1.0, factor: FloatRange::skew_factor(-1.0) },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(Arc::new(|v| {
                if v <= 0.0001 { "-INF DB".into() } else { format!("{:.1} DB", 20.0 * v.log10()) }
            })),
        }
    }
}

impl ZbParams {
    fn settings(&self) -> Settings {
        Settings {
            osc1_wave: self.osc1_wave.value().into(),
            osc2_wave: self.osc2_wave.value().into(),
            osc2_octave: self.osc2_octave.value(),
            osc2_detune_cents: self.osc2_detune.value(),
            osc_mix: self.osc_mix.value(),
            sub_level: self.sub_level.value(),
            sub_wave: match self.sub_wave.value() {
                SubWaveParam::Sine => SubWave::Sine,
                SubWaveParam::Square => SubWave::Square,
            },
            cutoff_hz: self.cutoff.value(),
            resonance: self.resonance.value(),
            env_amount: self.env_amount.value(),
            key_track: self.key_track.value(),
            f_attack: self.f_attack.value(),
            f_decay: self.f_decay.value(),
            f_sustain: self.f_sustain.value(),
            f_release: self.f_release.value(),
            a_attack: self.a_attack.value(),
            a_decay: self.a_decay.value(),
            a_sustain: self.a_sustain.value(),
            a_release: self.a_release.value(),
            velocity_amount: self.velocity_amount.value(),
            glide_ms: self.glide.value(),
            legato: self.legato.value(),
            drive: self.drive.value(),
            volume: self.volume.value(),
        }
    }
}

pub struct Zb1 {
    params: Arc<ZbParams>,
    engine: Engine,
    scope: Arc<Scope>,
    scope_decim: u32,
}

impl Default for Zb1 {
    fn default() -> Self {
        Self {
            params: Arc::new(ZbParams::default()),
            engine: Engine::new(44100.0),
            scope: Arc::new(Scope::new()),
            scope_decim: 0,
        }
    }
}

impl Plugin for Zb1 {
    const NAME: &'static str = "ZB-1";
    const VENDOR: &'static str = "ZVLE";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.scope.clone())
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.engine.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut next_event = context.next_event();
        let gui_open = self.params.editor_state.is_open();

        for (sample_id, mut channel_samples) in buffer.iter_samples().enumerate() {
            // Settings are cheap to gather; doing it per sample keeps automation sample-accurate.
            let mut s = self.params.settings();
            s.cutoff_hz = self.params.cutoff.smoothed.next();
            s.resonance = self.params.resonance.smoothed.next();
            s.volume = self.params.volume.smoothed.next();

            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { note, velocity, .. } => self.engine.note_on(note, velocity, &s),
                    NoteEvent::NoteOff { note, .. } => self.engine.note_off(note),
                    NoteEvent::Choke { .. } => self.engine.all_notes_off(),
                    _ => {}
                }
                next_event = context.next_event();
            }

            let out = self.engine.next_sample(&s);
            for sample in channel_samples.iter_mut() {
                *sample = out;
            }

            if gui_open {
                self.scope_decim += 1;
                if self.scope_decim >= 4 {
                    self.scope_decim = 0;
                    let w = self.scope.write.load(Ordering::Relaxed);
                    self.scope.samples[w].store(out, Ordering::Relaxed);
                    self.scope.write.store((w + 1) % SCOPE_LEN, Ordering::Relaxed);
                }
            }
        }

        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for Zb1 {
    const CLAP_ID: &'static str = "com.zvle.zb1";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Mono house-bass synthesizer");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Mono];
}

impl Vst3Plugin for Zb1 {
    const VST3_CLASS_ID: [u8; 16] = *b"ZvleZB1HouseBass";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth, Vst3SubCategory::Mono];
}

nih_export_clap!(Zb1);
nih_export_vst3!(Zb1);
