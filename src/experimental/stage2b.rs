// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! Test-only restoration model using explicit intermediate buffers.
//! This does not decode, calibrate or fully render a camera image.

use crate::{Result, invalid, zeroed};

const MAX_PIXELS: usize = 32 * 1024 * 1024;
const TRANSFORM_EXPONENT: f64 = 0.416_666_656_732_559_2;
const COLOR_EPSILON: f32 = 0.000_1;

struct RestoreInput<'a> {
    width: usize,
    height: usize,
    reference: &'a [[f32; 3]],
    color: &'a [[f32; 3]],
    guide: &'a [[i16; 3]],
    fine: &'a [f32],
    k: f32,
    beta: f32,
    transformed: bool,
    adaptive: bool,
}

struct RestoreOutput {
    rgb: Vec<[u16; 3]>,
    gain: Vec<f32>,
    valid: ValidArea,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ValidArea {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

fn restore(input: RestoreInput<'_>) -> Result<RestoreOutput> {
    let coarse_width = input.width / 2;
    let coarse_height = input.height / 2;
    let pixels = validate(&input, coarse_width, coarse_height)?;
    let valid = ValidArea {
        left: 5,
        top: 5,
        right: input.width - 5,
        bottom: input.height - 4,
    };
    let mut rgb = zeroed(pixels)?;
    let mut gain = zeroed(pixels)?;
    gain.copy_from_slice(input.fine);
    let distances = distance_tables();

    for y in valid.top..valid.bottom {
        for x in valid.left..valid.right {
            let pixel = y * input.width + x;
            let coarse_x = x / 2;
            let coarse_y = y / 2;
            let guide = input.guide[pixel].map(f32::from);
            let guide_max = guide.into_iter().fold(f32::NEG_INFINITY, f32::max);
            let (a, tau) = weighting_parameters(input.transformed, input.adaptive, guide_max);
            let distances = &distances[(y & 1) + 2 * (x & 1)];
            let mut weights = 0.0;
            let mut color = [0.0; 3];
            let mut reference_blue = 0.0;

            for table_y in 0..5 {
                let source_y = coarse_y + table_y - 2;
                for table_x in 0..5 {
                    let source_x = coarse_x + table_x - 2;
                    let source = source_y * coarse_width + source_x;
                    let reference = input.reference[source];
                    let mut difference = 0.0;
                    for (reference, guide) in reference.into_iter().zip(guide) {
                        difference = finite(difference + (reference - guide).abs())?;
                    }
                    let exponent = finite(-a * distances[table_y * 5 + table_x])?;
                    let spatial = finite64(f64::from(exponent).exp())?;
                    let reciprocal = finite(1.0 / finite(1.0 + difference / tau)?)?;
                    let weight = finite((spatial * f64::from(reciprocal)) as f32)?;
                    weights = finite(weights + weight)?;
                    for (channel, output) in color.iter_mut().enumerate() {
                        *output = finite(*output + weight * input.color[source][channel])?;
                    }
                    reference_blue = finite(reference_blue + weight * reference[2])?;
                }
            }

            if weights <= 0.0 {
                return Err(invalid("invalid restore weight"));
            }
            let color = [
                finite(color[0] / weights)?,
                finite(color[1] / weights)?,
                finite(color[2] / weights)?,
            ];
            let reference_blue = finite(reference_blue / weights)?;
            let ratio = detail_ratio(
                input.fine[pixel],
                reference_blue,
                input.k,
                input.transformed,
            )?;
            let ratio = if input.transformed || input.fine[pixel] < 11_000.0 || ratio >= 1.0 {
                ratio
            } else {
                let t = ((input.fine[pixel] - 11_000.0) / 3_000.0).clamp(0.0, 1.0);
                finite(1.0 + (1.0 - t) * (ratio - 1.0))?
            };
            let reference = input.reference[coarse_y * coarse_width + coarse_x];
            let numerator = finite(reference[0] + reference[1] - 2.0 * reference[2])?;
            let chroma = finite(numerator / reference[2].max(COLOR_EPSILON))?;
            let attenuation = finite(7.0 - 4.0 * chroma)?;
            let strength = finite(1.0 - input.beta / finite(attenuation.exp() + 1.0)?)?;
            gain[pixel] = reciprocal_symmetric(ratio, strength)?.clamp(0.001, 1000.0);
            rgb[pixel] = [
                quantize(color[0], input.k, input.transformed)?,
                quantize(color[1], input.k, input.transformed)?,
                quantize(color[2], input.k, input.transformed)?,
            ];
        }
    }

    Ok(RestoreOutput { rgb, gain, valid })
}

fn validate(input: &RestoreInput<'_>, coarse_width: usize, coarse_height: usize) -> Result<usize> {
    if input.width < 10 || input.height < 10 || input.width % 2 != 0 || input.height % 2 != 0 {
        return Err(invalid("invalid restore geometry"));
    }
    let pixels = input
        .width
        .checked_mul(input.height)
        .filter(|&pixels| pixels <= MAX_PIXELS)
        .ok_or_else(|| invalid("restore geometry exceeds resource bound"))?;
    let coarse_pixels = coarse_width
        .checked_mul(coarse_height)
        .ok_or_else(|| invalid("invalid coarse restore geometry"))?;
    if input.reference.len() != coarse_pixels
        || input.color.len() != coarse_pixels
        || input.guide.len() != pixels
        || input.fine.len() != pixels
    {
        return Err(invalid("incompatible restore buffer lengths"));
    }
    if !input.k.is_finite()
        || input.k <= 0.0
        || !input.beta.is_finite()
        || !(0.0..=1.0).contains(&input.beta)
    {
        return Err(invalid("invalid restore parameters"));
    }
    if input
        .reference
        .iter()
        .chain(input.color)
        .flatten()
        .any(|value| !value.is_finite())
        || input.fine.iter().any(|value| !value.is_finite())
    {
        return Err(invalid("non-finite restore input"));
    }
    Ok(pixels)
}

fn distance_tables() -> [[f32; 25]; 4] {
    std::array::from_fn(|phase| {
        let y_parity = (phase & 1) as f32;
        let x_parity = (phase >> 1) as f32;
        std::array::from_fn(|index| {
            let y_index = index / 5;
            let x_index = index % 5;
            let y = 4.0 - y_index as f32 + y_parity * 0.5 - 0.25 - 2.0;
            let x = 4.0 - x_index as f32 + x_parity * 0.5 - 0.25 - 2.0;
            (x * x + y * y).sqrt()
        })
    })
}

fn weighting_parameters(transformed: bool, adaptive: bool, maximum_guide: f32) -> (f32, f32) {
    if transformed || maximum_guide <= 1000.0 {
        return (1.0, 25.0);
    }
    if !adaptive {
        return (1.0, 25.0 * maximum_guide / 1000.0);
    }
    let t = ((maximum_guide - 1000.0) / 9000.0).clamp(0.0, 1.0);
    (t * 3.0 + (1.0 - t), (1.0 - t) * 25.0 + t * 1000.0)
}

fn detail_ratio(fine: f32, reference_blue: f32, k: f32, transformed: bool) -> Result<f32> {
    if reference_blue <= 0.0 {
        return Ok(1.0);
    }
    let fine = fine.max(0.0);
    if !transformed {
        return finite(finite(fine + 5.0)? / finite(reference_blue + 5.0)?);
    }
    let fine = transformed_value(fine, k)?;
    let reference = transformed_value(reference_blue, k)?;
    finite((finite64(fine + 5.0)? / finite64(reference + 5.0)?) as f32)
}

fn transformed_value(value: f32, k: f32) -> Result<f64> {
    let normalized = finite(value / k)?;
    finite64(f64::from(normalized).powf(TRANSFORM_EXPONENT) * f64::from(k))
}

fn reciprocal_symmetric(ratio: f32, strength: f32) -> Result<f32> {
    if ratio > 1.0 {
        finite(1.0 + strength * (ratio - 1.0))
    } else {
        finite(1.0 / finite(1.0 + strength * finite(finite(1.0 / ratio)? - 1.0)?)?)
    }
}

fn quantize(value: f32, k: f32, transformed: bool) -> Result<u16> {
    let value = value.clamp(0.0, 65535.0);
    let value = if transformed {
        finite((transformed_value(value, k)? + 0.5) as f32)?
    } else {
        value + 0.5
    };
    Ok(value.clamp(0.0, 65535.0) as u16)
}

fn finite(value: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid("non-finite restore intermediate"))
    }
}

fn finite64(value: f64) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid("non-finite restore intermediate"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        width: usize,
        height: usize,
        reference: Vec<[f32; 3]>,
        color: Vec<[f32; 3]>,
        guide: Vec<[i16; 3]>,
        fine: Vec<f32>,
    }

    impl Fixture {
        fn input(&self) -> RestoreInput<'_> {
            RestoreInput {
                width: self.width,
                height: self.height,
                reference: &self.reference,
                color: &self.color,
                guide: &self.guide,
                fine: &self.fine,
                k: 13_000.0,
                beta: 0.3,
                transformed: false,
                adaptive: false,
            }
        }
    }

    fn fixture(width: usize, height: usize) -> Fixture {
        let coarse = width / 2 * (height / 2);
        Fixture {
            width,
            height,
            reference: vec![[1000.0, 1200.0, 800.0]; coarse],
            color: vec![[500.0, 600.0, 400.0]; coarse],
            guide: vec![[1000, 1200, 800]; width * height],
            fine: vec![800.0; width * height],
        }
    }

    #[test]
    fn constant_field_preserves_color_and_unity_gain() {
        let fixture = fixture(32, 24);
        let output = restore(fixture.input()).unwrap();
        for y in output.valid.top..output.valid.bottom {
            for x in output.valid.left..output.valid.right {
                assert_eq!(output.rgb[y * 32 + x], [500, 600, 400]);
                assert!((output.gain[y * 32 + x] - 1.0).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn geometry_parameters_lengths_and_non_finite_inputs_are_rejected() {
        let invalid_geometry = fixture(31, 24);
        assert!(restore(invalid_geometry.input()).is_err());
        let mut invalid_input = fixture(32, 24);
        invalid_input.fine.fill(f32::NAN);
        assert!(restore(invalid_input.input()).is_err());
        let mut invalid_input = fixture(32, 24);
        invalid_input.reference[0][0] = f32::NAN;
        assert!(restore(invalid_input.input()).is_err());
        let mut mismatched = fixture(32, 24);
        mismatched.color.pop();
        assert!(restore(mismatched.input()).is_err());
        let fixture = fixture(32, 24);
        let mut invalid_parameters = fixture.input();
        invalid_parameters.k = 0.0;
        assert!(restore(invalid_parameters).is_err());
        let mut invalid_parameters = fixture.input();
        invalid_parameters.beta = 1.1;
        assert!(restore(invalid_parameters).is_err());
    }

    #[test]
    fn checked_geometry_overflow_is_rejected_before_buffer_validation() {
        let input = RestoreInput {
            width: usize::MAX - 1,
            height: 10,
            reference: &[],
            color: &[],
            guide: &[],
            fine: &[],
            k: 1.0,
            beta: 0.0,
            transformed: false,
            adaptive: false,
        };
        assert!(restore(input).is_err());
    }

    #[test]
    fn finite_inputs_with_overflowing_intermediates_are_rejected() {
        let mut fixture = fixture(32, 24);
        fixture.reference.fill([f32::MAX; 3]);
        assert!(restore(fixture.input()).is_err());
    }

    #[test]
    fn negative_fine_uses_the_normal_ratio_floor() {
        let mut fixture = fixture(32, 24);
        fixture.fine.fill(-100.0);
        let output = restore(fixture.input()).unwrap();
        assert!(output.gain[5 * 32 + 5] < 1.0);
    }

    #[test]
    fn nonpositive_weighted_reference_blue_uses_unity_gain_in_both_modes() {
        let mut normal = fixture(32, 24);
        normal.reference.fill([1000.0, 1200.0, 0.0]);
        normal.fine.fill(-100.0);
        assert_eq!(restore(normal.input()).unwrap().gain[5 * 32 + 5], 1.0);
        let mut transformed = fixture(32, 24);
        transformed.reference.fill([1000.0, 1200.0, 0.0]);
        transformed.fine.fill(-100.0);
        let mut input = transformed.input();
        input.k = 9000.0;
        input.transformed = true;
        assert_eq!(restore(input).unwrap().gain[5 * 32 + 5], 1.0);
    }

    #[test]
    fn highlight_suppression_reaches_its_endpoints() {
        let mut at_start = fixture(32, 24);
        at_start.reference.fill([1000.0, 1200.0, 16_000.0]);
        at_start.fine.fill(11_000.0);
        let at_start = restore(at_start.input()).unwrap().gain[5 * 32 + 5];
        let mut at_end = fixture(32, 24);
        at_end.reference.fill([1000.0, 1200.0, 16_000.0]);
        at_end.fine.fill(14_000.0);
        let at_end = restore(at_end.input()).unwrap().gain[5 * 32 + 5];
        assert!(at_start < 1.0);
        assert!((at_end - 1.0).abs() < 1e-6);
    }

    #[test]
    fn all_distance_phases_are_distinct_and_finite() {
        let tables = distance_tables();
        assert!(tables.iter().flatten().all(|value| value.is_finite()));
        assert_ne!(tables[0], tables[1]);
        assert_ne!(tables[0], tables[2]);
        assert_ne!(tables[1], tables[3]);
    }
}
