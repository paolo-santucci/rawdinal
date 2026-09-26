// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::{Entry, X3f};
use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    path::Path,
    time::Instant,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if !(2..=3).contains(&args.len()) {
        return Err("usage: rawdinal-inspect INPUT.x3f [EXISTING_DUMP_DIRECTORY]".into());
    }
    let started = Instant::now();
    let mut bytes = Vec::new();
    File::open(&args[1])?
        .take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let file = X3f::parse(&bytes)?;
    let calibration = file.calibration()?;
    eprintln!(
        "Selected white balance: {}",
        file.white_balance(&calibration)?
    );
    eprintln!(
        "CAMF: {} entries, {} decoded bytes, {:.3}s",
        calibration.entries.len(),
        calibration.decoded_bytes().len(),
        started.elapsed().as_secs_f64()
    );
    for (name, entry) in calibration.entries.iter().filter(|_| args.len() == 2) {
        match entry {
            Entry::Text(text) => println!("{name}: {text:?}"),
            Entry::Properties(properties) => println!("{name}: {properties:?}"),
            Entry::Matrix(matrix) => println!(
                "{name}: {:?} {:?}{}",
                matrix.dimensions,
                &matrix.values[..matrix.values.len().min(12)],
                if matrix.values.len() > 12 { " …" } else { "" }
            ),
        }
    }
    let started = Instant::now();
    let image = file.decode()?;
    eprintln!("Sensor decode: {:.3}s", started.elapsed().as_secs_f64());
    for (index, plane) in image.layers.iter().enumerate() {
        let sum: u64 = plane.samples.iter().map(|&sample| u64::from(sample)).sum();
        eprintln!(
            "Layer {index}: {}x{}, sum={sum}, min={:?}, max={:?}",
            plane.width,
            plane.height,
            plane.samples.iter().min(),
            plane.samples.iter().max()
        );
        if let Some(directory) = args.get(2) {
            let path = Path::new(directory).join(format!("layer-{index}.u16le"));
            let mut output =
                BufWriter::new(File::options().write(true).create_new(true).open(path)?);
            for sample in &plane.samples {
                output.write_all(&sample.to_le_bytes())?;
            }
            output.flush()?;
        }
    }
    if let Some(directory) = args.get(2) {
        let path = Path::new(directory).join("calibration.bin");
        let mut output = File::options().write(true).create_new(true).open(path)?;
        output.write_all(calibration.decoded_bytes())?;
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rawdinal-inspect: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
