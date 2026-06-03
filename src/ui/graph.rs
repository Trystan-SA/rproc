//! Normalizes the rolling history into exactly `SLOTS` (=60) values in `0..1`,
//! oldest-first, right-anchored (left-padded with the oldest sample when the
//! buffer isn't full yet). The `.slint` `Poly` draws these as a fixed-length
//! polyline — Slint's software renderer supports a `Path` with a static set of
//! child line elements, but not a dynamic `commands` string nor `for` inside a
//! `Path`, so the point count must be constant.

use std::collections::VecDeque;

use slint::{ModelRc, VecModel};

pub const SLOTS: usize = 60;

pub fn norm_f32(data: &VecDeque<f32>, max: f32) -> ModelRc<f32> {
    let m = max.max(1e-9);
    pad(data.iter().map(|x| (x / m).clamp(0.0, 1.0)).collect())
}

pub fn norm_f64(data: &VecDeque<f64>, max: f64) -> ModelRc<f32> {
    let m = max.max(1e-9);
    pad(data
        .iter()
        .map(|x| (x / m).clamp(0.0, 1.0) as f32)
        .collect())
}

fn pad(mut v: Vec<f32>) -> ModelRc<f32> {
    if v.len() > SLOTS {
        v = v.split_off(v.len() - SLOTS);
    } else if v.len() < SLOTS {
        let lead = *v.first().unwrap_or(&0.0);
        let mut out = vec![lead; SLOTS - v.len()];
        out.extend(v);
        v = out;
    }
    ModelRc::new(VecModel::from(v))
}
