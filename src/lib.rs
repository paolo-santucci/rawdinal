// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! Native, bounded decoding of Sigma sd Quattro X3F sensor planes and CAMF data.
//! Sensor planes are not RGB; rendering requires camera calibration and reconstruction.

mod camf;
mod container;
mod entropy;
pub mod experimental;
mod photo;
mod reader;

pub use camf::{Calibration, Entry, Matrix};
pub use container::{Plane, SensorImage, X3f};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid(message: impl Into<String>) -> Error {
    Error(message.into())
}

fn zeroed<T: Default + Clone>(count: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| invalid("allocation failed"))?;
    values.resize(count, T::default());
    Ok(values)
}
