// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use crate::{Result, invalid, reader::Reader};

pub(crate) fn light_source(jpeg: Reader<'_>) -> Result<Option<u16>> {
    match exif(jpeg)? {
        Some(bytes) => Tiff::parse(Reader(bytes))?.light_source(),
        None => Ok(None),
    }
}

pub(crate) fn exif(jpeg: Reader<'_>) -> Result<Option<&[u8]>> {
    jpeg.signature(0, &[255, 216])?;
    let mut offset = 2;
    while offset < jpeg.0.len() {
        jpeg.signature(offset, &[255])?;
        while jpeg.bytes(offset, 1)?[0] == 255 {
            offset += 1;
        }
        let marker = jpeg.bytes(offset, 1)?[0];
        offset += 1;
        if marker == 218 || marker == 217 {
            return Ok(None);
        }
        if marker == 0 || (208..=216).contains(&marker) {
            return Err(invalid("invalid JPEG metadata marker"));
        }
        if marker == 1 {
            continue;
        }
        let length_bytes = jpeg.bytes(offset, 2)?;
        let length = usize::from(u16::from_be_bytes([length_bytes[0], length_bytes[1]]));
        if length < 2 {
            return Err(invalid("invalid JPEG segment length"));
        }
        let payload = jpeg.bytes(offset + 2, length - 2)?;
        if marker == 225 && payload.starts_with(b"Exif\0\0") {
            return Ok(Some(&payload[6..]));
        }
        offset += length;
    }
    Err(invalid("truncated JPEG metadata"))
}

struct Tiff<'a> {
    data: Reader<'a>,
    big_endian: bool,
}

impl<'a> Tiff<'a> {
    fn parse(data: Reader<'a>) -> Result<Self> {
        let big_endian = match data.bytes(0, 2)? {
            b"II" => false,
            b"MM" => true,
            _ => return Err(invalid("invalid EXIF byte order")),
        };
        let tiff = Self { data, big_endian };
        if tiff.u16(2)? != 42 {
            return Err(invalid("invalid TIFF identifier"));
        }
        Ok(tiff)
    }

    fn u16(&self, offset: usize) -> Result<u16> {
        let value = self.data.u16(offset)?;
        Ok(if self.big_endian {
            value.swap_bytes()
        } else {
            value
        })
    }

    fn u32(&self, offset: usize) -> Result<u32> {
        let value = self.data.u32(offset)?;
        Ok(if self.big_endian {
            value.swap_bytes()
        } else {
            value
        })
    }

    fn entry(&self, offset: usize, tag: u16, kind: u16) -> Result<Option<usize>> {
        if offset < 8 {
            return Err(invalid("invalid EXIF directory offset"));
        }
        let count = usize::from(self.u16(offset)?);
        if count > 1024 {
            return Err(invalid("too many EXIF entries"));
        }
        self.data.bytes(offset, 2 + count * 12 + 4)?;
        for index in 0..count {
            let entry = offset + 2 + index * 12;
            if self.u16(entry)? == tag {
                if self.u16(entry + 2)? != kind || self.u32(entry + 4)? != 1 {
                    return Err(invalid("invalid EXIF tag type"));
                }
                return Ok(Some(entry + 8));
            }
        }
        Ok(None)
    }

    fn light_source(&self) -> Result<Option<u16>> {
        let root = usize::try_from(self.u32(4)?).map_err(|_| invalid("EXIF offset overflow"))?;
        let Some(pointer) = self.entry(root, 0x8769, 4)? else {
            return Ok(None);
        };
        let photo =
            usize::try_from(self.u32(pointer)?).map_err(|_| invalid("EXIF offset overflow"))?;
        self.entry(photo, 0x9208, 3)?
            .map(|entry| self.u16(entry))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_standard_light_source_in_both_byte_orders() {
        for big in [false, true] {
            let mut bytes = vec![0; 44];
            bytes[..2].copy_from_slice(if big { b"MM" } else { b"II" });
            for (offset, value) in [
                (2, 42u16),
                (8, 1),
                (10, 0x8769),
                (12, 4),
                (26, 1),
                (28, 0x9208),
                (30, 3),
                (36, 4),
            ] {
                bytes[offset..offset + 2].copy_from_slice(&if big {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                });
            }
            for (offset, value) in [(4, 8u32), (14, 1), (18, 26), (32, 1)] {
                bytes[offset..offset + 4].copy_from_slice(&if big {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                });
            }
            assert_eq!(
                Tiff::parse(Reader(&bytes)).unwrap().light_source().unwrap(),
                Some(4)
            );
        }
    }

    #[test]
    fn rejects_truncated_jpeg_segments() {
        for bytes in [&[255, 216, 255][..], &[255, 216, 255, 225, 0, 40, 0][..]] {
            assert!(light_source(Reader(bytes)).is_err());
        }
    }

    #[test]
    fn rejects_stuffed_bytes_restart_markers_and_nested_soi_in_metadata() {
        for marker in [0, 208, 209, 210, 211, 212, 213, 214, 215, 216] {
            assert!(exif(Reader(&[255, 216, 255, marker, 0, 2, 255, 217])).is_err());
        }
    }

    #[test]
    fn skips_standalone_tem_and_marker_fill_bytes() {
        assert_eq!(
            exif(Reader(&[255, 216, 255, 1, 255, 255, 217])).unwrap(),
            None
        );
    }

    #[test]
    fn rejects_exif_directory_offsets_and_counts_outside_the_payload() {
        let mut bytes = [0; 32];
        bytes[..4].copy_from_slice(b"II*\0");
        for offset in [0, 7, 31, u32::MAX] {
            bytes[4..8].copy_from_slice(&offset.to_le_bytes());
            assert!(Tiff::parse(Reader(&bytes)).unwrap().light_source().is_err());
        }
        bytes[4..8].copy_from_slice(&8u32.to_le_bytes());
        bytes[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(Tiff::parse(Reader(&bytes)).unwrap().light_source().is_err());
    }

    #[test]
    fn jpeg_exif_mutations_and_truncations_never_panic() {
        let bytes = b"\xff\xd8\xff\xe1\x00\x16Exif\0\0II*\0\x08\0\0\0\0\0\0\0\0\0\xff\xd9";
        for size in 0..bytes.len() {
            let _ = light_source(Reader(&bytes[..size]));
        }
        for offset in 0..bytes.len() {
            for value in [0, 1, 127, 255] {
                let mut mutated = bytes.to_vec();
                mutated[offset] = value;
                let _ = light_source(Reader(&mutated));
            }
        }
    }
}
