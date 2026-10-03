// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! Extracts complete decompressed CAMF metadata without decoding image planes.

use rawdinal::X3f;
use std::{
    fs::File,
    io::{Read, Write},
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: dump_camf INPUT.x3f OUTPUT.camf (must not exist)".into());
    }
    let mut bytes = Vec::new();
    File::open(&args[1])?
        .take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let file = X3f::parse(&bytes)?;
    let calibration = file.calibration()?;
    let mut output = File::options()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    output.write_all(calibration.decoded_bytes())?;
    output.flush()?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dump_camf: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
