// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! Test-only pre-color reconstruction diagnostics under the renderer's assumed 2x2 footprint.
//! Residuals measure consistency with calibrated native samples, not physical reconstruction error.

use super::*;
use crate::X3f;
use std::path::Path;

struct Projection<'a> {
    source: &'a Layer,
    top: &'a Layer,
    regression: &'a Regression,
}

#[derive(Default)]
struct ProjectedSample {
    bilinear: f32,
    guided: f32,
    excluded: bool,
}

impl Projection<'_> {
    fn sample(&self, x: usize, y: usize) -> ProjectedSample {
        let mut sample = ProjectedSample::default();
        for sy in 2 * y..2 * y + 2 {
            for sx in 2 * x..2 * x + 2 {
                let low_x = (sx as f32 - 0.5) * 0.5;
                let low_y = (sy as f32 - 0.5) * 0.5;
                let support =
                    interpolation_support(low_x, low_y, self.source.width, self.source.height);
                let [slope, intercept] = sample_coefficients(
                    &self.regression.coefficients,
                    self.source.width,
                    self.source.height,
                    low_x,
                    low_y,
                );
                let top_index = sy * self.top.width + sx;
                sample.bilinear += self.source.sample(low_x, low_y) * 0.25;
                sample.guided += (slope * self.top.pixels[top_index] + intercept) * 0.25;
                sample.excluded |= self.excluded(&support, top_index);
            }
        }
        sample
    }

    fn excluded(&self, support: &InterpolationSupport, top_index: usize) -> bool {
        self.top
            .influence
            .as_ref()
            .is_some_and(|mask| mask[top_index] != 0)
            || self
                .source
                .influence
                .as_ref()
                .is_some_and(|mask| support_has_influence(support, mask))
            || self
                .regression
                .influence
                .as_ref()
                .is_some_and(|mask| support_has_influence(support, mask))
    }
}

#[derive(Default)]
struct Residuals {
    sum: f64,
    squared: f64,
    absolute: Vec<f64>,
}

struct Summary {
    count: usize,
    bias: f64,
    rms: f64,
    p95: f64,
}

impl Residuals {
    fn record(&mut self, reference: f32, candidate: f32, codes_per_unit: f64) {
        let residual = (f64::from(candidate) - f64::from(reference)) * codes_per_unit;
        assert!(residual.is_finite());
        self.sum += residual;
        self.squared += residual * residual;
        self.absolute.push(residual.abs());
    }

    fn summarize(mut self) -> Summary {
        let count = self.absolute.len();
        assert!(count > 0, "no unmasked native samples");
        let percentile = (count * 95).div_ceil(100) - 1;
        let (_, &mut p95, _) = self
            .absolute
            .select_nth_unstable_by(percentile, f64::total_cmp);
        Summary {
            count,
            bias: self.sum / count as f64,
            rms: (self.squared / count as f64).sqrt(),
            p95,
        }
    }
}

fn diagnostic_layer(plane: &Plane, calibration: &Calibration, channel: usize) -> Layer {
    let (mut layer, clipping) = calibrate(plane, calibration, channel, true).unwrap();
    let marked = marked_pixels(plane, calibration, channel).unwrap();
    let mut excluded = clipping.unwrap().mask;
    for (flag, bad) in excluded.iter_mut().zip(marked) {
        *flag |= u8::from(bad);
    }
    layer.influence = Some(excluded);
    layer
}

fn codes_per_unit(plane: &Plane, calibration: &Calibration) -> f64 {
    let columns = calibration.values::<4>("DarkShieldColRange").unwrap();
    let (black, _) = measure_black(plane, columns.map(|value| value as usize / 2)).unwrap();
    let [depth] = calibration.values::<1>("ImageDepth").unwrap();
    f64::from(((1u32 << depth as u32) - 1) as f32 - black)
}

fn measure_projection(
    projection: &Projection<'_>,
    crop: Crop,
    scale: f64,
) -> (usize, [Summary; 2]) {
    let mut bilinear = Residuals::default();
    let mut guided = Residuals::default();
    let mut excluded = 0;
    let columns = native_interior(crop.left, crop.right);
    for y in native_interior(crop.top, crop.bottom) {
        for x in columns.clone() {
            let sample = projection.sample(x, y);
            if sample.excluded {
                excluded += 1;
                continue;
            }
            let reference = projection.source.pixels[y * projection.source.width + x];
            bilinear.record(reference, sample.bilinear, scale);
            guided.record(reference, sample.guided, scale);
        }
    }
    (excluded, [bilinear.summarize(), guided.summarize()])
}

fn native_interior(first: usize, last: usize) -> std::ops::Range<usize> {
    let start = first.div_ceil(2) + 3;
    let end = last
        .div_ceil(2)
        .checked_sub(3)
        .expect("crop too narrow for regression support");
    assert!(start < end, "empty diagnostic crop");
    start..end
}

fn report_sample(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    let file = X3f::parse(&bytes).unwrap();
    let sensor = file.decode().unwrap();
    validate_geometry(&sensor).unwrap();
    let calibration = file.calibration().unwrap();
    assert_eq!(calibration.values::<1>("CAMERAID").unwrap(), [40.0]);
    let crop = Crop::parse(
        calibration.values("ActiveImageArea").unwrap(),
        &sensor.layers[2],
    )
    .unwrap();
    let top = diagnostic_layer(&sensor.layers[2], &calibration, 2);
    let guide = downsample(&top).unwrap();
    for (channel, plane) in sensor.layers[..2].iter().enumerate() {
        let source = diagnostic_layer(plane, &calibration, channel);
        let regression = local_regression(&guide, &source).unwrap();
        let projection = Projection {
            source: &source,
            top: &top,
            regression: &regression,
        };
        let (excluded, summaries) =
            measure_projection(&projection, crop, codes_per_unit(plane, &calibration));
        for (method, summary) in ["bilinear", "guided"].into_iter().zip(summaries) {
            eprintln!(
                "{}\t{channel}\t{method}\t{}\t{excluded}\t{:.6}\t{:.6}\t{:.6}",
                path.display(),
                summary.count,
                summary.bias,
                summary.rms,
                summary.p95
            );
        }
    }
}

#[test]
#[ignore = "requires X3F_SAMPLE_DIR containing the six supported sd Quattro corpus files; reports diagnostics, not calibrated accuracy"]
fn sd_quattro_corpus_reports_native_layer_residuals() {
    let root =
        std::path::PathBuf::from(std::env::var_os("X3F_SAMPLE_DIR").expect("X3F_SAMPLE_DIR"));
    eprintln!("sample\tlayer\tmethod\taccepted\texcluded\tbias_codes\trms_codes\tp95_abs_codes");
    for relative in [
        "20220305-of_intermedio_ritratto-001.x3f",
        "20220305-of_intermedio_ritratto-002.x3f",
        "20220305-of_intermedio_ritratto-003.x3f",
        "20220305-of_intermedio_ritratto-004.x3f",
        "20220305-of_intermedio_ritratto-005.x3f",
        "raw-pixls-us/sd Quattro/sample3.X3F",
    ] {
        report_sample(&root.join(relative));
    }
}

fn constant_layer(width: usize, height: usize, value: f32) -> Layer {
    Layer {
        width,
        height,
        pixels: vec![value; width * height],
        noise_variance: 0.0,
        influence: Some(vec![0; width * height]),
    }
}

#[test]
fn projection_preserves_constant_native_signal() {
    let source = constant_layer(12, 12, 0.75);
    let top = constant_layer(24, 24, 0.5);
    let guide = downsample(&top).unwrap();
    let regression = local_regression(&guide, &source).unwrap();
    let sample = Projection {
        source: &source,
        top: &top,
        regression: &regression,
    }
    .sample(6, 6);
    assert_eq!(
        (sample.bilinear, sample.guided, sample.excluded),
        (0.75, 0.75, false)
    );
}

#[test]
fn projection_preserves_affine_signal_with_half_pixel_alignment() {
    let mut source = constant_layer(12, 12, 0.0);
    for (index, pixel) in source.pixels.iter_mut().enumerate() {
        *pixel = 0.525 + 0.02 * (index % 12) as f32 + 0.01 * (index / 12) as f32;
    }
    let mut top = constant_layer(24, 24, 0.0);
    for (index, pixel) in top.pixels.iter_mut().enumerate() {
        *pixel = 0.2 + 0.005 * ((index % 24) as f32 - 0.5) + 0.0025 * ((index / 24) as f32 - 0.5);
    }
    let guide = downsample(&top).unwrap();
    let regression = local_regression(&guide, &source).unwrap();
    let sample = Projection {
        source: &source,
        top: &top,
        regression: &regression,
    }
    .sample(6, 6);
    assert!((sample.bilinear - 0.705).abs() < 1e-6);
    assert!((sample.guided - 0.705).abs() < 1e-6);
}

#[test]
fn flat_guide_spreads_native_impulse_across_regression_window() {
    let mut source = constant_layer(12, 12, 0.5);
    source.pixels[6 * 12 + 6] = 0.75;
    let top = constant_layer(24, 24, 0.5);
    let guide = downsample(&top).unwrap();
    let regression = local_regression(&guide, &source).unwrap();
    let sample = Projection {
        source: &source,
        top: &top,
        regression: &regression,
    }
    .sample(6, 6);
    assert!((sample.bilinear - 0.640625).abs() < 1e-6);
    assert!((sample.guided - 0.51).abs() < 1e-6);
    assert!(!sample.excluded);
}

#[test]
fn projection_excludes_indirect_regression_support() {
    let mut source = constant_layer(12, 12, 0.5);
    source.influence.as_mut().unwrap()[4 * 12 + 4] = 1;
    let top = constant_layer(24, 24, 0.5);
    let guide = downsample(&top).unwrap();
    let regression = local_regression(&guide, &source).unwrap();
    let projection = Projection {
        source: &source,
        top: &top,
        regression: &regression,
    };
    assert!(projection.sample(6, 6).excluded);
    assert!(!projection.sample(9, 9).excluded);
}

#[test]
fn residual_summary_uses_signed_bias_and_nearest_rank_percentile() {
    let mut residuals = Residuals::default();
    for value in [1.0, -2.0, 3.0, -4.0] {
        residuals.record(0.0, value, 2.0);
    }
    let summary = residuals.summarize();
    assert_eq!(summary.count, 4);
    assert_eq!(summary.bias, -1.0);
    assert_eq!(summary.rms, 30.0f64.sqrt());
    assert_eq!(summary.p95, 8.0);
}
