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
    /// X3F 2.3, 3.0 or 3.1 with RAW type 1 and TRUE format `0x1e`.
    Merrill,
    /// X3F 4.1 with TRUE format `0x23`.
    Quattro,
    /// X3F 4.2 with TRUE format `0x25` (sd Quattro) or `0x27` (sd Quattro H).
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
            0x0002_0003 | 0x0003_0000 | 0x0003_0001 | 0x0004_0001 | 0x0004_0002
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
            (0x0002_0003 | 0x0003_0000 | 0x0003_0001, 0x1e) => Ok(SensorFormat::Merrill),
            (0x0004_0001, 0x23) => Ok(SensorFormat::Quattro),
            (0x0004_0002, 0x25 | 0x27) => Ok(SensorFormat::SdQuattro),
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
        validate_geometry(self.raw.u32(12)?, dimensions, image_dimensions)?;
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
        let mut streams = [&[][..]; 3];
        for channel in 0..3 {
            streams[channel] = self.raw.bytes(offset, lengths[channel])?;
            let (width, height) = dimensions[channel];
            validate_plane_capacity(streams[channel], &book, width, height)?;
            let padded = lengths[channel]
                .checked_add(15)
                .ok_or_else(|| invalid("plane length overflow"))?
                & !15;
            offset = offset
                .checked_add(padded)
                .ok_or_else(|| invalid("plane offset overflow"))?;
        }
        let mut layers = Vec::new();
        for (channel, &(width, height)) in dimensions.iter().enumerate() {
            let seed = self.raw.u16(seeds_offset + 2 * channel)?;
            layers.push(decode_plane(
                streams[channel],
                &book,
                (width, height),
                seed,
            )?);
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
    raw_format: u32,
    encoded: [(usize, usize); 3],
    image: (usize, usize),
) -> Result<()> {
    let expected = match raw_format {
        0x1e => ([(4928, 3264); 3], (4928, 3264)),
        0x23 => ([(2944, 1836), (2944, 1836), (6272, 3672)], (5888, 3672)),
        0x25 => ([(2944, 1888), (2944, 1888), (5888, 3776)], (5888, 3776)),
        0x27 => ([(3328, 2240), (3328, 2240), (6656, 4480)], (6656, 4480)),
        _ => return Err(invalid("unsupported TRUE format")),
    };
    if (encoded, image) != expected {
        return Err(invalid("unsupported sensor layer geometry"));
    }
    Ok(())
}

fn validate_plane_capacity(
    bytes: &[u8],
    book: &Codebook,
    width: usize,
    height: usize,
) -> Result<usize> {
    let count = width
        .checked_mul(height)
        .ok_or_else(|| invalid("plane dimensions overflow"))?;
    if width < 2 || height < 2 || count > MAX_PLANE_SAMPLES {
        return Err(invalid("plane dimensions outside supported limits"));
    }
    book.validate_capacity(bytes.len(), count)?;
    Ok(count)
}

fn decode_plane(
    bytes: &[u8],
    book: &Codebook,
    (width, height): (usize, usize),
    seed: u16,
) -> Result<Plane> {
    let count = validate_plane_capacity(bytes, book, width, height)?;
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
    fn rejects_predictor_underflow() {
        let book = Codebook::parse(Reader(&[1, 0, 1, 128, 0, 0]), &mut 0).unwrap();
        assert!(decode_plane(&[0b1000_0000], &book, (2, 2), 0).is_err());
    }

    #[test]
    fn rejects_impossible_entropy_capacity_before_decoding() {
        let book = Codebook::parse(Reader(&[8, 0, 0, 0]), &mut 0).unwrap();
        assert_eq!(
            decode_plane(&[255], &book, (2, 2), 0)
                .unwrap_err()
                .to_string(),
            "declared output exceeds entropy capacity"
        );
    }

    #[test]
    fn rejects_plane_dimension_overflow() {
        let book = Codebook::parse(Reader(&[1, 0, 0, 0]), &mut 0).unwrap();
        for dimensions in [(0, 2), (1, 2), (usize::MAX, 2), (2, usize::MAX)] {
            assert!(decode_plane(&[0; 8], &book, dimensions, 0).is_err());
        }
    }

    #[test]
    fn rejects_entropy_truncated_inside_the_last_difference() {
        let book = Codebook::parse(Reader(&[1, 0, 1, 128, 0, 0]), &mut 0).unwrap();
        assert_eq!(
            decode_plane(&[1], &book, (4, 2), 100)
                .unwrap_err()
                .to_string(),
            "truncated entropy stream"
        );
    }

    #[test]
    fn entropy_mutations_reach_the_plane_decoder_without_panics() {
        let mut state = 0x8d26_51f3u32;
        let mut next_byte = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        };
        for _ in 0..4096 {
            let mut table = [0u8; 36];
            for symbol in 0..17 {
                table[2 * symbol] = 8;
                table[2 * symbol + 1] = symbol as u8;
            }
            let offset = usize::from(next_byte()) % table.len();
            table[offset] = next_byte();
            if let Ok(book) = Codebook::parse(Reader(&table), &mut 0) {
                let mut stream = [0; 192];
                for byte in &mut stream {
                    *byte = next_byte();
                }
                let _ = decode_plane(&stream, &book, (8, 8), 32768);
            }
        }
    }

    #[test]
    fn accepts_only_observed_sensor_geometries() {
        assert!(validate_geometry(0x1e, [(4928, 3264); 3], (4928, 3264)).is_ok());
        assert!(
            validate_geometry(
                0x23,
                [(2944, 1836), (2944, 1836), (6272, 3672)],
                (5888, 3672)
            )
            .is_ok()
        );
        assert!(
            validate_geometry(
                0x25,
                [(2944, 1888), (2944, 1888), (5888, 3776)],
                (5888, 3776)
            )
            .is_ok()
        );
        assert!(
            validate_geometry(
                0x23,
                [(2944, 1836), (2944, 1836), (5888, 3672)],
                (5888, 3672)
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_quattro_h_geometry_only_for_its_true_format() {
        let layers = [(3328, 2240), (3328, 2240), (6656, 4480)];
        assert!(validate_geometry(0x27, layers, (6656, 4480)).is_ok());
        for format in [0x1e, 0x23, 0x25, 0x99] {
            assert!(validate_geometry(format, layers, (6656, 4480)).is_err());
        }
        for channel in 0..3 {
            let mut invalid = layers;
            invalid[channel].0 += 1;
            assert!(validate_geometry(0x27, invalid, (6656, 4480)).is_err());
            invalid = layers;
            invalid[channel].1 += 1;
            assert!(validate_geometry(0x27, invalid, (6656, 4480)).is_err());
        }
        assert!(validate_geometry(0x27, layers, (6655, 4480)).is_err());
        assert!(validate_geometry(0x27, layers, (6656, 4479)).is_err());
    }

    #[test]
    fn truncated_headers_never_panic() {
        for length in 0..128 {
            assert!(X3f::parse(&vec![0; length]).is_err());
        }
    }
}
