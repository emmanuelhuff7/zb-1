//! Pixel-RPG style editor. Everything is hand-painted: chunky bitmap text,
//! white-bordered "battle boxes", HP-bar style sliders and a typewriter
//! dialog box along the bottom.

use crate::font;
use crate::presets::{self, Preset};
use crate::{Scope, ZbParams, SCOPE_LEN};
use nih_plug::prelude::*;
use nih_plug_egui::create_egui_editor;
use nih_plug_egui::egui::{self, Color32, Id, Painter, Pos2, Rect, Sense, Vec2};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub const WIDTH: u32 = 900;
pub const HEIGHT: u32 = 560;

const ACCENT: Color32 = Color32::from_rgb(0x4d, 0x47, 0x75); // #4d4775
const ACCENT_DARK: Color32 = Color32::from_rgb(0x24, 0x21, 0x38);
const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x9a, 0x92, 0xd6);
const BG: Color32 = Color32::from_rgb(0x08, 0x07, 0x0d);
const WHITE: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const DIM: Color32 = Color32::from_rgb(0xb4, 0xae, 0xd8);
const GOLD: Color32 = Color32::from_rgb(0xff, 0xdc, 0x4a);

const PX: f32 = 2.0; // standard text pixel size
const ROW_H: f32 = 25.0;
const LABEL_W: f32 = 84.0;

struct UiState {
    presets: Vec<Preset>,
    dialog_text: String,
    dialog_start: f64,
    drag_value: f32,
    last_preset_seen: usize,
}

pub fn create(params: Arc<ZbParams>, scope: Arc<Scope>) -> Option<Box<dyn Editor>> {
    let current_preset = params.current_preset.clone();
    let presets = presets::all();
    let first = presets[current_preset.load(Ordering::Relaxed).min(presets.len() - 1)].flavor.to_string();
    let state = UiState {
        presets,
        dialog_text: first,
        dialog_start: 0.0,
        drag_value: 0.0,
        last_preset_seen: usize::MAX,
    };

    create_egui_editor(
        params.editor_state.clone(),
        state,
        |ctx, _| {
            let mut style = (*ctx.style()).clone();
            style.visuals.panel_fill = BG;
            style.visuals.window_fill = BG;
            ctx.set_style(style);
        },
        move |ctx, setter, st| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(BG))
                .show(ctx, |ui| {
                    draw(ui, ctx, setter, st, &params, &scope, &current_preset);
                });
            ctx.request_repaint();
        },
    )
}

// ------------------------------------------------------------------ helpers

fn pixel_box(p: &Painter, r: Rect, title: &str) {
    // Drop shadow in the accent colour, then a thick white frame.
    p.rect_filled(r.translate(Vec2::new(4.0, 4.0)), 0.0, ACCENT);
    p.rect_filled(r, 0.0, WHITE);
    p.rect_filled(r.shrink(3.0), 0.0, BG);
    if !title.is_empty() {
        let tw = font::text_width(title, PX);
        let tag = Rect::from_min_size(r.min + Vec2::new(12.0, -12.0), Vec2::new(tw + 20.0, 24.0));
        p.rect_filled(tag, 0.0, WHITE);
        p.rect_filled(tag.shrink(3.0), 0.0, ACCENT);
        font::draw(p, tag.min + Vec2::new(10.0, 5.0), title, PX, WHITE);
    }
}

fn shadow_text(p: &Painter, pos: Pos2, text: &str, px: f32, color: Color32) {
    font::draw(p, pos + Vec2::new(px, px), text, px, Color32::BLACK);
    font::draw(p, pos, text, px, color);
}

fn set_hover(hover: &mut Option<String>, param: &impl Param) {
    let v = param.normalized_value_to_string(param.unmodulated_normalized_value(), true);
    *hover = Some(format!("* {}: {}", param.name().to_uppercase(), v.to_uppercase()));
}

/// HP-bar style horizontal slider for a continuous parameter.
#[allow(clippy::too_many_arguments)]
fn bar_row<P: Param>(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    st: &mut UiState,
    hover: &mut Option<String>,
    origin: Pos2,
    width: f32,
    label: &str,
    param: &P,
) {
    let p = ui.painter().clone();
    font::draw(&p, origin + Vec2::new(0.0, 4.0), label, PX, DIM);

    let bar = Rect::from_min_size(origin + Vec2::new(LABEL_W, 0.0), Vec2::new(width - LABEL_W, 22.0));
    let id = Id::new(("bar", param.name()));
    let resp = ui.interact(bar.expand(2.0), id, Sense::click_and_drag());

    let norm = param.unmodulated_normalized_value();
    if resp.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, param.default_normalized_value());
        setter.end_set_parameter(param);
    } else if resp.drag_started() {
        setter.begin_set_parameter(param);
        st.drag_value = norm;
        if let Some(pos) = resp.interact_pointer_pos() {
            if !ui.input(|i| i.modifiers.shift) {
                st.drag_value = ((pos.x - bar.min.x - 3.0) / (bar.width() - 6.0)).clamp(0.0, 1.0);
                setter.set_parameter_normalized(param, st.drag_value);
            }
        }
    } else if resp.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.15 } else { 1.0 };
        st.drag_value = (st.drag_value + resp.drag_delta().x / (bar.width() - 6.0) * fine).clamp(0.0, 1.0);
        setter.set_parameter_normalized(param, st.drag_value);
    }
    if resp.drag_stopped() {
        setter.end_set_parameter(param);
    }

    let hot = resp.hovered() || resp.dragged();
    if hot {
        set_hover(hover, param);
    }

    // Frame, track and segmented fill.
    p.rect_filled(bar, 0.0, if hot { GOLD } else { WHITE });
    let inner = bar.shrink(3.0);
    p.rect_filled(inner, 0.0, ACCENT_DARK);
    let fill_w = (inner.width() * norm).round();
    if fill_w > 0.0 {
        let fill = Rect::from_min_size(inner.min, Vec2::new(fill_w, inner.height()));
        p.rect_filled(fill, 0.0, ACCENT);
        // Lighter top highlight row for a bit of 8-bit depth.
        p.rect_filled(Rect::from_min_size(fill.min, Vec2::new(fill_w, 3.0)), 0.0, ACCENT_LIGHT);
        // Segment notches every 8 points.
        let mut x = inner.min.x + 8.0;
        while x < fill.max.x - 1.0 {
            p.rect_filled(Rect::from_min_size(Pos2::new(x, inner.min.y + 3.0), Vec2::new(1.0, inner.height() - 3.0)), 0.0, ACCENT_DARK);
            x += 8.0;
        }
    }
    let text = param.normalized_value_to_string(norm, true).to_uppercase();
    let tw = font::text_width(&text, PX);
    shadow_text(&p, Pos2::new(inner.max.x - tw - 5.0, inner.min.y + 1.0), &text, PX, WHITE);
}

/// "< VALUE >" selector for stepped parameters (enums, ints, bools).
#[allow(clippy::too_many_arguments)]
fn select_row<P: Param>(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    hover: &mut Option<String>,
    origin: Pos2,
    width: f32,
    label: &str,
    param: &P,
) {
    let p = ui.painter().clone();
    font::draw(&p, origin + Vec2::new(0.0, 5.0), label, PX, DIM);
    let r = Rect::from_min_size(origin + Vec2::new(LABEL_W, 1.0), Vec2::new(width - LABEL_W, 20.0));
    let resp = ui.interact(r, Id::new(("sel", param.name())), Sense::click());

    let steps = param.step_count().unwrap_or(1).max(1);
    let norm = param.unmodulated_normalized_value();
    let idx = (norm * steps as f32).round() as i32;

    if resp.clicked() {
        let left = resp.interact_pointer_pos().map(|pos| pos.x < r.center().x).unwrap_or(false);
        let next = if left { idx - 1 } else { idx + 1 };
        let next = next.rem_euclid(steps as i32 + 1);
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, next as f32 / steps as f32);
        setter.end_set_parameter(param);
    }
    if resp.secondary_clicked() || resp.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, param.default_normalized_value());
        setter.end_set_parameter(param);
    }

    let hot = resp.hovered();
    if hot {
        set_hover(hover, param);
    }
    let ptr_left = ui.input(|i| i.pointer.hover_pos()).map(|pos| pos.x < r.center().x).unwrap_or(false);

    let text = param.normalized_value_to_string(norm, true).to_uppercase();
    let tw = font::text_width(&text, PX);
    shadow_text(&p, Pos2::new(r.center().x - tw / 2.0, r.min.y + 3.5), &text, PX, if hot { GOLD } else { WHITE });
    let arrow_l = if hot && ptr_left { GOLD } else { ACCENT_LIGHT };
    let arrow_r = if hot && !ptr_left { GOLD } else { ACCENT_LIGHT };
    font::draw(&p, Pos2::new(r.min.x + 2.0, r.min.y + 3.5), "<", PX, arrow_l);
    font::draw(&p, Pos2::new(r.max.x - 10.0, r.min.y + 3.5), ">", PX, arrow_r);
    // Dotted underline.
    let mut x = r.min.x + 16.0;
    while x < r.max.x - 16.0 {
        p.rect_filled(Rect::from_min_size(Pos2::new(x, r.max.y - 2.0), Vec2::new(2.0, 2.0)), 0.0, ACCENT);
        x += 6.0;
    }
}

fn apply_preset(setter: &ParamSetter, params: &ZbParams, pr: &Preset) {
    fn set<P: Param>(setter: &ParamSetter, p: &P, v: P::Plain) {
        setter.begin_set_parameter(p);
        setter.set_parameter(p, v);
        setter.end_set_parameter(p);
    }
    let s = &pr.s;
    set(setter, &params.osc1_wave, s.osc1_wave.into());
    set(setter, &params.osc2_wave, s.osc2_wave.into());
    set(setter, &params.osc2_octave, s.osc2_octave);
    set(setter, &params.osc2_detune, s.osc2_detune_cents);
    set(setter, &params.osc_mix, s.osc_mix);
    set(setter, &params.sub_level, s.sub_level);
    set(
        setter,
        &params.sub_wave,
        match s.sub_wave {
            crate::dsp::SubWave::Sine => crate::SubWaveParam::Sine,
            crate::dsp::SubWave::Square => crate::SubWaveParam::Square,
        },
    );
    set(setter, &params.cutoff, s.cutoff_hz);
    set(setter, &params.resonance, s.resonance);
    set(setter, &params.env_amount, s.env_amount);
    set(setter, &params.key_track, s.key_track);
    set(setter, &params.f_attack, s.f_attack);
    set(setter, &params.f_decay, s.f_decay);
    set(setter, &params.f_sustain, s.f_sustain);
    set(setter, &params.f_release, s.f_release);
    set(setter, &params.a_attack, s.a_attack);
    set(setter, &params.a_decay, s.a_decay);
    set(setter, &params.a_sustain, s.a_sustain);
    set(setter, &params.a_release, s.a_release);
    set(setter, &params.velocity_amount, s.velocity_amount);
    set(setter, &params.glide, s.glide_ms);
    set(setter, &params.legato, s.legato);
    set(setter, &params.drive, s.drive);
    set(setter, &params.volume, s.volume);
}

// --------------------------------------------------------------------- draw

fn draw(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    setter: &ParamSetter,
    st: &mut UiState,
    params: &ZbParams,
    scope: &Scope,
    current_preset: &AtomicUsize,
) {
    let full = Rect::from_min_size(Pos2::ZERO, Vec2::new(WIDTH as f32, HEIGHT as f32));
    let p = ui.painter().clone();
    let now = ctx.input(|i| i.time);
    let mut hover: Option<String> = None;

    // Background: dark field with a sparse accent dither, like a night sky in 8-bit.
    p.rect_filled(full, 0.0, BG);
    let mut seed: u32 = 0x9e37_79b9;
    for _ in 0..140 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let x = (seed % WIDTH) as f32;
        let y = 60.0 + ((seed / WIDTH) % (HEIGHT - 60)) as f32;
        let c = if seed % 7 == 0 { ACCENT_LIGHT } else { ACCENT };
        p.rect_filled(Rect::from_min_size(Pos2::new(x.floor(), y.floor()), Vec2::splat(2.0)), 0.0, c);
    }

    // Title bar.
    let title = Rect::from_min_size(Pos2::ZERO, Vec2::new(WIDTH as f32, 54.0));
    p.rect_filled(title, 0.0, ACCENT);
    p.rect_filled(Rect::from_min_size(Pos2::new(0.0, 51.0), Vec2::new(WIDTH as f32, 3.0)), 0.0, WHITE);
    shadow_text(&p, Pos2::new(18.0, 11.0), "ZB-1", 4.0, WHITE);
    shadow_text(&p, Pos2::new(122.0, 14.0), "HOUSE BASS MACHINE", PX, WHITE);
    font::draw(&p, Pos2::new(122.0, 31.0), "MONO SYNTHESIZER  /  24DB LADDER", PX, ACCENT_LIGHT);
    let tag = "ZVLE";
    let tw = font::text_width(tag, 3.0);
    shadow_text(&p, Pos2::new(WIDTH as f32 - tw - 20.0, 16.0), tag, 3.0, WHITE);

    // --- three control boxes
    let margin = 16.0;
    let gap = 14.0;
    let col_w = (WIDTH as f32 - margin * 2.0 - gap * 2.0) / 3.0;
    let top = 76.0;
    let box_h = 9.0 * ROW_H + 30.0;
    let inner_w = col_w - 28.0;

    let col = |i: f32| margin + i * (col_w + gap);
    let osc_box = Rect::from_min_size(Pos2::new(col(0.0), top), Vec2::new(col_w, box_h));
    let flt_box = Rect::from_min_size(Pos2::new(col(1.0), top), Vec2::new(col_w, box_h));
    let amp_box = Rect::from_min_size(Pos2::new(col(2.0), top), Vec2::new(col_w, box_h));
    pixel_box(&p, osc_box, "OSC");
    pixel_box(&p, flt_box, "FILTER");
    pixel_box(&p, amp_box, "AMP + OUT");

    let row = |b: Rect, i: f32| Pos2::new(b.min.x + 14.0, b.min.y + 20.0 + i * ROW_H);

    select_row(ui, setter, &mut hover, row(osc_box, 0.0), inner_w, "WAVE 1", &params.osc1_wave);
    select_row(ui, setter, &mut hover, row(osc_box, 1.0), inner_w, "WAVE 2", &params.osc2_wave);
    select_row(ui, setter, &mut hover, row(osc_box, 2.0), inner_w, "OCT 2", &params.osc2_octave);
    bar_row(ui, setter, st, &mut hover, row(osc_box, 3.0), inner_w, "DETUNE", &params.osc2_detune);
    bar_row(ui, setter, st, &mut hover, row(osc_box, 4.0), inner_w, "MIX 2", &params.osc_mix);
    bar_row(ui, setter, st, &mut hover, row(osc_box, 5.0), inner_w, "SUB", &params.sub_level);
    select_row(ui, setter, &mut hover, row(osc_box, 6.0), inner_w, "SUB WV", &params.sub_wave);
    bar_row(ui, setter, st, &mut hover, row(osc_box, 7.0), inner_w, "VELO", &params.velocity_amount);
    bar_row(ui, setter, st, &mut hover, row(osc_box, 8.0), inner_w, "GLIDE", &params.glide);

    bar_row(ui, setter, st, &mut hover, row(flt_box, 0.0), inner_w, "CUTOFF", &params.cutoff);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 1.0), inner_w, "RESO", &params.resonance);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 2.0), inner_w, "ENV", &params.env_amount);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 3.0), inner_w, "KEYTRK", &params.key_track);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 4.0), inner_w, "ATTACK", &params.f_attack);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 5.0), inner_w, "DECAY", &params.f_decay);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 6.0), inner_w, "SUSTN", &params.f_sustain);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 7.0), inner_w, "RELEAS", &params.f_release);
    bar_row(ui, setter, st, &mut hover, row(flt_box, 8.0), inner_w, "DRIVE", &params.drive);

    bar_row(ui, setter, st, &mut hover, row(amp_box, 0.0), inner_w, "ATTACK", &params.a_attack);
    bar_row(ui, setter, st, &mut hover, row(amp_box, 1.0), inner_w, "DECAY", &params.a_decay);
    bar_row(ui, setter, st, &mut hover, row(amp_box, 2.0), inner_w, "SUSTN", &params.a_sustain);
    bar_row(ui, setter, st, &mut hover, row(amp_box, 3.0), inner_w, "RELEAS", &params.a_release);
    select_row(ui, setter, &mut hover, row(amp_box, 4.0), inner_w, "LEGATO", &params.legato);
    bar_row(ui, setter, st, &mut hover, row(amp_box, 5.0), inner_w, "VOLUME", &params.volume);

    // Amp envelope shape preview in the spare space of the AMP box.
    {
        let r = Rect::from_min_max(
            Pos2::new(amp_box.min.x + 14.0, row(amp_box, 6.0).y + 6.0),
            Pos2::new(amp_box.max.x - 14.0, amp_box.max.y - 12.0),
        );
        p.rect_filled(r, 0.0, ACCENT_DARK);
        let a = params.a_attack.unmodulated_normalized_value();
        let d = params.a_decay.unmodulated_normalized_value();
        let s = params.a_sustain.value();
        let rl = params.a_release.unmodulated_normalized_value();
        let total = a + d + 0.6 + rl + 1e-3;
        let w = r.width() - 8.0;
        let h = r.height() - 8.0;
        let x0 = r.min.x + 4.0;
        let y_of = |lvl: f32| r.max.y - 4.0 - lvl * h;
        let pts = [
            (0.0, 0.0),
            (a / total, 1.0),
            ((a + d) / total, s),
            ((a + d + 0.6) / total, s),
            (1.0, 0.0),
        ];
        let step = 3.0;
        let mut x = 0.0;
        while x <= w {
            let t = x / w;
            let mut lvl = 0.0;
            for k in 0..4 {
                let (t0, l0) = pts[k];
                let (t1, l1) = pts[k + 1];
                if t >= t0 && t <= t1 {
                    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                    lvl = l0 + (l1 - l0) * f;
                    break;
                }
            }
            p.rect_filled(Rect::from_min_size(Pos2::new((x0 + x).floor(), y_of(lvl).floor()), Vec2::splat(3.0)), 0.0, ACCENT_LIGHT);
            x += step;
        }
        let lw = font::text_width("AMP ENV", PX);
        font::draw(&p, Pos2::new(r.max.x - lw - 6.0, r.min.y + 5.0), "AMP ENV", PX, ACCENT);
    }

    // --- patch list
    let row2 = top + box_h + 22.0;
    let row2_h = 128.0;
    let patch_box = Rect::from_min_size(Pos2::new(margin, row2), Vec2::new(col_w * 2.0 + gap - 150.0, row2_h));
    pixel_box(&p, patch_box, "PATCH");

    let sel = current_preset.load(Ordering::Relaxed).min(st.presets.len() - 1);
    let pcol_w = (patch_box.width() - 28.0) / 2.0;
    let mut clicked: Option<usize> = None;
    for (i, pr) in st.presets.iter().enumerate() {
        let cx = patch_box.min.x + 14.0 + (i / 4) as f32 * pcol_w;
        let cy = patch_box.min.y + 18.0 + (i % 4) as f32 * 25.0;
        let r = Rect::from_min_size(Pos2::new(cx, cy), Vec2::new(pcol_w - 8.0, 22.0));
        let resp = ui.interact(r, Id::new(("patch", i)), Sense::click());
        if resp.clicked() {
            clicked = Some(i);
        }
        let hot = resp.hovered();
        if hot {
            hover = Some(format!("* LOAD PATCH: {}", pr.name));
        }
        let color = if i == sel { GOLD } else if hot { WHITE } else { DIM };
        if i == sel {
            // Pixel diamond cursor.
            let c = Pos2::new(cx + 6.0, cy + 10.0);
            for (dx, dy, w, h) in [(-2.0, -6.0, 4.0, 2.0), (-4.0, -4.0, 8.0, 2.0), (-6.0, -2.0, 12.0, 4.0), (-4.0, 2.0, 8.0, 2.0), (-2.0, 4.0, 4.0, 2.0)] {
                p.rect_filled(Rect::from_min_size(Pos2::new(c.x + dx, c.y + dy), Vec2::new(w, h)), 0.0, GOLD);
            }
        }
        font::draw(&p, Pos2::new(cx + 18.0, cy + 4.0), pr.name, PX, color);
    }
    if let Some(i) = clicked {
        apply_preset(setter, params, &st.presets[i]);
        current_preset.store(i, Ordering::Relaxed);
    }
    let sel = current_preset.load(Ordering::Relaxed).min(st.presets.len() - 1);
    if sel != st.last_preset_seen {
        st.last_preset_seen = sel;
        st.dialog_text = st.presets[sel].flavor.to_string();
        st.dialog_start = now;
    }

    // --- scope
    let scope_box = Rect::from_min_max(Pos2::new(patch_box.max.x + gap, row2), Pos2::new(WIDTH as f32 - margin, row2 + row2_h));
    pixel_box(&p, scope_box, "SCOPE");
    {
        let r = scope_box.shrink(14.0).translate(Vec2::new(0.0, 4.0));
        let r = Rect::from_min_max(r.min, Pos2::new(r.max.x, r.max.y - 2.0));
        // Grid.
        let mut gx = r.min.x;
        while gx <= r.max.x {
            let mut gy = r.min.y;
            while gy <= r.max.y {
                p.rect_filled(Rect::from_min_size(Pos2::new(gx, gy), Vec2::splat(1.0)), 0.0, ACCENT);
                gy += 12.0;
            }
            gx += 12.0;
        }
        p.rect_filled(Rect::from_min_size(Pos2::new(r.min.x, r.center().y), Vec2::new(r.width(), 1.0)), 0.0, ACCENT);

        let w = scope.write.load(Ordering::Relaxed);
        let read = |k: usize| scope.samples[(w + k) % SCOPE_LEN].load(Ordering::Relaxed);
        // Rising zero-crossing trigger in the first half for a stable picture.
        let mut start = 0;
        for k in 1..SCOPE_LEN / 2 {
            if read(k - 1) <= 0.0 && read(k) > 0.0 {
                start = k;
                break;
            }
        }
        let n = SCOPE_LEN / 2;
        let peak = (0..SCOPE_LEN).map(read).fold(0.05f32, |m, v| m.max(v.abs()));
        let gain = (0.9 / peak).min(4.0);
        let cell = 3.0;
        let cols = (r.width() / cell) as usize;
        for c in 0..cols {
            let k = start + c * n / cols.max(1);
            let v = (read(k.min(SCOPE_LEN - 1)) * gain).clamp(-1.0, 1.0);
            let y = r.center().y - v * (r.height() / 2.0 - 2.0);
            let y = (y / cell).floor() * cell;
            p.rect_filled(Rect::from_min_size(Pos2::new(r.min.x + c as f32 * cell, y), Vec2::splat(cell)), 0.0, ACCENT_LIGHT);
        }
    }

    // --- dialog box with typewriter text
    let dlg = Rect::from_min_max(Pos2::new(margin, row2 + row2_h + 20.0), Pos2::new(WIDTH as f32 - margin, HEIGHT as f32 - 14.0));
    pixel_box(&p, dlg, "");
    let (text, reveal) = match &hover {
        Some(h) => (h.clone(), usize::MAX),
        None => {
            let n = ((now - st.dialog_start).max(0.0) * 45.0) as usize;
            (st.dialog_text.clone(), n)
        }
    };
    let shown: String = text.chars().take(reveal).collect();
    font::draw(&p, dlg.min + Vec2::new(18.0, (dlg.height() - 7.0 * PX) / 2.0), &shown, PX, WHITE);
    // Blinking caret once the line is fully typed.
    if hover.is_none() && reveal >= text.chars().count() && ((now * 2.0) as i64) % 2 == 0 {
        let x = dlg.min.x + 18.0 + font::text_width(&shown, PX) + 8.0;
        p.rect_filled(Rect::from_min_size(Pos2::new(x, dlg.center().y - 6.0), Vec2::new(8.0, 12.0)), 0.0, ACCENT_LIGHT);
    }
}
