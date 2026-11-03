//! Offline render: plays a house bassline through every factory patch and writes WAVs.
use std::io::Write;
use zb1::dsp::Engine;
use zb1::presets;

fn write_wav(path: &str, data: &[f32], sr: u32) {
    let mut f = std::fs::File::create(path).unwrap();
    let n = data.len() as u32;
    let mut h = Vec::new();
    h.extend(b"RIFF"); h.extend((36 + n * 2).to_le_bytes()); h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes()); h.extend(1u16.to_le_bytes()); h.extend(1u16.to_le_bytes());
    h.extend(sr.to_le_bytes()); h.extend((sr * 2).to_le_bytes()); h.extend(2u16.to_le_bytes());
    h.extend(16u16.to_le_bytes()); h.extend(b"data"); h.extend((n * 2).to_le_bytes());
    f.write_all(&h).unwrap();
    for s in data {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).unwrap();
    }
}

fn main() {
    let sr = 44100.0;
    let bpm = 124.0;
    let step = (60.0 / bpm / 4.0 * sr) as usize; // 16th notes
    // (step, note, length in 16ths, velocity): offbeat-driven house line in F minor
    let pattern: &[(usize, u8, usize, f32)] = &[
        (2, 41, 1, 1.0), (3, 41, 1, 0.7), (6, 41, 1, 1.0), (7, 44, 1, 0.8),
        (10, 41, 1, 1.0), (11, 48, 1, 0.7), (14, 39, 1, 1.0), (15, 41, 1, 0.8),
    ];
    let bars = 2;
    let mut all = Vec::new();
    let mut summary = Vec::new();
    for pr in presets::all() {
        let mut e = Engine::new(sr);
        let total = step * 16 * bars + (sr as usize / 2);
        let mut out = Vec::with_capacity(total);
        for i in 0..total {
            let st = i / step;
            if i % step == 0 && st < 16 * bars {
                for &(s, note, len, _vel) in pattern {
                    if st % 16 == (s + len) % 16 { e.note_off(note); }
                }
                for &(s, note, _len, vel) in pattern {
                    if st % 16 == s { e.note_on(note, vel, &pr.s); }
                }
            }
            out.push(e.next_sample(&pr.s));
        }
        let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
        let rms = (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt();
        assert!(out.iter().all(|v| v.is_finite()), "{} produced NaN", pr.name);
        summary.push(format!("{:<15} peak {:.2}  rms {:.3}", pr.name, peak, rms));
        let fname = format!("render/{}.wav", pr.name.to_lowercase().replace(' ', "_"));
        write_wav(&fname, &out, sr as u32);
        all.extend(out);
        all.extend(std::iter::repeat(0.0).take(sr as usize / 4));
    }
    write_wav("render/all_patches_demo.wav", &all, sr as u32);
    for l in summary { println!("{l}"); }
}
