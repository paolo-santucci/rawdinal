use crate::dng_metadata::allocate;
use crate::{DecodeError, LinearRawProcessing, ProbeResult};

pub(crate) struct OpcodeList {
    operations: Vec<Operation>,
    skipped: bool,
}

enum Operation {
    Trim([u32; 4]),
    Map(Area, Mapping),
}

enum Mapping {
    Table(Vec<u16>),
    Polynomial(Vec<f64>),
    Gain(GainMap),
    RowDelta(Vec<f64>),
    ColumnDelta(Vec<f64>),
    RowScale(Vec<f64>),
    ColumnScale(Vec<f64>),
}

struct GainMap {
    points: [usize; 2],
    spacing: [f64; 2],
    origin: [f64; 2],
    planes: usize,
    gains: Vec<f64>,
}

#[derive(Clone, Copy)]
struct Area {
    bounds: [u32; 4],
    plane: usize,
    planes: usize,
    pitch: [usize; 2],
}

pub(crate) struct Stage<'a> {
    pub samples: &'a mut [f32],
    pub width: u32,
    pub origin: [u32; 2],
    pub bounds: [u32; 4],
    pub number: u8,
}

impl OpcodeList {
    pub fn parse(bytes: &[u8]) -> ProbeResult<Self> {
        let mut reader = Reader::new(bytes);
        let count = reader.u32()? as usize;
        if count > 256 {
            return Err(DecodeError::ResourceLimit);
        }
        let mut operations = allocate(count)?;
        let mut skipped = false;
        for _ in 0..count {
            let id = reader.u32()?;
            let version = reader.u32()?;
            let flags = reader.u32()?;
            let length = reader.u32()? as usize;
            let payload = reader.take(length)?;
            if flags & !3 != 0 {
                return Err(DecodeError::UnsupportedFeature);
            }
            let parsed = if version > 0x01070100 {
                Err(DecodeError::UnsupportedFeature)
            } else {
                Operation::parse(id, payload)
            };
            match parsed {
                Ok(operation) => operations.push(operation),
                Err(DecodeError::UnsupportedFeature) if flags & 1 != 0 => skipped = true,
                Err(error) => return Err(error),
            }
        }
        reader.finish()?;
        Ok(Self {
            operations,
            skipped,
        })
    }

    pub fn processing(&self) -> LinearRawProcessing {
        match (self.operations.is_empty(), self.skipped) {
            (true, false) => LinearRawProcessing::NotPresent,
            (false, false) => LinearRawProcessing::Applied,
            (true, true) => LinearRawProcessing::SkippedOptional,
            (false, true) => LinearRawProcessing::PartiallyApplied,
        }
    }

    pub fn apply(&self, stage: &mut Stage<'_>) -> ProbeResult<()> {
        for operation in &self.operations {
            match operation {
                Operation::Trim(bounds) => {
                    if bounds[0] < stage.bounds[0]
                        || bounds[1] < stage.bounds[1]
                        || bounds[2] > stage.bounds[2]
                        || bounds[3] > stage.bounds[3]
                    {
                        return Err(DecodeError::InvalidGeometry);
                    }
                    stage.bounds = *bounds;
                }
                Operation::Map(area, mapping) => mapping.apply(*area, stage)?,
            }
        }
        Ok(())
    }
}

impl Operation {
    fn parse(id: u32, bytes: &[u8]) -> ProbeResult<Self> {
        if !(6..=13).contains(&id) {
            return Err(DecodeError::UnsupportedFeature);
        }
        let mut reader = Reader::new(bytes);
        if id == 6 {
            let bounds = reader.bounds()?;
            reader.finish()?;
            if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
                return Err(DecodeError::InvalidGeometry);
            }
            return Ok(Self::Trim(bounds));
        }
        let area = Area::parse(&mut reader)?;
        let mapping = match id {
            7 => {
                let count = reader.u32()? as usize;
                if !(1..=65536).contains(&count) {
                    return Err(DecodeError::InvalidTag);
                }
                let mut values = allocate(count)?;
                for _ in 0..count {
                    values.push(u16::from_be_bytes(reader.take(2)?.try_into().unwrap()));
                }
                Mapping::Table(values)
            }
            8 => {
                let degree = reader.u32()? as usize;
                if degree > 8 {
                    return Err(DecodeError::UnsupportedFeature);
                }
                let mut values = allocate(degree + 1)?;
                for _ in 0..=degree {
                    values.push(reader.f64()?);
                }
                Mapping::Polynomial(values)
            }
            9 => Mapping::Gain(GainMap::parse(&mut reader)?),
            10..=13 => {
                let count = reader.u32()? as usize;
                let axis = usize::from(id % 2 == 1);
                let expected = (area.bounds[axis + 2] - area.bounds[axis]) as usize;
                if count != expected.div_ceil(area.pitch[axis]) || count == 0 {
                    return Err(DecodeError::InvalidTag);
                }
                let values = reader.floats(count)?;
                match id {
                    10 => Mapping::RowDelta(values),
                    11 => Mapping::ColumnDelta(values),
                    12 => Mapping::RowScale(values),
                    _ => Mapping::ColumnScale(values),
                }
            }
            _ => return Err(DecodeError::UnsupportedFeature),
        };
        reader.finish()?;
        Ok(Self::Map(area, mapping))
    }
}

impl Area {
    fn parse(reader: &mut Reader<'_>) -> ProbeResult<Self> {
        let bounds = reader.bounds()?;
        let plane = reader.u32()? as usize;
        let planes = reader.u32()? as usize;
        let pitch = [reader.u32()? as usize, reader.u32()? as usize];
        let empty = bounds[0] == bounds[2] || bounds[1] == bounds[3];
        if empty && (bounds != [0; 4] || pitch != [1, 1]) {
            return Err(DecodeError::UnsupportedFeature);
        }
        if bounds[0] > bounds[2]
            || bounds[1] > bounds[3]
            || pitch.contains(&0)
            || planes == 0
            || plane.checked_add(planes).is_none_or(|end| end > 3)
        {
            return Err(DecodeError::InvalidTag);
        }
        Ok(Self {
            bounds,
            plane,
            planes,
            pitch,
        })
    }
}

impl Mapping {
    fn apply(&self, mut area: Area, stage: &mut Stage<'_>) -> ProbeResult<()> {
        if area.bounds == [0; 4] {
            area.bounds = stage.bounds;
        }
        let start = [
            aligned_start(area.bounds[0], stage.bounds[0], area.pitch[0]),
            aligned_start(area.bounds[1], stage.bounds[1], area.pitch[1]),
        ];
        let end = [
            area.bounds[2].min(stage.bounds[2]),
            area.bounds[3].min(stage.bounds[3]),
        ];
        for row in (start[0]..end[0] as usize).step_by(area.pitch[0]) {
            for column in (start[1]..end[1] as usize).step_by(area.pitch[1]) {
                let pixel = (row + stage.origin[0] as usize) * stage.width as usize
                    + column
                    + stage.origin[1] as usize;
                for channel in area.plane..area.plane + area.planes {
                    let index = pixel
                        .checked_mul(3)
                        .and_then(|n| n.checked_add(channel))
                        .ok_or(DecodeError::ResourceLimit)?;
                    let sample = f64::from(
                        *stage
                            .samples
                            .get(index)
                            .ok_or(DecodeError::InvalidGeometry)?,
                    );
                    let y = (row - area.bounds[0] as usize) / area.pitch[0];
                    let x = (column - area.bounds[1] as usize) / area.pitch[1];
                    let scale = if stage.number == 1 { 65535.0 } else { 1.0 };
                    let value = match self {
                        Self::Table(table) => {
                            let index = if stage.number == 1 {
                                sample
                            } else {
                                sample * 65535.0
                            };
                            f64::from(
                                table[(index.clamp(0.0, 65535.0).round() as usize)
                                    .min(table.len() - 1)],
                            ) / if stage.number == 1 { 1.0 } else { 65535.0 }
                        }
                        Self::Polynomial(coefficients) => {
                            let value = coefficients
                                .iter()
                                .skip(1)
                                .rev()
                                .fold(0.0, |value, &coefficient| {
                                    value * sample.abs() + coefficient
                                })
                                * sample.abs();
                            (coefficients[0] + sample.signum() * value).clamp(-scale, scale)
                        }
                        Self::Gain(map) => {
                            (sample * map.gain(row, column, channel, stage.bounds)).min(scale)
                        }
                        Self::RowDelta(values) => (sample + values[y]).clamp(-scale, scale),
                        Self::ColumnDelta(values) => (sample + values[x]).clamp(-scale, scale),
                        Self::RowScale(values) => (sample * values[y]).clamp(-scale, scale),
                        Self::ColumnScale(values) => (sample * values[x]).clamp(-scale, scale),
                    };
                    let value = if stage.number == 1 {
                        value.clamp(0.0, 65535.0).round()
                    } else {
                        value
                    };
                    if !value.is_finite() {
                        return Err(DecodeError::InvalidTag);
                    }
                    stage.samples[index] = value as f32;
                }
            }
        }
        Ok(())
    }
}

fn aligned_start(start: u32, minimum: u32, pitch: usize) -> usize {
    start as usize + (minimum.saturating_sub(start) as usize).div_ceil(pitch) * pitch
}

impl GainMap {
    fn parse(reader: &mut Reader<'_>) -> ProbeResult<Self> {
        let points = [reader.u32()? as usize, reader.u32()? as usize];
        let spacing = [reader.f64()?, reader.f64()?];
        let origin = [reader.f64()?, reader.f64()?];
        let planes = reader.u32()? as usize;
        if points.contains(&0)
            || spacing.iter().any(|value| *value <= 0.0)
            || !(1..=3).contains(&planes)
        {
            return Err(DecodeError::InvalidTag);
        }
        let count = points[0]
            .checked_mul(points[1])
            .and_then(|n| n.checked_mul(planes))
            .ok_or(DecodeError::ResourceLimit)?;
        let gains = reader.floats(count)?;
        if gains.iter().any(|gain| *gain < 0.0) {
            return Err(DecodeError::InvalidTag);
        }
        Ok(Self {
            points,
            spacing,
            origin,
            planes,
            gains,
        })
    }

    fn gain(&self, row: usize, column: usize, channel: usize, bounds: [u32; 4]) -> f64 {
        let position = [row, column];
        let mut base = [0; 2];
        let mut next = [0; 2];
        let mut fraction = [0.0; 2];
        for axis in 0..2 {
            let normalized = (position[axis] as f64 - f64::from(bounds[axis]) + 0.5)
                / f64::from(bounds[axis + 2] - bounds[axis]);
            let map = ((normalized - self.origin[axis]) / self.spacing[axis])
                .clamp(0.0, (self.points[axis] - 1) as f64);
            base[axis] = map as usize;
            next[axis] = (base[axis] + 1).min(self.points[axis] - 1);
            fraction[axis] = map - base[axis] as f64;
        }
        let gain = |y, x| {
            self.gains[(y * self.points[1] + x) * self.planes + channel.min(self.planes - 1)]
        };
        let top =
            gain(base[0], base[1]) * (1.0 - fraction[1]) + gain(base[0], next[1]) * fraction[1];
        let bottom =
            gain(next[0], base[1]) * (1.0 - fraction[1]) + gain(next[0], next[1]) * fraction[1];
        top * (1.0 - fraction[0]) + bottom * fraction[0]
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    pub fn take(&mut self, count: usize) -> ProbeResult<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(DecodeError::ResourceLimit)?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(DecodeError::Truncated)?;
        self.position = end;
        Ok(bytes)
    }
    pub fn u32(&mut self) -> ProbeResult<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f64(&mut self) -> ProbeResult<f64> {
        let value = f64::from_be_bytes(self.take(8)?.try_into().unwrap());
        if !value.is_finite() {
            return Err(DecodeError::InvalidTag);
        }
        Ok(value)
    }
    pub fn floats(&mut self, count: usize) -> ProbeResult<Vec<f64>> {
        let bytes = self.take(count.checked_mul(4).ok_or(DecodeError::ResourceLimit)?)?;
        let mut values = allocate(count)?;
        for bytes in bytes.chunks_exact(4) {
            let value = f32::from_be_bytes(bytes.try_into().unwrap());
            if !value.is_finite() {
                return Err(DecodeError::InvalidTag);
            }
            values.push(f64::from(value));
        }
        Ok(values)
    }
    fn bounds(&mut self) -> ProbeResult<[u32; 4]> {
        let bounds = [self.u32()?, self.u32()?, self.u32()?, self.u32()?];
        if bounds.iter().any(|value| *value > i32::MAX as u32) {
            return Err(DecodeError::InvalidGeometry);
        }
        Ok(bounds)
    }
    pub fn finish(self) -> ProbeResult<()> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(DecodeError::InvalidTag)
        }
    }
}
