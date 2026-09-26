// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use crate::{
    Result,
    entropy::{Bits, Codebook},
    invalid,
    reader::Reader,
    zeroed,
};
use std::collections::BTreeMap;

const MAX_CALIBRATION_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
pub struct Matrix {
    pub dimensions: Vec<usize>,
    pub values: Vec<f64>,
}

#[derive(Debug)]
pub enum Entry {
    Text(String),
    Properties(BTreeMap<String, String>),
    Matrix(Matrix),
}

#[derive(Debug)]
pub struct Calibration {
    pub entries: BTreeMap<String, Entry>,
    decoded: Vec<u8>,
}

impl Calibration {
    pub(crate) fn decode(section: Reader<'_>) -> Result<Self> {
        if section.u32(4)? != 0x0002_0000 || section.u32(8)? != 5 {
            return Err(invalid("only type-5 CAMF is currently supported"));
        }
        let size = section.size(12)?;
        if size == 0 || size > MAX_CALIBRATION_BYTES {
            return Err(invalid("invalid CAMF size"));
        }
        section.bytes(0, 28)?;
        let data = Reader(&section.0[28..]);
        let mut offset = 0;
        let book = Codebook::parse(data, &mut offset)?;
        if offset > 28 {
            return Err(invalid("CAMF codebook overlaps stream header"));
        }
        let stream = data.bytes(32, data.size(28)?)?;
        if size > stream.len() * 8 {
            return Err(invalid("CAMF size exceeds entropy capacity"));
        }
        let mut bits = Bits::new(stream);
        let mut decoded: Vec<u8> = zeroed(size)?;
        let mut accumulator = section.u32(16)?;
        for byte in &mut decoded {
            accumulator = accumulator.wrapping_add_signed(book.difference(&mut bits)?);
            *byte = accumulator as u8;
        }
        let entries = Self::parse_entries(&decoded)?;
        Ok(Self { entries, decoded })
    }

    pub fn decoded_bytes(&self) -> &[u8] {
        &self.decoded
    }

    fn parse_entries(bytes: &[u8]) -> Result<BTreeMap<String, Entry>> {
        let mut entries = BTreeMap::new();
        let mut offset = 0;
        while offset < bytes.len() {
            if entries.len() >= 4096 {
                return Err(invalid("too many calibration entries"));
            }
            let remaining = Reader(&bytes[offset..]);
            let size = remaining.size(8)?;
            if size < 20 {
                return Err(invalid("invalid CAMF entry size"));
            }
            let entry = Reader(remaining.bytes(0, size)?);
            let name = entry.string(entry.size(12)?)?.to_owned();
            let value_offset = entry.size(16)?;
            if value_offset < 20 {
                return Err(invalid("invalid CAMF value offset"));
            }
            let value = match entry.bytes(0, 4)? {
                b"CMbT" => {
                    let length = entry.size(value_offset)?;
                    let text = entry.bytes(value_offset + 4, length)?;
                    let text =
                        std::str::from_utf8(text).map_err(|_| invalid("invalid CAMF text"))?;
                    Entry::Text(text.trim_end_matches('\0').to_owned())
                }
                b"CMbP" => Entry::Properties(parse_properties(entry, value_offset)?),
                b"CMbM" => Entry::Matrix(parse_matrix(entry, value_offset)?),
                _ => return Err(invalid("unknown CAMF entry type")),
            };
            if entries.insert(name, value).is_some() {
                return Err(invalid("duplicate CAMF entry"));
            }
            offset += size;
        }
        Ok(entries)
    }

    pub fn matrix(&self, name: &str) -> Result<&Matrix> {
        match self.entries.get(name) {
            Some(Entry::Matrix(matrix)) => Ok(matrix),
            _ => Err(invalid(format!("missing calibration matrix: {name}"))),
        }
    }

    pub fn values<const N: usize>(&self, name: &str) -> Result<[f64; N]> {
        self.matrix(name)?
            .values
            .as_slice()
            .try_into()
            .map_err(|_| invalid(format!("wrong calibration dimensions: {name}")))
    }

    pub fn property(&self, name: &str, key: &str) -> Result<&str> {
        match self.entries.get(name) {
            Some(Entry::Properties(properties)) => properties
                .get(key)
                .map(String::as_str)
                .ok_or_else(|| invalid(format!("missing calibration property: {name}/{key}"))),
            _ => Err(invalid(format!("missing calibration properties: {name}"))),
        }
    }
}

fn parse_properties(entry: Reader<'_>, offset: usize) -> Result<BTreeMap<String, String>> {
    let count = entry.size(offset)?;
    let strings = entry.size(offset + 4)?;
    if count > 4096 {
        return Err(invalid("too many calibration properties"));
    }
    let mut properties = BTreeMap::new();
    for index in 0..count {
        let name_offset = strings
            .checked_add(entry.size(offset + 8 + 8 * index)?)
            .ok_or_else(|| invalid("property offset overflow"))?;
        let value_offset = strings
            .checked_add(entry.size(offset + 12 + 8 * index)?)
            .ok_or_else(|| invalid("property offset overflow"))?;
        let name = entry.string(name_offset)?.to_owned();
        let value = entry.string(value_offset)?.to_owned();
        if properties.insert(name, value).is_some() {
            return Err(invalid("duplicate calibration property"));
        }
    }
    Ok(properties)
}

fn parse_matrix(entry: Reader<'_>, offset: usize) -> Result<Matrix> {
    let kind = entry.u32(offset)?;
    let dimension_count = entry.size(offset + 4)?;
    let data_offset = entry.size(offset + 8)?;
    if !(1..=3).contains(&dimension_count) {
        return Err(invalid("unsupported matrix dimensions"));
    }
    let mut dimensions = Vec::new();
    let mut count = 1usize;
    for index in 0..dimension_count {
        let size = entry.size(offset + 12 + 12 * index)?;
        count = count
            .checked_mul(size)
            .ok_or_else(|| invalid("matrix size overflow"))?;
        dimensions.push(size);
    }
    let element_size = match kind {
        0 | 6 => 2,
        1..=3 => 4,
        5 => 1,
        _ => return Err(invalid("unknown matrix element type")),
    };
    let byte_count = count
        .checked_mul(element_size)
        .ok_or_else(|| invalid("matrix size overflow"))?;
    let data = Reader(entry.bytes(data_offset, byte_count)?);
    let mut values = zeroed(count)?;
    for (index, value) in values.iter_mut().enumerate() {
        let position = index * element_size;
        *value = match kind {
            0 => f64::from(data.u16(position)? as i16),
            1 | 2 => f64::from(data.u32(position)?),
            3 => f64::from(f32::from_bits(data.u32(position)?)),
            5 => f64::from(data.0[position]),
            6 => f64::from(data.u16(position)?),
            _ => unreachable!(),
        };
        if !value.is_finite() {
            return Err(invalid("non-finite calibration value"));
        }
    }
    Ok(Matrix { dimensions, values })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix_entry(kind: u32, values: &[u8], count: u32) -> Vec<u8> {
        let mut entry = vec![0; 48];
        for (offset, value) in [(20, kind), (24, 1), (28, 48), (32, count)] {
            entry[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        entry.extend(values);
        entry
    }

    #[test]
    fn decodes_signed_and_unsigned_sixteen_bit_matrices() {
        for (kind, expected) in [(0, [-1.0, -32768.0]), (6, [65535.0, 32768.0])] {
            let entry = matrix_entry(kind, &[255, 255, 0, 128], 2);
            assert_eq!(parse_matrix(Reader(&entry), 20).unwrap().values, expected);
        }
    }

    #[test]
    fn rejects_non_finite_float_calibration() {
        let entry = matrix_entry(3, &f32::NAN.to_le_bytes(), 1);
        assert!(parse_matrix(Reader(&entry), 20).is_err());
    }

    #[test]
    fn rejects_matrix_payload_outside_entry() {
        let entry = matrix_entry(3, &[0; 4], u32::MAX);
        assert!(parse_matrix(Reader(&entry), 20).is_err());
    }

    #[test]
    fn rejects_bad_property_string_offsets() {
        let mut entry = vec![0; 40];
        entry[20..24].copy_from_slice(&1u32.to_le_bytes());
        entry[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_properties(Reader(&entry), 20).is_err());
    }

    #[test]
    fn rejects_zero_size_entry() {
        assert!(Calibration::parse_entries(&[0; 20]).is_err());
    }

    #[test]
    fn rejects_truncated_camf_header() {
        for size in 0..28 {
            assert!(Calibration::decode(Reader(&vec![0; size])).is_err());
        }
    }
}
