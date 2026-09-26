// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use crate::{
    Calibration, Result,
    entropy::{Bits, Codebook},
    invalid,
    reader::Reader,
    zeroed,
};

const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const MAX_PLANE_SAMPLES: usize = 32 * 1024 * 1024;

pub struct X3f<'a> {
    raw: Reader<'a>,
    camf: Reader<'a>,
    jpeg: Option<Reader<'a>>,
}

#[derive(Debug)]
pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<u16>,
}

#[derive(Debug)]
pub struct SensorImage {
    /// Physical bottom, middle and top layers, in that order. These are not RGB.
    pub layers: [Plane; 3],
}

impl<'a> X3f<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if !(44..=MAX_FILE_BYTES).contains(&bytes.len()) {
            return Err(invalid("file size outside supported limits"));
        }
        let file = Reader(bytes);
        file.signature(0, b"FOVb")?;
        if file.u32(4)? != 0x0004_0002 {
            return Err(invalid("only X3F 4.2 is currently supported"));
        }
        let directory_offset = file.size(bytes.len() - 4)?;
        if directory_offset < 40 || directory_offset >= bytes.len() - 4 {
            return Err(invalid("invalid directory location"));
        }
        let directory = Reader(file.bytes(directory_offset, bytes.len() - 4 - directory_offset)?);
        directory.signature(0, b"SECd")?;
        if directory.u32(4)? != 0x0002_0000 {
            return Err(invalid("unsupported directory version"));
        }
        let count = directory.size(8)?;
        if count > 64 || directory.0.len() != 12 + count * 12 {
            return Err(invalid("invalid directory size"));
        }
        let mut ranges = Vec::new();
        let (mut raw, mut camf) = (None, None);
        let mut jpeg = None;
        for index in 0..count {
            let entry = 12 + 12 * index;
            let offset = directory.size(entry)?;
            let length = directory.size(entry + 4)?;
            let end = offset
                .checked_add(length)
                .ok_or_else(|| invalid("section overflow"))?;
            if offset < 40
                || end > directory_offset
                || length < 8
                || ranges
                    .iter()
                    .any(|&(start, stop)| offset < stop && end > start)
            {
                return Err(invalid("invalid or overlapping sections"));
            }
            ranges.push((offset, end));
            let section = Reader(file.bytes(offset, length)?);
            match directory.bytes(entry + 8, 4)? {
                b"IMA2" => {
                    section.signature(0, b"SECi")?;
                    if section.u32(8)? == 1 && raw.replace(section).is_some() {
                        return Err(invalid("multiple RAW sections"));
                    }
                    if section.u32(8)? == 2 && section.u32(12)? == 0x12 {
                        section.bytes(0, 28)?;
                        if jpeg.replace(Reader(&section.0[28..])).is_some() {
                            return Err(invalid("multiple JPEG sections"));
                        }
                    }
                }
                b"CAMF" => {
                    section.signature(0, b"SECc")?;
                    if camf.replace(section).is_some() {
                        return Err(invalid("multiple CAMF sections"));
                    }
                }
                _ => {}
            }
        }
        let raw = raw.ok_or_else(|| invalid("missing RAW section"))?;
        let camf = camf.ok_or_else(|| invalid("missing CAMF section"))?;
        Ok(Self { raw, camf, jpeg })
    }

    pub fn calibration(&self) -> Result<Calibration> {
        Calibration::decode(self.camf)
    }

    /// Borrowed TIFF/EXIF bytes from the JPEG preview; no preview pixels are decoded.
    pub fn exif(&self) -> Result<Option<&'a [u8]>> {
        self.jpeg
            .map(crate::photo::exif)
            .transpose()
            .map(Option::flatten)
    }

    /// Borrowed camera JPEG, for metadata and thumbnails only.
    pub fn preview(&self) -> Option<&'a [u8]> {
        self.jpeg.map(|jpeg| jpeg.0)
    }

    /// Prefers the JPEG EXIF light source over inconsistent sd Quattro CAMF settings.
    pub fn white_balance(&self, calibration: &Calibration) -> Result<&'static str> {
        let source = self
            .jpeg
            .map(crate::photo::light_source)
            .transpose()?
            .flatten();
        if let Some(preset) = source.and_then(|source| match source {
            1 | 9 => Some("Sunlight"),
            2 => Some("Fluorescent"),
            3 => Some("Incandescent"),
            4 => Some("Flash"),
            10 => Some("Overcast"),
            11 => Some("Shade"),
            _ => None,
        }) {
            return Ok(preset);
        }
        match calibration.values::<1>("WhiteBalance")? {
            [1.0] => Ok("Auto"),
            [2.0] => Ok("Sunlight"),
            [3.0] => Ok("Shade"),
            [4.0] => Ok("Overcast"),
            [5.0] => Ok("Incandescent"),
            [6.0] => Ok("Fluorescent"),
            [7.0] => Ok("Flash"),
            [8.0] => Ok("Custom"),
            [12.0] => Ok("AutoLSP"),
            _ => Err(invalid("unsupported white-balance setting")),
        }
    }

    pub fn decode(&self) -> Result<SensorImage> {
        if self.raw.u32(4)? != 0x0002_0000 || self.raw.u32(12)? != 0x25 {
            return Err(invalid(
                "only sd Quattro TRUE format 0x25 is currently supported",
            ));
        }
        let dimensions: Vec<_> = (0..3)
            .map(|channel| {
                Ok((
                    usize::from(self.raw.u16(28 + 4 * channel)?),
                    usize::from(self.raw.u16(30 + 4 * channel)?),
                ))
            })
            .collect::<Result<_>>()?;
        if dimensions[0] != dimensions[1]
            || dimensions[2] != (2 * dimensions[0].0, 2 * dimensions[0].1)
            || dimensions[2] != (self.raw.size(16)?, self.raw.size(20)?)
        {
            return Err(invalid("unsupported Quattro layer geometry"));
        }
        let mut offset = 48;
        let book = Codebook::parse(self.raw, &mut offset)?;
        offset += 4;
        let lengths = [
            self.raw.size(offset)?,
            self.raw.size(offset + 4)?,
            self.raw.size(offset + 8)?,
        ];
        offset += 12;
        let mut layers = Vec::new();
        for (channel, &(width, height)) in dimensions.iter().enumerate() {
            let data = self.raw.bytes(offset, lengths[channel])?;
            let seed = self.raw.u16(40 + 2 * channel)?;
            layers.push(decode_plane(data, &book, (width, height), seed)?);
            offset = offset
                .checked_add((lengths[channel] + 15) & !15)
                .ok_or_else(|| invalid("plane offset overflow"))?;
        }
        let layers = layers
            .try_into()
            .map_err(|_| invalid("invalid layer count"))?;
        Ok(SensorImage { layers })
    }
}

fn decode_plane(
    bytes: &[u8],
    book: &Codebook,
    (width, height): (usize, usize),
    seed: u16,
) -> Result<Plane> {
    let count = width
        .checked_mul(height)
        .ok_or_else(|| invalid("plane dimensions overflow"))?;
    if width < 2 || height < 2 || count > MAX_PLANE_SAMPLES || count > bytes.len() * 8 {
        return Err(invalid("plane dimensions outside supported limits"));
    }
    let mut samples = zeroed(count)?;
    let mut bits = Bits::new(bytes);
    let mut row_start = [[i32::from(seed); 2]; 2];
    for (y, row) in samples.chunks_exact_mut(width).enumerate() {
        let mut previous = row_start[y % 2];
        for (x, sample) in row.iter_mut().enumerate() {
            let value = previous[x % 2] + book.difference(&mut bits)?;
            *sample = u16::try_from(value)
                .map_err(|_| invalid("sensor predictor outside 16-bit range"))?;
            previous[x % 2] = value;
            if x < 2 {
                row_start[y % 2][x] = value;
            }
        }
    }
    Ok(Plane {
        width,
        height,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_independent_even_odd_predictors() {
        let book = Codebook::parse(Reader(&[1, 0, 1, 128, 0, 0]), &mut 0).unwrap();
        let plane = decode_plane(&[255; 4], &book, (4, 4), 100).unwrap();
        assert_eq!(
            plane.samples,
            [
                101, 101, 102, 102, 101, 101, 102, 102, 102, 102, 103, 103, 102, 102, 103, 103
            ]
        );
    }

    #[test]
    fn rejects_predictor_overflow() {
        let book = Codebook::parse(Reader(&[1, 0, 1, 128, 0, 0]), &mut 0).unwrap();
        assert!(decode_plane(&[255; 2], &book, (2, 2), 65535).is_err());
    }

    #[test]
    fn truncated_headers_never_panic() {
        for length in 0..128 {
            assert!(X3f::parse(&vec![0; length]).is_err());
        }
    }
}
