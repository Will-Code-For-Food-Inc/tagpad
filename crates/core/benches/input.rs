//! What this measures is variance, not speed.
//!
//! `Gamepad::frame` runs on every animation frame, so its *distribution* is the
//! interesting figure -- a mean with a long tail drops inputs, which a labeller
//! experiences as the tool ignoring them. Criterion reports the spread rather
//! than one number, which is the same standard the rest of this project holds
//! its measurements to.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tagpad_core::input::Gamepad;

fn frames(c: &mut Criterion) {
    let idle = [0.0f32; 16];
    let mut pressed = [0.0f32; 16];
    pressed[0] = 1.0;
    let axes = [0.0f32, -0.9, 0.0, 0.0];

    c.bench_function("frame/idle", |b| {
        let mut pad = Gamepad::new();
        b.iter(|| black_box(pad.frame(black_box(&idle), black_box(&[]))));
    });

    // The worst realistic case: a press edge plus the stick acting as a d-pad,
    // so both the button scan and the axis path run and the result allocates.
    c.bench_function("frame/press_with_stick", |b| {
        let mut pad = Gamepad::new();
        b.iter(|| {
            pad.reset();
            black_box(pad.frame(black_box(&pressed), black_box(&axes)))
        });
    });
}

criterion_group!(benches, frames);
criterion_main!(benches);
