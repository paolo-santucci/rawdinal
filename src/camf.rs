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
const MAX_PROPERTY_STRING_BYTES: usize = 4 * 1024 * 1024;
const MAX_MATRIX_VALUES: usize = MAX_CALIBRATION_BYTES / size_of::<f64>();
const MAX_PROPERTIES: usize = 64 * 1024;

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
        let decoded = decompress(section)?;
        let entries = Self::parse_entries(&decoded)?;
        Ok(Self { entries, decoded })
    }

    pub fn decoded_bytes(&self) -> &[u8] {
        &self.decoded
    }

    fn parse_entries(bytes: &[u8]) -> Result<BTreeMap<String, Entry>> {
        let mut entries = BTreeMap::new();
        let mut offset = 0;
        let mut remaining_property_strings = MAX_PROPERTY_STRING_BYTES;
        let mut remaining_properties = MAX_PROPERTIES;
        let mut remaining_matrix_values = MAX_MATRIX_VALUES;
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
            let name_offset = entry.size(12)?;
            if name_offset < 20 {
                return Err(invalid("invalid CAMF name offset"));
            }
            let name = entry.string(name_offset)?.to_owned();
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
                b"CMbP" => {
                    remaining_properties = remaining_properties
                        .checked_sub(entry.size(value_offset)?)
                        .ok_or_else(|| invalid("too many calibration properties"))?;
                    Entry::Properties(parse_properties(
                        entry,
                        value_offset,
                        &mut remaining_property_strings,
                    )?)
                }
                b"CMbM" => Entry::Matrix(parse_matrix(
                    entry,
                    value_offset,
                    &mut remaining_matrix_values,
                )?),
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

pub(crate) fn decompress(section: Reader<'_>) -> Result<Vec<u8>> {
    if section.u32(4)? != 0x0002_0000 {
        return Err(invalid("unsupported CAMF section version"));
    }
    match section.u32(8)? {
        4 => decompress_type4(section),
        5 => decompress_type5(section),
        _ => Err(invalid("unsupported CAMF type")),
    }
}

fn decompress_type5(section: Reader<'_>) -> Result<Vec<u8>> {
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
    book.validate_capacity(stream.len(), size)?;
    let mut bits = Bits::new(stream);
    let mut decoded: Vec<u8> = zeroed(size)?;
    let mut accumulator = section.u32(16)?;
    for byte in &mut decoded {
        accumulator = accumulator.wrapping_add_signed(book.difference(&mut bits)?);
        *byte = accumulator as u8;
    }
    Ok(decoded)
}

fn decompress_type4(section: Reader<'_>) -> Result<Vec<u8>> {
    let size = section.size(12)?;
    let seed = i32::try_from(section.u32(16)?).map_err(|_| invalid("invalid CAMF decode bias"))?;
    let block_size = section.size(20)?;
    let block_count = section.size(24)?;
    if size == 0 || size > MAX_CALIBRATION_BYTES {
        return Err(invalid("invalid CAMF size"));
    }
    let values = size
        .checked_mul(2)
        .and_then(|size| size.checked_add(2))
        .map(|size| size / 3)
        .ok_or_else(|| invalid("CAMF value count overflow"))?;
    let blocks = block_size
        .checked_mul(block_count)
        .ok_or_else(|| invalid("CAMF block dimensions overflow"))?;
    if block_size < 2 || block_count == 0 || blocks < values || blocks > MAX_CALIBRATION_BYTES * 2 {
        return Err(invalid("CAMF block dimensions outside supported limits"));
    }
    section.bytes(0, 60)?;
    let data = Reader(&section.0[28..]);
    let mut offset = 0;
    let book = Codebook::parse(data, &mut offset)?;
    if offset > 28 {
        return Err(invalid("CAMF codebook overlaps stream header"));
    }
    let stream = data.bytes(32, data.size(28)?)?;
    book.validate_capacity(stream.len(), values)?;
    let mut decoded = zeroed(size)?;
    let mut bits = Bits::new(stream);
    let mut row_start = [[seed; 2]; 2];
    let mut index = 0;
    for row in 0..block_count {
        let mut previous = row_start[row % 2];
        for column in 0..block_size {
            if index == values {
                return Ok(decoded);
            }
            let parity = column % 2;
            let value = previous[parity]
                .checked_add(book.difference(&mut bits)?)
                .ok_or_else(|| invalid("CAMF predictor overflow"))?;
            if !(0..=0x0fff).contains(&value) {
                return Err(invalid("CAMF predictor outside 12-bit range"));
            }
            previous[parity] = value;
            if column < 2 {
                row_start[row % 2][parity] = value;
            }
            pack_camf_value(&mut decoded, index, value as u16);
            index += 1;
        }
    }
    if index == values {
        Ok(decoded)
    } else {
        Err(invalid("CAMF blocks ended before declared output"))
    }
}

fn pack_camf_value(decoded: &mut [u8], index: usize, value: u16) {
    let offset = 3 * (index / 2);
    if index % 2 == 0 {
        decoded[offset] = (value >> 4) as u8;
        if let Some(byte) = decoded.get_mut(offset + 1) {
            *byte = (value as u8 & 0x0f) << 4;
        }
    } else {
        if let Some(byte) = decoded.get_mut(offset + 1) {
            *byte |= (value >> 8) as u8;
        }
        if let Some(byte) = decoded.get_mut(offset + 2) {
            *byte = value as u8;
        }
    }
}

fn parse_properties(
    entry: Reader<'_>,
    offset: usize,
    remaining_strings: &mut usize,
) -> Result<BTreeMap<String, String>> {
    let count = entry.size(offset)?;
    let strings = entry.size(offset + 4)?;
    if count > 4096 {
        return Err(invalid("too many calibration properties"));
    }
    let table_size = 8 + count * 8;
    entry.bytes(offset, table_size)?;
    if strings < offset + table_size || strings > entry.0.len() {
        return Err(invalid("invalid calibration property strings offset"));
    }
    let mut properties = BTreeMap::new();
    for index in 0..count {
        let name_offset = strings
            .checked_add(entry.size(offset + 8 + 8 * index)?)
            .ok_or_else(|| invalid("property offset overflow"))?;
        let value_offset = strings
            .checked_add(entry.size(offset + 12 + 8 * index)?)
            .ok_or_else(|| invalid("property offset overflow"))?;
        let name = property_string(entry, name_offset, &mut *remaining_strings)?.to_owned();
        let value = property_string(entry, value_offset, &mut *remaining_strings)?.to_owned();
        if properties.insert(name, value).is_some() {
            return Err(invalid("duplicate calibration property"));
        }
    }
    Ok(properties)
}

fn property_string<'a>(entry: Reader<'a>, offset: usize, remaining: &mut usize) -> Result<&'a str> {
    let bytes = entry
        .0
        .get(offset..)
        .ok_or_else(|| invalid("invalid string offset"))?;
    let bounded = &bytes[..bytes.len().min(*remaining)];
    let length = bounded
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(|| invalid("calibration property strings exceed supported limits"))?;
    let scanned = length + 1;
    *remaining -= scanned;
    std::str::from_utf8(&bounded[..length]).map_err(|_| invalid("invalid UTF-8 metadata"))
}

fn parse_matrix(entry: Reader<'_>, offset: usize, remaining_values: &mut usize) -> Result<Matrix> {
    let kind = entry.u32(offset)?;
    let dimension_count = entry.size(offset + 4)?;
    let data_offset = entry.size(offset + 8)?;
    if !(1..=3).contains(&dimension_count) {
        return Err(invalid("unsupported matrix dimensions"));
    }
    let descriptor_size = 12 + 12 * dimension_count;
    entry.bytes(offset, descriptor_size)?;
    if data_offset < offset + descriptor_size {
        return Err(invalid("matrix data overlaps descriptor"));
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
    *remaining_values = remaining_values
        .checked_sub(count)
        .ok_or_else(|| invalid("calibration matrices exceed supported limits"))?;
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
            let mut budget = MAX_MATRIX_VALUES;
            assert_eq!(
                parse_matrix(Reader(&entry), 20, &mut budget)
                    .unwrap()
                    .values,
                expected
            );
        }
    }

    #[test]
    fn decodes_high_bit_unsigned_and_fractional_matrix_values() {
        for (kind, bytes, expected) in [
            (
                1,
                &[255, 255, 255, 255, 0, 0, 0, 128][..],
                [4294967295.0, 2147483648.0],
            ),
            (
                2,
                &[255, 255, 255, 255, 0, 0, 0, 128][..],
                [4294967295.0, 2147483648.0],
            ),
            (3, &[0, 0, 192, 63, 0, 0, 16, 192][..], [1.5, -2.25]),
            (5, &[128, 255][..], [128.0, 255.0]),
        ] {
            let entry = matrix_entry(kind, bytes, 2);
            let mut budget = MAX_MATRIX_VALUES;
            let decoded = parse_matrix(Reader(&entry), 20, &mut budget).unwrap();
            assert_eq!(decoded.values, expected, "matrix type {kind}");
        }
    }

    #[test]
    fn preserves_matrix_descriptor_dimensions_and_payload_order() {
        let mut entry = vec![0; 56];
        for (offset, value) in [(20, 5u32), (24, 2), (28, 56), (32, 2), (44, 3), (52, 1)] {
            entry[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        entry.extend([11, 12, 13, 21, 22, 23]);
        let mut budget = MAX_MATRIX_VALUES;
        let matrix = parse_matrix(Reader(&entry), 20, &mut budget).unwrap();
        assert_eq!(matrix.dimensions, [2, 3]);
        assert_eq!(matrix.values, [11.0, 12.0, 13.0, 21.0, 22.0, 23.0]);
    }

    #[test]
    fn rejects_non_finite_float_calibration() {
        let entry = matrix_entry(3, &f32::NAN.to_le_bytes(), 1);
        let mut budget = MAX_MATRIX_VALUES;
        assert!(parse_matrix(Reader(&entry), 20, &mut budget).is_err());
    }

    #[test]
    fn rejects_matrix_payload_outside_entry() {
        let entry = matrix_entry(3, &[0; 4], u32::MAX);
        let mut budget = MAX_MATRIX_VALUES;
        assert!(parse_matrix(Reader(&entry), 20, &mut budget).is_err());
    }

    #[test]
    fn rejects_matrix_payload_aliasing_its_descriptor() {
        let mut entry = matrix_entry(5, &[42], 1);
        entry[28..32].copy_from_slice(&32u32.to_le_bytes());
        let mut budget = MAX_MATRIX_VALUES;
        assert!(parse_matrix(Reader(&entry), 20, &mut budget).is_err());
    }

    #[test]
    fn rejects_matrix_expansion_over_the_calibration_budget() {
        let entry = matrix_entry(5, &vec![0; 4 * 1024 * 1024 + 1], 4 * 1024 * 1024 + 1);
        let mut budget = MAX_MATRIX_VALUES;
        assert!(parse_matrix(Reader(&entry), 20, &mut budget).is_err());
    }

    fn named_matrix_entry(name: &str, count: usize) -> Vec<u8> {
        let mut entry = matrix_entry(5, &vec![0; count], count as u32);
        let name_offset = entry.len() as u32;
        entry.extend(name.bytes());
        entry.push(0);
        let size = entry.len() as u32;
        entry[..4].copy_from_slice(b"CMbM");
        entry[8..12].copy_from_slice(&size.to_le_bytes());
        entry[12..16].copy_from_slice(&name_offset.to_le_bytes());
        entry[16..20].copy_from_slice(&20u32.to_le_bytes());
        entry
    }

    #[test]
    fn rejects_matrices_that_exceed_the_aggregate_expansion_budget() {
        let mut bytes = named_matrix_entry("First", MAX_MATRIX_VALUES / 2 + 1);
        bytes.extend(named_matrix_entry("Second", MAX_MATRIX_VALUES / 2 + 1));
        assert_eq!(
            Calibration::parse_entries(&bytes).unwrap_err().to_string(),
            "calibration matrices exceed supported limits"
        );
    }

    #[test]
    fn rejects_entry_names_pointing_into_the_header() {
        let mut entry = named_matrix_entry("Name", 1);
        entry[12..16].copy_from_slice(&4u32.to_le_bytes());
        assert!(Calibration::parse_entries(&entry).is_err());
    }

    #[test]
    fn rejects_properties_over_the_aggregate_entry_budget() {
        let mut bytes = Vec::new();
        for index in 0..17 {
            let strings = 36 + 4096 * 8;
            let mut entry = vec![0; strings];
            entry[..4].copy_from_slice(b"CMbP");
            entry[12..16].copy_from_slice(&20u32.to_le_bytes());
            entry[16..20].copy_from_slice(&28u32.to_le_bytes());
            entry[20..24].copy_from_slice(format!("{index:04x}").as_bytes());
            entry[28..32].copy_from_slice(&4096u32.to_le_bytes());
            entry[32..36].copy_from_slice(&(strings as u32).to_le_bytes());
            for property in 0..4096 {
                let slot = 36 + property * 8;
                let offset = (entry.len() - strings) as u32;
                entry[slot..slot + 4].copy_from_slice(&offset.to_le_bytes());
                entry[slot + 4..slot + 8].copy_from_slice(&(offset + 4).to_le_bytes());
                entry.extend(format!("{property:04x}\0").bytes());
            }
            let size = entry.len() as u32;
            entry[8..12].copy_from_slice(&size.to_le_bytes());
            bytes.extend(entry);
        }
        assert_eq!(
            Calibration::parse_entries(&bytes).unwrap_err().to_string(),
            "too many calibration properties"
        );
    }

    #[test]
    fn matrix_metadata_mutations_and_truncations_never_panic() {
        let original = named_matrix_entry("Matrix", 16);
        for size in 0..original.len() {
            assert!(Calibration::parse_entries(&original[..size]).is_err() || size == 0);
        }
        for offset in 0..original.len() {
            for value in [0, 1, 127, 255] {
                let mut bytes = original.clone();
                bytes[offset] = value;
                let _ = Calibration::parse_entries(&bytes);
            }
        }
    }

    #[test]
    fn rejects_property_strings_aliasing_the_offset_table() {
        let mut entry = vec![0; 40];
        entry[20..24].copy_from_slice(&1u32.to_le_bytes());
        entry[24..28].copy_from_slice(&28u32.to_le_bytes());
        let mut budget = MAX_PROPERTY_STRING_BYTES;
        assert!(parse_properties(Reader(&entry), 20, &mut budget).is_err());
    }

    #[test]
    fn rejects_empty_property_table_with_out_of_bounds_strings() {
        let mut entry = vec![0; 28];
        entry[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut budget = MAX_PROPERTY_STRING_BYTES;
        assert!(parse_properties(Reader(&entry), 20, &mut budget).is_err());
    }

    #[test]
    fn rejects_bad_property_string_offsets() {
        let mut entry = vec![0; 40];
        entry[20..24].copy_from_slice(&1u32.to_le_bytes());
        entry[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut budget = MAX_PROPERTY_STRING_BYTES;
        assert!(parse_properties(Reader(&entry), 20, &mut budget).is_err());
    }

    #[test]
    fn resolves_properties_relative_to_the_declared_string_area() {
        let mut entry = vec![0; 44];
        entry[20..24].copy_from_slice(&1u32.to_le_bytes());
        entry[24..28].copy_from_slice(&44u32.to_le_bytes());
        entry[32..36].copy_from_slice(&4u32.to_le_bytes());
        entry.extend(b"key\0value\0");
        let mut budget = MAX_PROPERTY_STRING_BYTES;
        let properties = parse_properties(Reader(&entry), 20, &mut budget).unwrap();
        assert_eq!(properties, BTreeMap::from([("key".into(), "value".into())]));
        assert_eq!(budget, MAX_PROPERTY_STRING_BYTES - 10);
    }

    #[test]
    fn rejects_aliased_property_strings_over_the_aggregate_budget() {
        let mut entry = vec![0; 40 + 1024 * 1024 + 1];
        entry[20..24].copy_from_slice(&2u32.to_le_bytes());
        entry[24..28].copy_from_slice(&40u32.to_le_bytes());
        let end = entry.len() - 1;
        entry[40..end].fill(b'a');
        let mut budget = MAX_PROPERTY_STRING_BYTES;
        assert!(parse_properties(Reader(&entry), 20, &mut budget).is_err());
    }

    fn property_entry(name: &[u8], string_bytes: usize) -> Vec<u8> {
        let value_offset = 20 + name.len() + 1;
        let strings = value_offset + 16;
        let mut entry = vec![0; strings + string_bytes + 1];
        entry[..4].copy_from_slice(b"CMbP");
        entry[12..16].copy_from_slice(&20u32.to_le_bytes());
        entry[16..20].copy_from_slice(&(value_offset as u32).to_le_bytes());
        entry[20..20 + name.len()].copy_from_slice(name);
        entry[value_offset..value_offset + 4].copy_from_slice(&1u32.to_le_bytes());
        entry[value_offset + 4..value_offset + 8].copy_from_slice(&(strings as u32).to_le_bytes());
        entry[strings..strings + string_bytes].fill(b'a');
        let size = entry.len() as u32;
        entry[8..12].copy_from_slice(&size.to_le_bytes());
        entry
    }

    #[test]
    fn rejects_property_strings_that_exceed_the_global_calibration_budget() {
        let mut entries = property_entry(b"First", 1_500_000);
        entries.extend(property_entry(b"Second", 1_500_000));
        assert!(Calibration::parse_entries(&entries).is_err());
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

    fn type4_section(size: usize) -> Vec<u8> {
        let mut section = vec![0; 60];
        section[..4].copy_from_slice(b"SECc");
        section[4..8].copy_from_slice(&0x20000u32.to_le_bytes());
        section[8..12].copy_from_slice(&4u32.to_le_bytes());
        section[12..16].copy_from_slice(&(size as u32).to_le_bytes());
        section[16..20].copy_from_slice(&0x123u32.to_le_bytes());
        section[20..24].copy_from_slice(&3u32.to_le_bytes());
        section[24..28].copy_from_slice(&1u32.to_le_bytes());
        section[28..32].copy_from_slice(&[1, 0, 0, 0]);
        section[56..60].copy_from_slice(&1u32.to_le_bytes());
        section.push(0);
        section
    }

    #[test]
    fn decodes_type4_twelve_bit_packing() {
        assert_eq!(
            decompress(Reader(&type4_section(3))).unwrap(),
            [0x12, 0x31, 0x23]
        );
    }

    #[test]
    fn decodes_type4_mixed_values_across_predictor_rows() {
        let mut section = type4_section(24);
        section.truncate(60);
        section[16..20].copy_from_slice(&0x100u32.to_le_bytes());
        section[20..24].copy_from_slice(&4u32.to_le_bytes());
        section[24..28].copy_from_slice(&4u32.to_le_bytes());
        section[28..38].copy_from_slice(&[2, 0, 2, 64, 2, 128, 2, 192, 0, 0]);
        section[56..60].copy_from_slice(&8u32.to_le_bytes());
        section.extend([0x75, 0x6b, 0x93, 0x0e, 0x9a, 0x37, 0x6e, 0x40]);
        assert_eq!(
            decompress(Reader(&section)).unwrap(),
            [
                0x10, 0x11, 0x02, 0x10, 0x41, 0x01, 0x10, 0x40, 0xfe, 0x10, 0x10, 0xff, 0x10, 0x31,
                0x03, 0x10, 0x21, 0x03, 0x10, 0x01, 0x01, 0x10, 0x10, 0xff,
            ]
        );
    }

    #[test]
    fn decodes_type5_positive_and_negative_wraparound() {
        let mut section = type4_section(8);
        section.truncate(60);
        section[8..12].copy_from_slice(&5u32.to_le_bytes());
        section[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        section[28..34].copy_from_slice(&[1, 0, 1, 128, 0, 0]);
        section[56..60].copy_from_slice(&2u32.to_le_bytes());
        section.extend([0xfa, 0xbc]);
        assert_eq!(
            decompress(Reader(&section)).unwrap(),
            [0, 1, 0, 255, 254, 255, 0, 0]
        );
    }

    #[test]
    fn type4_packs_partial_final_values_and_accepts_exact_grids() {
        assert_eq!(decompress(Reader(&type4_section(1))).unwrap(), [0x12]);
        assert_eq!(decompress(Reader(&type4_section(2))).unwrap(), [0x12, 0x31]);
        assert_eq!(
            decompress(Reader(&type4_section(3))).unwrap(),
            [0x12, 0x31, 0x23]
        );
        assert_eq!(
            decompress(Reader(&type4_section(4))).unwrap(),
            [0x12, 0x31, 0x23, 0x12]
        );
    }

    #[test]
    fn type4_rejects_truncated_stream_and_invalid_blocks() {
        let mut truncated = type4_section(3);
        truncated.pop();
        assert!(decompress(Reader(&truncated)).is_err());
        let mut blocks = type4_section(3);
        blocks[20..24].copy_from_slice(&1u32.to_le_bytes());
        assert!(decompress(Reader(&blocks)).is_err());
    }

    #[test]
    fn type4_rejects_predictor_overflow_and_invalid_entropy_capacity() {
        let mut section = type4_section(3);
        section[16..20].copy_from_slice(&4096u32.to_le_bytes());
        assert!(decompress(Reader(&section)).is_err());
        section[16..20].copy_from_slice(&0u32.to_le_bytes());
        section[28] = 8;
        assert_eq!(
            decompress(Reader(&section)).unwrap_err().to_string(),
            "declared output exceeds entropy capacity"
        );
    }

    #[test]
    fn type4_header_mutations_and_all_truncations_never_panic() {
        let original = type4_section(4);
        for size in 0..original.len() {
            assert!(decompress(Reader(&original[..size])).is_err());
        }
        for offset in 4..original.len() {
            for value in [0, 1, 127, 255] {
                let mut bytes = original.clone();
                bytes[offset] = value;
                let _ = decompress(Reader(&bytes));
            }
        }
    }
}
