use crate::{ByteOrder, DecodeError, ProbeResult};
use std::ops::Range;

/// An owned TIFF field. Payload bytes retain their original TIFF byte order.
#[derive(Debug, Clone, PartialEq)]
pub struct DngTag {
    pub id: u16,
    pub field_type: u16,
    pub count: u32,
    pub data: Vec<u8>,
}

impl DngTag {
    /// Reads a numeric TIFF component without losing rational precision to float32.
    pub fn number(&self, index: usize, order: ByteOrder) -> ProbeResult<f64> {
        number(&self.data, self.field_type, index, order)
    }

    pub fn numbers(&self, order: ByteOrder) -> ProbeResult<Vec<f64>> {
        let mut values = allocate(self.count as usize)?;
        for index in 0..self.count as usize {
            values.push(self.number(index, order)?);
        }
        Ok(values)
    }

    pub fn text(&self) -> ProbeResult<&str> {
        if self.field_type != 2 {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self
            .data
            .split(|&value| value == 0)
            .next()
            .unwrap_or_default();
        std::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidTag)
    }
}

/// Fields belonging to one IFD, never merged with preview or mask fields.
#[derive(Debug, Clone, PartialEq)]
pub struct DngDirectory {
    pub offset: u32,
    pub parent_offset: Option<u32>,
    pub tags: Vec<DngTag>,
}

impl DngDirectory {
    pub fn tag(&self, id: u16) -> Option<&DngTag> {
        self.tags.iter().find(|tag| tag.id == id)
    }
}

/// Owned metadata for the TIFF IFD chain, SubIFDs, EXIF and GPS directories.
/// Maker notes and private fields remain opaque; internal offsets may still refer to the source.
#[derive(Debug, Clone, PartialEq)]
pub struct DngMetadata {
    pub byte_order: ByteOrder,
    pub root_offset: u32,
    pub raw_offset: u32,
    pub directories: Vec<DngDirectory>,
}

impl DngMetadata {
    pub fn directory(&self, offset: u32) -> Option<&DngDirectory> {
        self.directories.iter().find(|ifd| ifd.offset == offset)
    }

    pub fn root(&self) -> &DngDirectory {
        self.directory(self.root_offset)
            .expect("retained root directory")
    }

    pub fn raw(&self) -> &DngDirectory {
        self.directory(self.raw_offset)
            .expect("retained raw directory")
    }

    /// Shared calibration metadata is read from IFD0, not inherited from previews.
    pub fn calibration(&self) -> ProbeResult<DngCalibration> {
        let root = self.root();
        let numbers = |id| {
            root.tag(id)
                .map(|tag| tag.numbers(self.byte_order))
                .transpose()
        };
        Ok(DngCalibration {
            as_shot_neutral: numbers(50728)?,
            as_shot_white_xy: numbers(50729)?,
            analog_balance: numbers(50727)?,
            color_matrices: [numbers(50721)?, numbers(50722)?, numbers(52531)?],
            forward_matrices: [numbers(50964)?, numbers(50965)?, numbers(52532)?],
            camera_calibrations: [numbers(50723)?, numbers(50724)?, numbers(52530)?],
            illuminants: [numbers(50778)?, numbers(50779)?, numbers(52529)?],
        })
    }
}

/// DNG matrix coefficients in row-major order; no color transform is applied.
/// Custom illuminant data, profile signatures and additional profiles are available as scoped tags.
#[derive(Debug, Clone, PartialEq)]
pub struct DngCalibration {
    pub as_shot_neutral: Option<Vec<f64>>,
    pub as_shot_white_xy: Option<Vec<f64>>,
    pub analog_balance: Option<Vec<f64>>,
    pub color_matrices: [Option<Vec<f64>>; 3],
    pub forward_matrices: [Option<Vec<f64>>; 3],
    pub camera_calibrations: [Option<Vec<f64>>; 3],
    pub illuminants: [Option<Vec<f64>>; 3],
}

#[derive(Debug, Clone)]
pub(crate) struct TagRange {
    pub id: u16,
    pub field_type: u16,
    pub count: u32,
    pub range: Range<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct DirectoryRange {
    pub offset: u32,
    pub parent_offset: Option<u32>,
    pub tags: Vec<TagRange>,
}

pub(crate) fn own_metadata(
    bytes: &[u8],
    facts: &crate::probe::DngRawFacts,
) -> ProbeResult<DngMetadata> {
    let mut directories = allocate(facts.directories.len())?;
    for directory in &facts.directories {
        let mut tags = allocate(directory.tags.len())?;
        for tag in &directory.tags {
            let source = bytes
                .get(tag.range.clone())
                .ok_or(DecodeError::InvalidOffset)?;
            let mut data = allocate(source.len())?;
            data.extend_from_slice(source);
            tags.push(DngTag {
                id: tag.id,
                field_type: tag.field_type,
                count: tag.count,
                data,
            });
        }
        directories.push(DngDirectory {
            offset: directory.offset,
            parent_offset: directory.parent_offset,
            tags,
        });
    }
    Ok(DngMetadata {
        byte_order: facts.byte_order,
        root_offset: facts.root_offset,
        raw_offset: facts.raw_offset,
        directories,
    })
}

pub(crate) fn allocate<T>(count: usize) -> ProbeResult<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| DecodeError::Allocation)?;
    Ok(values)
}

pub(crate) fn number(bytes: &[u8], kind: u16, index: usize, order: ByteOrder) -> ProbeResult<f64> {
    let unit = match kind {
        1 | 6 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => return Err(DecodeError::InvalidTag),
    };
    let start = index.checked_mul(unit).ok_or(DecodeError::InvalidTag)?;
    let data = bytes
        .get(start..start.checked_add(unit).ok_or(DecodeError::InvalidTag)?)
        .ok_or(DecodeError::InvalidTag)?;
    let unsigned = |data: &[u8]| -> u64 {
        match order {
            ByteOrder::BigEndian => data
                .iter()
                .fold(0, |value, &byte| value << 8 | u64::from(byte)),
            ByteOrder::LittleEndian => data
                .iter()
                .rev()
                .fold(0, |value, &byte| value << 8 | u64::from(byte)),
        }
    };
    let value = match kind {
        1 | 3 | 4 | 13 => unsigned(data) as f64,
        6 => f64::from(data[0] as i8),
        8 => f64::from(unsigned(data) as i16),
        9 => f64::from(unsigned(data) as i32),
        5 | 10 => {
            let (numerator, denominator) = if kind == 5 {
                (unsigned(&data[..4]) as f64, unsigned(&data[4..]) as f64)
            } else {
                (
                    f64::from(unsigned(&data[..4]) as i32),
                    f64::from(unsigned(&data[4..]) as i32),
                )
            };
            if denominator == 0.0 {
                return Err(DecodeError::InvalidTag);
            }
            numerator / denominator
        }
        11 => f64::from(f32::from_bits(unsigned(data) as u32)),
        12 => f64::from_bits(unsigned(data)),
        _ => return Err(DecodeError::InvalidTag),
    };
    if !value.is_finite() {
        return Err(DecodeError::InvalidTag);
    }
    Ok(value)
}
