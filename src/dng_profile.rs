use crate::dng_metadata::{allocate, number};
use crate::{ByteOrder, DecodeError, DngTag, ProbeResult};

/// A validated profile gain table. Evaluation requires linear RIMM/ProPhoto RGB,
/// not camera-native channels. This does not perform white balance or color conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileGainTable {
    points: [usize; 3],
    spacing: [f64; 2],
    origin: [f64; 2],
    weights: [f64; 5],
    gamma: f64,
    gains: Vec<f32>,
}

impl ProfileGainTable {
    /// Parses ProfileGainTableMap (52525) or ProfileGainTableMap2 (52544).
    pub fn parse(tag: &DngTag, order: ByteOrder) -> ProbeResult<Self> {
        if !matches!(tag.id, 52525 | 52544) || !matches!(tag.field_type, 1 | 7) {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = &tag.data;
        if tag.count as usize != bytes.len() {
            return Err(DecodeError::InvalidTag);
        }
        let at = |offset, kind| {
            number(
                bytes.get(offset..).ok_or(DecodeError::Truncated)?,
                kind,
                0,
                order,
            )
        };
        let points = [at(0, 4)? as usize, at(4, 4)? as usize, at(40, 4)? as usize];
        let spacing = [at(8, 12)?, at(16, 12)?];
        let origin = [at(24, 12)?, at(32, 12)?];
        let weights = [
            at(44, 11)?,
            at(48, 11)?,
            at(52, 11)?,
            at(56, 11)?,
            at(60, 11)?,
        ];
        let (kind, gamma, minimum, maximum, header) = if tag.id == 52544 {
            (
                at(64, 4)? as u32,
                at(68, 11)?,
                at(72, 11)?,
                at(76, 11)?,
                80usize,
            )
        } else {
            (3, 1.0, 0.0, 1.0, 64usize)
        };
        if points.contains(&0)
            || spacing.iter().any(|value| *value <= 0.0)
            || gamma <= 0.0
            || minimum > maximum
        {
            return Err(DecodeError::InvalidTag);
        }
        let unit = match kind {
            0 => 1,
            1 | 2 => 2,
            3 => 4,
            _ => return Err(DecodeError::UnsupportedFeature),
        };
        let count = points[0]
            .checked_mul(points[1])
            .and_then(|value| value.checked_mul(points[2]))
            .ok_or(DecodeError::ResourceLimit)?;
        if count
            .checked_mul(unit)
            .and_then(|value| value.checked_add(header))
            != Some(bytes.len())
        {
            return Err(DecodeError::InvalidTag);
        }
        let mut gains = allocate(count)?;
        for index in 0..count {
            let offset = header + index * unit;
            let value = match kind {
                0 => minimum + (maximum - minimum) * f64::from(bytes[offset]) / 255.0,
                1 => minimum + (maximum - minimum) * at(offset, 3)? / 65535.0,
                2 => half(at(offset, 3)? as u16),
                _ => at(offset, 11)?,
            };
            if !value.is_finite()
                || value <= 0.0
                || value > f64::from(f32::MAX)
                || (value as f32) == 0.0
            {
                return Err(DecodeError::InvalidTag);
            }
            gains.push(value as f32);
        }
        Ok(Self {
            points,
            spacing,
            origin,
            weights,
            gamma,
            gains,
        })
    }

    pub fn dimensions(&self) -> [usize; 3] {
        self.points
    }

    /// Evaluates a common RGB gain at normalized pixel-center `[row, column]` coordinates.
    /// `exposure_weight` is 1 after baseline exposure, otherwise the baseline-exposure gain.
    pub fn gain(
        &self,
        rgb: [f64; 3],
        position: [f64; 2],
        exposure_weight: f64,
    ) -> ProbeResult<f64> {
        if rgb
            .iter()
            .chain(position.iter())
            .any(|value| !value.is_finite())
            || !exposure_weight.is_finite()
            || exposure_weight < 0.0
        {
            return Err(DecodeError::InvalidTag);
        }
        let minimum = rgb.into_iter().fold(f64::INFINITY, f64::min);
        let maximum = rgb.into_iter().fold(f64::NEG_INFINITY, f64::max);
        let input = (self.weights[0] * rgb[0]
            + self.weights[1] * rgb[1]
            + self.weights[2] * rgb[2]
            + self.weights[3] * minimum
            + self.weights[4] * maximum)
            * exposure_weight;
        if !input.is_finite() {
            return Err(DecodeError::InvalidTag);
        }
        let coordinates = [
            ((position[0] - self.origin[0]) / self.spacing[0])
                .clamp(0.0, (self.points[0] - 1) as f64),
            ((position[1] - self.origin[1]) / self.spacing[1])
                .clamp(0.0, (self.points[1] - 1) as f64),
            input.clamp(0.0, 1.0).powf(self.gamma) * self.points[2] as f64,
        ];
        let base = std::array::from_fn::<_, 3, _>(|axis| {
            (coordinates[axis] as usize).min(self.points[axis] - 1)
        });
        let next =
            std::array::from_fn::<_, 3, _>(|axis| (base[axis] + 1).min(self.points[axis] - 1));
        let fraction = std::array::from_fn::<_, 3, _>(|axis| coordinates[axis] - base[axis] as f64);
        let mut result = 0.0;
        for vertex in 0..8 {
            let mut position = [0; 3];
            let mut weight = 1.0;
            for axis in 0..3 {
                let high = vertex & (1 << axis) != 0;
                position[axis] = if high { next[axis] } else { base[axis] };
                weight *= if high {
                    fraction[axis]
                } else {
                    1.0 - fraction[axis]
                };
            }
            result += weight
                * f64::from(
                    self.gains[(position[0] * self.points[1] + position[1]) * self.points[2]
                        + position[2]],
                );
        }
        Ok(result)
    }
}

fn half(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = (bits >> 10) & 31;
    let fraction = f64::from(bits & 1023);
    match exponent {
        0 => sign * fraction * 2.0f64.powi(-24),
        31 => f64::NAN,
        _ => sign * (1.0 + fraction / 1024.0) * 2.0f64.powi(i32::from(exponent) - 15),
    }
}
