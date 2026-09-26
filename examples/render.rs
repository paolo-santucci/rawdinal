// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::{
    X3f,
    experimental::{Reconstruction, render},
};
use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    time::Instant,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 {
        return Err("usage: render INPUT.x3f OUTPUT.pfm WB_PRESET bilinear|guided".into());
    }
    let white_balance = args[3].to_str().ok_or("invalid white balance")?;
    let reconstruction = match args[4].to_str() {
        Some("bilinear") => Reconstruction::Bilinear,
        Some("guided") => Reconstruction::Guided,
        _ => return Err("reconstruction must be bilinear or guided".into()),
    };
    eprintln!(
        "Experimental calibration model: column, spatial-color, response and AF-gain tables are not applied."
    );
    let started = Instant::now();
    let mut bytes = Vec::new();
    File::open(&args[1])?
        .take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let file = X3f::parse(&bytes)?;
    let calibration = file.calibration()?;
    let sensor = file.decode()?;
    let decoded = started.elapsed();
    let image = render(&sensor, &calibration, white_balance, reconstruction)?;
    eprintln!(
        "Decode {:.3}s, render {:.3}s, {}x{} linear sRGB",
        decoded.as_secs_f64(),
        started.elapsed().as_secs_f64() - decoded.as_secs_f64(),
        image.width,
        image.height
    );
    let mut output = BufWriter::new(
        File::options()
            .create_new(true)
            .write(true)
            .open(&args[2])?,
    );
    write!(output, "PF\n{} {}\n-1.0\n", image.width, image.height)?;
    for row in image.rgb.chunks_exact(image.width).rev() {
        for pixel in row {
            for channel in pixel {
                output.write_all(&channel.to_le_bytes())?;
            }
        }
    }
    output.flush()?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("render: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
