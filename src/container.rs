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
    version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorFormat {
    /// X3F 3.0 or 3.1 with TRUE format `0x1e`.
    Merrill,
    /// X3F 4.1 with TRUE format `0x23`.
    Quattro,
    /// X3F 4.2 with TRUE format `0x25`.
    SdQuattro,
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
    ///
    /// Plane dimensions preserve the full encoded sensor layout. In particular, a Quattro top
    /// layer can be wider than the nominal image dimensions. No crop or active-area interpretation
    /// is applied.
    pub layers: [Plane; 3],
}

impl<'a> X3f<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if !(44..=MAX_FILE_BYTES).contains(&bytes.len()) {
            return Err(invalid("file size outside supported limits"));
        }
        let file = Reader(bytes);
        file.signature(0, b"FOVb")?;
        let version = file.u32(4)?;
        if !matches!(
            version,
            0x0003_0000 | 0x0003_0001 | 0x0004_0001 | 0x0004_0002
        ) {
            return Err(invalid("unsupported X3F version"));
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
                        if jpeg.is_none() {
                            jpeg = Some(Reader(&section.0[28..]));
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
        Ok(Self {
            raw,
            camf,
            jpeg,
            version,
        })
    }

    pub fn calibration(&self) -> Result<Calibration> {
        Calibration::decode(self.validated_camf()?)
    }

    pub fn sensor_format(&self) -> Result<SensorFormat> {
        match (self.version, self.raw.u32(12)?) {
            (0x0003_0000 | 0x0003_0001, 0x1e) => Ok(SensorFormat::Merrill),
            (0x0004_0001, 0x23) => Ok(SensorFormat::Quattro),
            (0x0004_0002, 0x25) => Ok(SensorFormat::SdQuattro),
            _ => Err(invalid(
                "unsupported X3F version and TRUE format combination",
            )),
        }
    }

    /// Decompresses the CAMF payload without interpreting its entries.
    pub fn camf_bytes(&self) -> Result<Vec<u8>> {
        crate::camf::decompress(self.validated_camf()?)
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
        if self.raw.u32(4)? != 0x0002_0000 {
            return Err(invalid("unsupported RAW section version"));
        }
        let image_dimensions = (self.raw.size(16)?, self.raw.size(20)?);
        let format = self.sensor_format()?;
        let (dimensions, seeds_offset, mut offset, has_quattro_marker) = match format {
            SensorFormat::Merrill => ([image_dimensions; 3], 28, 36, false),
            SensorFormat::Quattro | SensorFormat::SdQuattro => {
                let encoded = [
                    (
                        usize::from(self.raw.u16(28)?),
                        usize::from(self.raw.u16(30)?),
                    ),
                    (
                        usize::from(self.raw.u16(32)?),
                        usize::from(self.raw.u16(34)?),
                    ),
                    (
                        usize::from(self.raw.u16(36)?),
                        usize::from(self.raw.u16(38)?),
                    ),
                ];
                (encoded, 40, 48, true)
            }
        };
        validate_geometry(format, dimensions, image_dimensions)?;
        let book = Codebook::parse(self.raw, &mut offset)?;
        if has_quattro_marker {
            offset = offset
                .checked_add(4)
                .ok_or_else(|| invalid("RAW header offset overflow"))?;
        }
        let lengths = [
            self.raw.size(offset)?,
            self.raw.size(offset + 4)?,
            self.raw.size(offset + 8)?,
        ];
        offset += 12;
        let mut layers = Vec::new();
        for (channel, &(width, height)) in dimensions.iter().enumerate() {
            let data = self.raw.bytes(offset, lengths[channel])?;
            let seed = self.raw.u16(seeds_offset + 2 * channel)?;
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

    fn validated_camf(&self) -> Result<Reader<'a>> {
        let expected_type = match self.sensor_format()? {
            SensorFormat::Merrill => 4,
            SensorFormat::Quattro | SensorFormat::SdQuattro => 5,
        };
        if self.camf.u32(8)? != expected_type {
            return Err(invalid("unsupported CAMF type for sensor format"));
        }
        Ok(self.camf)
    }
}

fn validate_geometry(
    format: SensorFormat,
    encoded: [(usize, usize); 3],
    image: (usize, usize),
) -> Result<()> {
    let expected = match format {
        SensorFormat::Merrill => ([(4928, 3264); 3], (4928, 3264)),
        SensorFormat::Quattro => ([(2944, 1836), (2944, 1836), (6272, 3672)], (5888, 3672)),
        SensorFormat::SdQuattro => ([(2944, 1888), (2944, 1888), (5888, 3776)], (5888, 3776)),
    };
    if (encoded, image) != expected {
        return Err(invalid("unsupported Quattro layer geometry"));
    }
    Ok(())
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
    if width < 2 || height < 2 || count > MAX_PLANE_SAMPLES || count > bytes.len().saturating_mul(8)
    {
        return Err(invalid("plane dimensions outside supported limits"));
    }
    let mut samples = zeroed(count)?;
    let mut bits = Bits::new(bytes);
    let mut row_start = [[i32::from(seed); 2]; 2];
    for y in 0..height {
        let mut previous = row_start[y % 2];
        for x in 0..width {
            let value = previous[x % 2]
                .checked_add(book.difference(&mut bits)?)
                .ok_or_else(|| invalid("sensor predictor overflow"))?;
            samples[y * width + x] = u16::try_from(value)
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
    fn rejects_predictors_outside_the_sensor_sample_range() {
        let book = Codebook::parse(Reader(&[1, 0, 1, 128, 0, 0]), &mut 0).unwrap();
        assert!(decode_plane(&[255], &book, (2, 2), 65535).is_err());
    }

    #[test]
    fn accepts_only_observed_sensor_geometries() {
        assert!(validate_geometry(SensorFormat::Merrill, [(4928, 3264); 3], (4928, 3264)).is_ok());
        assert!(
            validate_geometry(
                SensorFormat::Quattro,
                [(2944, 1836), (2944, 1836), (6272, 3672)],
                (5888, 3672)
            )
            .is_ok()
        );
        assert!(
            validate_geometry(
                SensorFormat::SdQuattro,
                [(2944, 1888), (2944, 1888), (5888, 3776)],
                (5888, 3776)
            )
            .is_ok()
        );
        assert!(
            validate_geometry(
                SensorFormat::Quattro,
                [(2944, 1836), (2944, 1836), (5888, 3672)],
                (5888, 3672)
            )
            .is_err()
        );
    }

    #[test]
    fn truncated_headers_never_panic() {
        for length in 0..128 {
            assert!(X3f::parse(&vec![0; length]).is_err());
        }
    }
}
