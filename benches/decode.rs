// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::X3f;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var_os("X3F_SAMPLE").ok_or("set X3F_SAMPLE to a local X3F file")?;
    let bytes = std::fs::read(path)?;
    let file = X3f::parse(&bytes)?;
    black_box(file.decode()?);
    let mut sensor_times = Vec::new();
    let mut calibration_times = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        black_box(file.decode()?);
        sensor_times.push(start.elapsed());
        let start = Instant::now();
        black_box(file.calibration()?);
        calibration_times.push(start.elapsed());
    }
    for (name, times) in [
        ("sensor", &mut sensor_times),
        ("CAMF", &mut calibration_times),
    ] {
        times.sort();
        println!(
            "{name}: median {:.3} ms, min {:.3} ms, max {:.3} ms (7 iterations, warm input)",
            milliseconds(times[3]),
            milliseconds(times[0]),
            milliseconds(times[6])
        );
    }
    Ok(())
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
