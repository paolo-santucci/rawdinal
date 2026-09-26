// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! Research renderer for evaluating Quattro reconstruction against external references.
//!
//! This is not a production calibration model. It implements measured black subtraction,
//! known bad-pixel/AF locations and the X3F Tools matrix/gain convention, but does not
//! interpret sd Quattro column, spatial-color, response or AF-gain tables. White balance
//! is explicit because the samples' CAMF setting disagrees with their JPEG EXIF setting.
//! Output is unbounded linear sRGB with no tone rendering or orientation transform.

use crate::{Calibration, Plane, Result, SensorImage, invalid, zeroed};

pub struct LinearImage {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<[f32; 3]>,
}

#[derive(Clone, Copy)]
pub enum Reconstruction {
    Bilinear,
    Guided,
}

struct Layer {
    width: usize,
    height: usize,
    pixels: Vec<f32>,
    noise_variance: f32,
}

#[derive(Clone, Copy)]
struct Crop {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

/// Produces a diagnostic rendering using a restricted, explicitly incomplete calibration model.
pub fn render(
    sensor: &SensorImage,
    calibration: &Calibration,
    white_balance: &str,
    reconstruction: Reconstruction,
) -> Result<LinearImage> {
    validate_geometry(sensor)?;
    if calibration.values::<1>("CAMERAID")? != [40.0] {
        return Err(invalid("experimental renderer is limited to sd Quattro"));
    }
    let crop = Crop::parse(calibration.values("ActiveImageArea")?, &sensor.layers[2])?;
    let bottom = calibrate(&sensor.layers[0], calibration, 0)?;
    let middle = calibrate(&sensor.layers[1], calibration, 1)?;
    let top = calibrate(&sensor.layers[2], calibration, 2)?;
    let coarse_top = downsample(&top)?;
    let coefficients = match reconstruction {
        Reconstruction::Guided => Some([
            local_regression(&coarse_top, &bottom)?,
            local_regression(&coarse_top, &middle)?,
        ]),
        Reconstruction::Bilinear => None,
    };
    let matrix = conversion_matrix(calibration, white_balance)?;
    let width = crop.right - crop.left + 1;
    let height = crop.bottom - crop.top + 1;
    let mut rgb: Vec<[f32; 3]> = zeroed(width * height)?;
    for (y, row) in rgb.chunks_exact_mut(width).enumerate() {
        for (x, pixel) in row.iter_mut().enumerate() {
            let sx = crop.left + x;
            let sy = crop.top + y;
            let guide = top.pixels[sy * top.width + sx];
            let low_x = (sx as f32 - 0.5) * 0.5;
            let low_y = (sy as f32 - 0.5) * 0.5;
            let mut native = [
                bottom.sample(low_x, low_y),
                middle.sample(low_x, low_y),
                guide,
            ];
            if let Some(coefficients) = &coefficients {
                for channel in 0..2 {
                    let [slope, intercept] = sample_coefficients(
                        &coefficients[channel],
                        bottom.width,
                        bottom.height,
                        low_x,
                        low_y,
                    );
                    native[channel] = slope * guide + intercept;
                }
            }
            for (channel, output) in pixel.iter_mut().enumerate() {
                *output = (0..3)
                    .map(|input| matrix[3 * channel + input] * native[input])
                    .sum();
                if !output.is_finite() {
                    return Err(invalid("non-finite reconstructed pixel"));
                }
            }
        }
    }
    Ok(LinearImage { width, height, rgb })
}

fn validate_geometry(sensor: &SensorImage) -> Result<()> {
    for layer in &sensor.layers {
        if layer.width < 2
            || layer.height < 2
            || layer.width.checked_mul(layer.height) != Some(layer.samples.len())
        {
            return Err(invalid("invalid sensor layer"));
        }
    }
    let [bottom, middle, top] = &sensor.layers;
    if (bottom.width, bottom.height) != (2944, 1888)
        || (middle.width, middle.height) != (2944, 1888)
        || (top.width, top.height) != (5888, 3776)
    {
        return Err(invalid(
            "experimental renderer requires full-resolution sd Quattro",
        ));
    }
    Ok(())
}

impl Crop {
    fn parse(rectangle: [f64; 4], layer: &Plane) -> Result<Self> {
        if rectangle
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0 || value.fract() != 0.0)
        {
            return Err(invalid("invalid active-area coordinates"));
        }
        let [left, top, right, bottom] = rectangle.map(|value| value as usize);
        if left > right || top > bottom || right >= layer.width || bottom >= layer.height {
            return Err(invalid("active area outside sensor"));
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }
}

fn calibrate(plane: &Plane, calibration: &Calibration, channel: usize) -> Result<Layer> {
    let scale = if channel < 2 { 2 } else { 1 };
    let ranges = calibration.values::<4>("DarkShieldColRange")?;
    let mut dark_columns = [0; 4];
    for (index, value) in ranges.iter().enumerate() {
        if *value < 0.0 || value.fract() != 0.0 || *value / scale as f64 >= plane.width as f64 {
            return Err(invalid("invalid dark-shield columns"));
        }
        dark_columns[index] = *value as usize / scale;
    }
    if dark_columns[0] > dark_columns[1]
        || dark_columns[2] > dark_columns[3]
        || dark_columns[1] >= dark_columns[2]
    {
        return Err(invalid("invalid dark-shield ranges"));
    }
    let (black, variance) = measure_black(plane, dark_columns)?;
    let [depth] = calibration.values::<1>("ImageDepth")?;
    if depth.fract() != 0.0 || !(1.0..=16.0).contains(&depth) {
        return Err(invalid("invalid sensor bit depth"));
    }
    let white = ((1u32 << depth as u32) - 1) as f32;
    if black >= white {
        return Err(invalid("black level exceeds white level"));
    }
    let mut pixels = zeroed(plane.samples.len())?;
    for (output, &sample) in pixels.iter_mut().zip(&plane.samples) {
        *output = (f32::from(sample) - black) / (white - black);
    }
    let mut layer = Layer {
        width: plane.width,
        height: plane.height,
        pixels,
        noise_variance: variance / (white - black).powi(2),
    };
    repair_pixels(&mut layer, calibration, channel)?;
    Ok(layer)
}

fn measure_black(plane: &Plane, columns: [usize; 4]) -> Result<(f32, f32)> {
    let mut histogram: Vec<u32> = zeroed(65536)?;
    let mut count = 0;
    for row in plane.samples.chunks_exact(plane.width) {
        for range in columns.chunks_exact(2) {
            for &value in &row[range[0]..=range[1]] {
                histogram[value as usize] += 1;
                count += 1;
            }
        }
    }
    let mut total = 0;
    let black = histogram
        .iter()
        .position(|&frequency| {
            total += frequency;
            total > count / 2
        })
        .ok_or_else(|| invalid("empty black-level sample"))?;
    let mut weight = 0u64;
    let mut squared = 0f64;
    for (value, &frequency) in histogram.iter().enumerate() {
        let difference = value as f64 - black as f64;
        if difference.abs() <= 64.0 {
            weight += u64::from(frequency);
            squared += difference * difference * f64::from(frequency);
        }
    }
    Ok((black as f32, (squared / weight as f64) as f32))
}

fn repair_pixels(layer: &mut Layer, calibration: &Calibration, channel: usize) -> Result<()> {
    let name = if channel == 2 {
        "BadPixelsLumaF23"
    } else {
        "BadPixelsChromaF23"
    };
    let mut marked: Vec<bool> = zeroed(layer.pixels.len())?;
    let table = &calibration.matrix(name)?.values;
    let mut position = 0;
    while position < table.len() {
        let row = table[position] as usize;
        if table[position].fract() != 0.0 || table[position] < 0.0 || row >= layer.height {
            return Err(invalid("invalid bad-pixel row"));
        }
        position += 1;
        loop {
            let &column = table
                .get(position)
                .ok_or_else(|| invalid("truncated bad-pixel list"))?;
            position += 1;
            if column == 0.0 {
                break;
            }
            if column.fract() != 0.0
                || column < 0.0
                || column as usize >= layer.width
                || position >= table.len()
            {
                return Err(invalid("invalid bad-pixel column"));
            }
            marked[row * layer.width + column as usize] = true;
            position += 1;
        }
    }
    let (x0, x1, dx, y0, y1, dy, rows) = if channel == 2 {
        (217, 5641, 16, 464, 3312, 32, 2)
    } else {
        (108, 2820, 8, 232, 1656, 16, 1)
    };
    for y in (y0..=y1).step_by(dy) {
        for x in (x0..=x1).step_by(dx) {
            for row in y..y + rows {
                marked[row * layer.width + x] = true;
            }
        }
    }
    let mut repaired = Vec::new();
    for (index, &bad) in marked.iter().enumerate() {
        if !bad {
            continue;
        }
        let (x, y) = (index % layer.width, index / layer.width);
        let mut sum = 0.0;
        let mut weight = 0.0;
        for radius in 1..=8 {
            for ny in y.saturating_sub(radius)..=(y + radius).min(layer.height - 1) {
                for nx in x.saturating_sub(radius)..=(x + radius).min(layer.width - 1) {
                    if nx.abs_diff(x).max(ny.abs_diff(y)) != radius || marked[ny * layer.width + nx]
                    {
                        continue;
                    }
                    let w = 1.0 / ((nx as f32 - x as f32).powi(2) + (ny as f32 - y as f32).powi(2));
                    sum += w * layer.pixels[ny * layer.width + nx];
                    weight += w;
                }
            }
            if weight > 0.0 {
                break;
            }
        }
        if weight == 0.0 {
            return Err(invalid("unresolved bad-pixel cluster"));
        }
        repaired.push((index, sum / weight));
    }
    for (index, value) in repaired {
        layer.pixels[index] = value;
    }
    Ok(())
}

fn conversion_matrix(calibration: &Calibration, white_balance: &str) -> Result<[f32; 9]> {
    let mut gains =
        calibration.values::<3>(calibration.property("WhiteBalanceGains", white_balance)?)?;
    let temperature = calibration.values::<3>("TempGainFact")?;
    for channel in 0..3 {
        gains[channel] *= temperature[channel];
    }
    let mut matrix = calibration
        .values::<9>(calibration.property("WhiteBalanceColorCorrections", white_balance)?)?;
    let [sensor_iso] = calibration.values::<1>("SensorISO")?;
    let [capture_iso] = calibration.values::<1>("CaptureISO")?;
    if sensor_iso <= 0.0
        || capture_iso <= 0.0
        || gains.iter().any(|gain| !gain.is_finite() || *gain <= 0.0)
    {
        return Err(invalid("invalid calibration gains"));
    }
    for (index, value) in matrix.iter_mut().enumerate() {
        *value *= gains[index % 3] * capture_iso / sensor_iso;
    }
    if matrix
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1000.0)
    {
        return Err(invalid("invalid color matrix"));
    }
    Ok(matrix.map(|value| value as f32))
}

fn downsample(top: &Layer) -> Result<Layer> {
    let width = top.width / 2;
    let height = top.height / 2;
    let mut pixels = zeroed(width * height)?;
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let source = (index / width) * 2 * top.width + (index % width) * 2;
        *pixel = (top.pixels[source]
            + top.pixels[source + 1]
            + top.pixels[source + top.width]
            + top.pixels[source + top.width + 1])
            * 0.25;
    }
    Ok(Layer {
        width,
        height,
        pixels,
        noise_variance: top.noise_variance * 0.25,
    })
}

fn local_regression(guide: &Layer, source: &Layer) -> Result<Vec<[f32; 2]>> {
    let mut coefficients = zeroed(source.pixels.len())?;
    for (index, coefficient) in coefficients.iter_mut().enumerate() {
        let (x, y) = (index % source.width, index / source.width);
        let mut sums = [0.0f64; 4];
        let mut count = 0.0;
        for ny in y.saturating_sub(2)..=(y + 2).min(source.height - 1) {
            for nx in x.saturating_sub(2)..=(x + 2).min(source.width - 1) {
                let i = ny * source.width + nx;
                let g = f64::from(guide.pixels[i]);
                let p = f64::from(source.pixels[i]);
                sums[0] += g;
                sums[1] += p;
                sums[2] += g * g;
                sums[3] += g * p;
                count += 1.0;
            }
        }
        let mean_g = sums[0] / count;
        let mean_p = sums[1] / count;
        let variance = (sums[2] / count - mean_g * mean_g).max(0.0);
        let covariance = sums[3] / count - mean_g * mean_p;
        let slope = covariance / (variance + f64::from(guide.noise_variance).max(1e-10));
        *coefficient = [slope as f32, (mean_p - slope * mean_g) as f32];
    }
    Ok(coefficients)
}

fn sample_coefficients(
    values: &[[f32; 2]],
    width: usize,
    height: usize,
    x: f32,
    y: f32,
) -> [f32; 2] {
    let (x0, x1, fx) = interpolation_position(x, width);
    let (y0, y1, fy) = interpolation_position(y, height);
    std::array::from_fn(|channel| {
        let upper =
            values[y0 * width + x0][channel] * (1.0 - fx) + values[y0 * width + x1][channel] * fx;
        let lower =
            values[y1 * width + x0][channel] * (1.0 - fx) + values[y1 * width + x1][channel] * fx;
        upper * (1.0 - fy) + lower * fy
    })
}

fn interpolation_position(coordinate: f32, size: usize) -> (usize, usize, f32) {
    let coordinate = coordinate.clamp(0.0, (size - 1) as f32);
    let lower = coordinate as usize;
    (lower, (lower + 1).min(size - 1), coordinate - lower as f32)
}

impl Layer {
    fn sample(&self, x: f32, y: f32) -> f32 {
        let (x0, x1, fx) = interpolation_position(x, self.width);
        let (y0, y1, fy) = interpolation_position(y, self.height);
        let upper =
            self.pixels[y0 * self.width + x0] * (1.0 - fx) + self.pixels[y0 * self.width + x1] * fx;
        let lower =
            self.pixels[y1 * self.width + x0] * (1.0 - fx) + self.pixels[y1 * self.width + x1] * fx;
        upper * (1.0 - fy) + lower * fy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilinear_preserves_negative_and_above_white_values() {
        let layer = Layer {
            width: 2,
            height: 2,
            pixels: vec![-1.0, 2.0, -1.0, 2.0],
            noise_variance: 0.0,
        };
        assert_eq!(
            [
                layer.sample(0.0, 0.0),
                layer.sample(1.0, 1.0),
                layer.sample(0.5, 0.5)
            ],
            [-1.0, 2.0, 0.5]
        );
    }

    #[test]
    fn guided_constant_chroma_does_not_invent_top_layer_edges() {
        let guide = Layer {
            width: 4,
            height: 4,
            pixels: (0..16).map(|x| (x % 2) as f32).collect(),
            noise_variance: 0.01,
        };
        let source = Layer {
            width: 4,
            height: 4,
            pixels: vec![0.3; 16],
            noise_variance: 0.01,
        };
        let coefficients = local_regression(&guide, &source).unwrap();
        assert!(
            coefficients
                .iter()
                .all(|&[a, b]| a.abs() < 1e-6 && (b - 0.3).abs() < 1e-6)
        );
    }

    #[test]
    fn guided_recovers_correlated_linear_signal() {
        let guide = Layer {
            width: 4,
            height: 4,
            pixels: (0..16).map(|x| x as f32 / 16.0).collect(),
            noise_variance: 0.0,
        };
        let source = Layer {
            width: 4,
            height: 4,
            pixels: guide.pixels.iter().map(|x| 0.5 * x - 0.1).collect(),
            noise_variance: 0.0,
        };
        let coefficients = local_regression(&guide, &source).unwrap();
        assert!(
            coefficients
                .iter()
                .all(|&[a, b]| (a - 0.5).abs() < 1e-6 && (b + 0.1).abs() < 1e-6)
        );
    }
}
