use crate::probe::{DecodeError, DecodeLimits, ProbeResult};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LosslessJpegFrame {
    pub width: u16,
    pub height: u16,
    pub precision: u8,
    pub point_transform: u8,
    pub component_ids: Vec<u8>,
    pub samples: Vec<u16>,
}

#[derive(Clone)]
struct HuffmanTable {
    entries: Vec<(u16, u8, u8)>,
}

struct FrameHeader {
    width: u16,
    height: u16,
    precision: u8,
    component_ids: Vec<u8>,
}

struct ScanHeader {
    order: Vec<usize>,
    tables: Vec<usize>,
    predictor: u8,
    point_transform: u8,
}

pub(crate) fn decode(bytes: &[u8], limits: DecodeLimits) -> ProbeResult<LosslessJpegFrame> {
    if bytes.len() > limits.max_input_bytes {
        return Err(DecodeError::ResourceLimit);
    }
    let mut parser = Parser { bytes, position: 0 };
    parser.marker(0xd8)?;
    let mut frame = None;
    let mut scan = None;
    let mut tables: [Option<HuffmanTable>; 4] = std::array::from_fn(|_| None);
    let mut restart_interval = None;
    loop {
        let marker = parser.next_marker()?;
        match marker {
            0xc3 => {
                if frame.is_some() {
                    return Err(DecodeError::InvalidContainer);
                }
                frame = Some(parser.frame()?);
            }
            0xc4 => parser.tables(&mut tables)?,
            0xdd => {
                if restart_interval.is_some() {
                    return Err(DecodeError::InvalidContainer);
                }
                let segment = parser.segment()?;
                if segment.len() != 2 {
                    return Err(DecodeError::InvalidContainer);
                }
                restart_interval = Some(u16::from_be_bytes([segment[0], segment[1]]));
            }
            0xda => {
                if scan.is_some() {
                    return Err(DecodeError::UnsupportedFeature);
                }
                let header = frame.as_ref().ok_or(DecodeError::InvalidContainer)?;
                scan = Some(parser.scan(header)?);
                break;
            }
            0xe0..=0xef | 0xfe => {
                parser.segment()?;
            }
            0xd9 => return Err(DecodeError::InvalidContainer),
            _ => return Err(DecodeError::UnsupportedFeature),
        }
    }
    let frame = frame.ok_or(DecodeError::InvalidContainer)?;
    let scan = scan.ok_or(DecodeError::InvalidContainer)?;
    decode_entropy(&mut parser, frame, scan, tables, restart_interval, limits)
}

fn decode_entropy(
    parser: &mut Parser<'_>,
    frame: FrameHeader,
    scan: ScanHeader,
    tables: [Option<HuffmanTable>; 4],
    restart_interval: Option<u16>,
    limits: DecodeLimits,
) -> ProbeResult<LosslessJpegFrame> {
    if u32::from(frame.width) > limits.max_width || u32::from(frame.height) > limits.max_height {
        return Err(DecodeError::ResourceLimit);
    }
    let components = frame.component_ids.len();
    let pixels = usize::from(frame.width)
        .checked_mul(usize::from(frame.height))
        .ok_or(DecodeError::ResourceLimit)?;
    if u64::try_from(pixels).map_err(|_| DecodeError::ResourceLimit)? > limits.max_pixels {
        return Err(DecodeError::ResourceLimit);
    }
    let count = pixels
        .checked_mul(components)
        .ok_or(DecodeError::ResourceLimit)?;
    let bounded_count = u64::try_from(count).map_err(|_| DecodeError::ResourceLimit)?;
    if bounded_count > limits.max_frame_samples || bounded_count > limits.max_decoded_samples {
        return Err(DecodeError::ResourceLimit);
    }
    if let Some(interval) = restart_interval {
        if interval != 0 && usize::from(interval) % usize::from(frame.width) != 0 {
            return Err(DecodeError::UnsupportedFeature);
        }
    }
    let effective = frame.precision - scan.point_transform;
    let initial = 1_i32 << (effective - 1);
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(count)
        .map_err(|_| DecodeError::Allocation)?;
    samples.resize(count, 0);
    let mut bits = Bits::new(parser);
    let mut expected_restart = 0_u8;
    let interval = restart_interval
        .map(usize::from)
        .filter(|&value| value != 0);
    for y in 0..usize::from(frame.height) {
        for x in 0..usize::from(frame.width) {
            let mcu = y * usize::from(frame.width) + x;
            let restart_row =
                interval.is_some_and(|value| (y * usize::from(frame.width)) % value == 0);
            for scan_index in 0..components {
                let component = scan.order[scan_index];
                let table = tables[scan.tables[scan_index]]
                    .as_ref()
                    .ok_or(DecodeError::InvalidContainer)?;
                let category = usize::from(bits.symbol(table)?);
                if category > 16 {
                    return Err(DecodeError::InvalidContainer);
                }
                let difference = bits.difference(category)?;
                let index = (mcu * components) + component;
                let prediction = Predictor {
                    samples: &samples,
                    width: usize::from(frame.width),
                    components,
                    x,
                    y,
                    component,
                    kind: scan.predictor,
                    initial,
                    restart_row,
                    point_transform: scan.point_transform,
                }
                .value();
                let value = (prediction + difference).rem_euclid(65_536);
                if value >= (1_i32 << effective) {
                    return Err(DecodeError::InvalidContainer);
                }
                samples[index] = u16::try_from(value << scan.point_transform)
                    .map_err(|_| DecodeError::InvalidContainer)?;
            }
            if interval.is_some_and(|value| (mcu + 1) % value == 0 && mcu + 1 < pixels) {
                bits.finish()?;
                parser.restart(expected_restart)?;
                expected_restart = (expected_restart + 1) & 7;
                bits = Bits::new(parser);
            }
        }
    }
    bits.finish()?;
    parser.marker(0xd9)?;
    if parser.position != parser.bytes.len() {
        return Err(DecodeError::InvalidContainer);
    }
    Ok(LosslessJpegFrame {
        width: frame.width,
        height: frame.height,
        precision: frame.precision,
        point_transform: scan.point_transform,
        component_ids: frame.component_ids,
        samples,
    })
}

struct Predictor<'a> {
    samples: &'a [u16],
    width: usize,
    components: usize,
    x: usize,
    y: usize,
    component: usize,
    kind: u8,
    initial: i32,
    restart_row: bool,
    point_transform: u8,
}

impl Predictor<'_> {
    fn value(self) -> i32 {
        let sample = |row, column| {
            i32::from(
                self.samples[(row * self.width + column) * self.components + self.component]
                    >> self.point_transform,
            )
        };
        if self.x == 0 && (self.y == 0 || self.restart_row) {
            return self.initial;
        }
        if self.y == 0 || self.restart_row {
            return sample(self.y, self.x - 1);
        }
        if self.x == 0 {
            return sample(self.y - 1, self.x);
        }
        let ra = sample(self.y, self.x - 1);
        let rb = sample(self.y - 1, self.x);
        let rc = sample(self.y - 1, self.x - 1);
        match self.kind {
            1 => ra,
            2 => rb,
            3 => rc,
            4 => ra + rb - rc,
            5 => ra + ((rb - rc) >> 1),
            6 => rb + ((ra - rc) >> 1),
            7 => (ra + rb) >> 1,
            _ => self.initial,
        }
    }
}

struct Bits<'a, 'b> {
    parser: &'a mut Parser<'b>,
    value: u32,
    available: u8,
}
impl<'a, 'b> Bits<'a, 'b> {
    fn new(parser: &'a mut Parser<'b>) -> Self {
        Self {
            parser,
            value: 0,
            available: 0,
        }
    }
    fn bit(&mut self) -> ProbeResult<u8> {
        if self.available == 0 {
            self.value = u32::from(self.parser.entropy_byte()?);
            self.available = 8;
        }
        self.available -= 1;
        Ok(((self.value >> self.available) & 1) as u8)
    }
    fn number(&mut self, count: usize) -> ProbeResult<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.bit()?);
        }
        Ok(value)
    }
    fn symbol(&mut self, table: &HuffmanTable) -> ProbeResult<u8> {
        let mut code = 0;
        for length in 1..=16 {
            code = (code << 1) | u16::from(self.bit()?);
            if let Some((_, _, symbol)) = table
                .entries
                .iter()
                .find(|&&(candidate, bits, _)| candidate == code && bits == length)
            {
                return Ok(*symbol);
            }
        }
        Err(DecodeError::InvalidContainer)
    }
    fn difference(&mut self, category: usize) -> ProbeResult<i32> {
        if category == 0 {
            return Ok(0);
        }
        if category == 16 {
            return Ok(-32_768);
        }
        let bits = self.number(category)? as i32;
        Ok(if bits < (1 << (category - 1)) {
            bits - ((1 << category) - 1)
        } else {
            bits
        })
    }
    fn finish(&mut self) -> ProbeResult<()> {
        if self.available != 0
            && self.value & ((1 << self.available) - 1) != (1 << self.available) - 1
        {
            return Err(DecodeError::InvalidContainer);
        }
        self.available = 0;
        Ok(())
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Parser<'a> {
    fn byte(&mut self) -> ProbeResult<u8> {
        let value = *self
            .bytes
            .get(self.position)
            .ok_or(DecodeError::Truncated)?;
        self.position += 1;
        Ok(value)
    }
    fn marker(&mut self, expected: u8) -> ProbeResult<()> {
        if self.marker_code()? != expected {
            return Err(DecodeError::InvalidContainer);
        }
        Ok(())
    }
    fn next_marker(&mut self) -> ProbeResult<u8> {
        self.marker_code()
    }
    fn marker_code(&mut self) -> ProbeResult<u8> {
        if self.byte()? != 0xff {
            return Err(DecodeError::InvalidContainer);
        }
        let mut marker = self.byte()?;
        while marker == 0xff {
            marker = self.byte()?;
        }
        if marker == 0 {
            return Err(DecodeError::InvalidContainer);
        }
        Ok(marker)
    }
    fn segment(&mut self) -> ProbeResult<&'a [u8]> {
        let length = usize::from(u16::from_be_bytes([self.byte()?, self.byte()?]));
        if length < 2 {
            return Err(DecodeError::InvalidContainer);
        }
        let start = self.position;
        let end = start
            .checked_add(length - 2)
            .ok_or(DecodeError::InvalidContainer)?;
        let segment = self.bytes.get(start..end).ok_or(DecodeError::Truncated)?;
        self.position = end;
        Ok(segment)
    }
    fn frame(&mut self) -> ProbeResult<FrameHeader> {
        let segment = self.segment()?;
        if segment.len() < 6 {
            return Err(DecodeError::InvalidContainer);
        }
        let precision = segment[0];
        let height = u16::from_be_bytes([segment[1], segment[2]]);
        let width = u16::from_be_bytes([segment[3], segment[4]]);
        let count = usize::from(segment[5]);
        if !(2..=16).contains(&precision)
            || width == 0
            || height == 0
            || !(1..=4).contains(&count)
            || segment.len() != 6 + count * 3
        {
            return Err(DecodeError::InvalidContainer);
        }
        let mut component_ids = Vec::new();
        component_ids
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::Allocation)?;
        for item in segment[6..].chunks_exact(3) {
            if item[1] != 0x11 || item[2] != 0 || component_ids.contains(&item[0]) {
                return Err(DecodeError::UnsupportedFeature);
            }
            component_ids.push(item[0]);
        }
        Ok(FrameHeader {
            width,
            height,
            precision,
            component_ids,
        })
    }
    fn tables(&mut self, tables: &mut [Option<HuffmanTable>; 4]) -> ProbeResult<()> {
        let segment = self.segment()?;
        let mut position = 0;
        while position < segment.len() {
            let info = *segment.get(position).ok_or(DecodeError::InvalidContainer)?;
            position += 1;
            if info >> 4 != 0 {
                return Err(DecodeError::UnsupportedFeature);
            }
            let id = usize::from(info & 15);
            if id > 3 {
                return Err(DecodeError::InvalidContainer);
            }
            let counts = segment
                .get(position..position + 16)
                .ok_or(DecodeError::Truncated)?;
            position += 16;
            let total: usize = counts.iter().map(|&value| usize::from(value)).sum();
            if total == 0 || total > 256 {
                return Err(DecodeError::InvalidContainer);
            }
            let values = segment
                .get(position..position + total)
                .ok_or(DecodeError::Truncated)?;
            position += total;
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(total)
                .map_err(|_| DecodeError::Allocation)?;
            let mut code = 0_u32;
            let mut value_index = 0;
            for (index, &amount) in counts.iter().enumerate() {
                for _ in 0..amount {
                    if code >= (1_u32 << (index + 1)) {
                        return Err(DecodeError::InvalidContainer);
                    }
                    if code == (1_u32 << (index + 1)) - 1 {
                        return Err(DecodeError::InvalidContainer);
                    }
                    let symbol = values[value_index];
                    if symbol > 16 {
                        return Err(DecodeError::InvalidContainer);
                    }
                    entries.push((code as u16, (index + 1) as u8, symbol));
                    code += 1;
                    value_index += 1;
                }
                code <<= 1;
            }
            tables[id] = Some(HuffmanTable { entries });
        }
        Ok(())
    }
    fn scan(&mut self, frame: &FrameHeader) -> ProbeResult<ScanHeader> {
        let segment = self.segment()?;
        let count = usize::from(*segment.first().ok_or(DecodeError::InvalidContainer)?);
        if count != frame.component_ids.len() || segment.len() != 1 + count * 2 + 3 {
            return Err(DecodeError::UnsupportedFeature);
        }
        let mut order = Vec::new();
        let mut tables = Vec::new();
        order
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::Allocation)?;
        tables
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::Allocation)?;
        for pair in segment[1..1 + count * 2].chunks_exact(2) {
            let component = frame
                .component_ids
                .iter()
                .position(|&id| id == pair[0])
                .ok_or(DecodeError::InvalidContainer)?;
            if order.contains(&component) {
                return Err(DecodeError::InvalidContainer);
            }
            if pair[1] >> 4 > 3 || pair[1] & 15 != 0 {
                return Err(DecodeError::UnsupportedFeature);
            }
            order.push(component);
            tables.push(usize::from(pair[1] >> 4));
        }
        let settings = &segment[1 + count * 2..];
        if !(1..=7).contains(&settings[0])
            || settings[1] != 0
            || settings[2] >> 4 != 0
            || settings[2] & 15 >= frame.precision
        {
            return Err(DecodeError::UnsupportedFeature);
        }
        Ok(ScanHeader {
            order,
            tables,
            predictor: settings[0],
            point_transform: settings[2] & 15,
        })
    }
    fn entropy_byte(&mut self) -> ProbeResult<u8> {
        let value = self.byte()?;
        if value != 0xff {
            return Ok(value);
        }
        if self.byte()? != 0 {
            return Err(DecodeError::InvalidContainer);
        }
        Ok(0xff)
    }
    fn restart(&mut self, expected: u8) -> ProbeResult<()> {
        self.marker(0xd0 + expected)
    }
}

#[cfg(test)]
#[path = "../tests/support/proraw_reference.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PRORAW_SAMPLE and PRORAW_REFERENCE_DIR from tests/proraw_reference.py"]
    fn proraw_tiles_match_independent_samples_before_linearization() {
        let source = std::fs::read(std::env::var_os("PRORAW_SAMPLE").unwrap()).unwrap();
        let directory = std::path::PathBuf::from(std::env::var_os("PRORAW_REFERENCE_DIR").unwrap());
        let provenance = super::reference::verified_manifest(&source, &directory);
        let facts = crate::probe::dng_raw_facts(&source, crate::DecodeLimits::default()).unwrap();
        let tiles = provenance["tiles"].as_array().unwrap();
        let offsets = facts.tile_offsets.unwrap();
        let counts = facts.tile_byte_counts.unwrap();
        assert_eq!(tiles.len(), offsets.len());
        for ((tile, offset), count) in tiles.iter().zip(offsets).zip(counts) {
            assert_eq!(tile["offset"], offset);
            assert_eq!(tile["size"], count);
        }
        let manifest = std::fs::read_to_string(directory.join("tiles.txt")).unwrap();
        assert!(!manifest.trim().is_empty());
        let mut samples = 0;
        for line in manifest.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 5);
            let offset: usize = fields[0].parse().unwrap();
            let length: usize = fields[1].parse().unwrap();
            let frame = super::decode(
                &source[offset..offset + length],
                crate::DecodeLimits::default(),
            )
            .unwrap();
            assert_eq!(frame.width, fields[2].parse::<u16>().unwrap());
            assert_eq!(frame.height, fields[3].parse::<u16>().unwrap());
            let reference = std::fs::read(directory.join(fields[4])).unwrap();
            assert_eq!(reference.len(), frame.samples.len() * 2);
            for (index, (actual, expected)) in frame
                .samples
                .iter()
                .zip(reference.chunks_exact(2))
                .enumerate()
            {
                assert_eq!(
                    *actual,
                    u16::from_le_bytes([expected[0], expected[1]]),
                    "tile {}, sample {index}",
                    fields[4]
                );
            }
            samples += frame.samples.len();
        }
        eprintln!(
            "{} tiles, {samples} pre-linearization samples match the independent decoder",
            manifest.lines().count()
        );
    }
    fn segment(stream: &mut Vec<u8>, marker: u8, body: &[u8]) {
        stream.extend_from_slice(&[0xff, marker]);
        stream.extend_from_slice(&(u16::try_from(body.len() + 2).unwrap()).to_be_bytes());
        stream.extend_from_slice(body);
    }

    fn bits(differences: &[i32]) -> Vec<u8> {
        let mut values = Vec::new();
        let mut current = 0_u8;
        let mut count = 0_u8;
        let push =
            |value: u32, width: u8, values: &mut Vec<u8>, current: &mut u8, count: &mut u8| {
                for shift in (0..width).rev() {
                    *current = (*current << 1) | ((value >> shift) as u8 & 1);
                    *count += 1;
                    if *count == 8 {
                        values.push(*current);
                        *current = 0;
                        *count = 0;
                    }
                }
            };
        for &difference in differences {
            let category = if difference == -32_768 {
                16
            } else if difference == 0 {
                0
            } else {
                32 - difference.unsigned_abs().leading_zeros()
            };
            push(category, 5, &mut values, &mut current, &mut count);
            if category != 0 && category != 16 {
                let payload = if difference < 0 {
                    difference + ((1_i32 << category) - 1)
                } else {
                    difference
                };
                push(
                    payload as u32,
                    category as u8,
                    &mut values,
                    &mut current,
                    &mut count,
                );
            }
        }
        if count != 0 {
            current = (current << (8 - count)) | ((1 << (8 - count)) - 1);
            values.push(current);
        }
        values
            .into_iter()
            .flat_map(|value| {
                if value == 0xff {
                    vec![0xff, 0]
                } else {
                    vec![value]
                }
            })
            .collect()
    }

    #[derive(Clone, Copy)]
    struct Stream<'a> {
        width: u16,
        height: u16,
        precision: u8,
        ids: &'a [u8],
        scan_ids: &'a [u8],
        predictor: u8,
        point_transform: u8,
        restart: Option<u16>,
    }

    fn stream(specification: Stream<'_>, differences: &[i32]) -> Vec<u8> {
        let Stream {
            width,
            height,
            precision,
            ids,
            scan_ids,
            predictor,
            point_transform,
            restart,
        } = specification;
        let mut output = vec![0xff, 0xd8];
        let mut frame = vec![precision];
        frame.extend_from_slice(&height.to_be_bytes());
        frame.extend_from_slice(&width.to_be_bytes());
        frame.push(ids.len() as u8);
        for &id in ids {
            frame.extend_from_slice(&[id, 0x11, 0]);
        }
        segment(&mut output, 0xc3, &frame);
        let mut dht = vec![0, 0, 0, 0, 0, 17];
        dht.extend_from_slice(&[0; 11]);
        dht.extend(0..=16);
        segment(&mut output, 0xc4, &dht);
        if let Some(interval) = restart {
            segment(&mut output, 0xdd, &interval.to_be_bytes());
        }
        let mut sos = vec![scan_ids.len() as u8];
        for &id in scan_ids {
            sos.extend_from_slice(&[id, 0]);
        }
        sos.extend_from_slice(&[predictor, 0, point_transform]);
        segment(&mut output, 0xda, &sos);
        if let Some(interval) = restart {
            let span = usize::from(interval) * ids.len();
            for (index, values) in differences.chunks(span).enumerate() {
                output.extend(bits(values));
                if index + 1 < differences.len().div_ceil(span) {
                    output.extend_from_slice(&[0xff, 0xd0 + index as u8]);
                }
            }
        } else {
            output.extend(bits(differences));
        }
        output.extend_from_slice(&[0xff, 0xd9]);
        output
    }

    fn dht_body(counts: [u8; 16], values: &[u8]) -> Vec<u8> {
        let mut body = vec![0];
        body.extend_from_slice(&counts);
        body.extend_from_slice(values);
        body
    }

    fn insert_before_sos(stream: &mut Vec<u8>, marker: u8, body: &[u8]) {
        let position = stream
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xda])
            .unwrap();
        let mut segment_bytes = Vec::new();
        segment(&mut segment_bytes, marker, body);
        stream.splice(position..position, segment_bytes);
    }

    #[test]
    fn predictors_match_independent_two_by_two_samples() {
        let cases = [
            (1, [0, 1, -1, 1]),
            (2, [0, 1, -1, -1]),
            (3, [0, 1, -1, 0]),
            (4, [0, 1, -1, 0]),
            (5, [0, 1, -1, 1]),
            (6, [0, 1, -1, 0]),
            (7, [0, 1, -1, 0]),
        ];
        for (predictor, differences) in cases {
            let decoded = decode(
                &stream(
                    Stream {
                        width: 2,
                        height: 2,
                        precision: 8,
                        ids: &[7],
                        scan_ids: &[7],
                        predictor,
                        point_transform: 0,
                        restart: None,
                    },
                    &differences,
                ),
                DecodeLimits::default(),
            )
            .unwrap();
            assert_eq!(decoded.samples, [128, 129, 127, 128]);
        }
    }

    #[test]
    fn preserves_frame_component_order_across_sos_permutation() {
        let decoded = decode(
            &stream(
                Stream {
                    width: 1,
                    height: 1,
                    precision: 8,
                    ids: &[4, 9],
                    scan_ids: &[9, 4],
                    predictor: 1,
                    point_transform: 0,
                    restart: None,
                },
                &[2, -1],
            ),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.component_ids, [4, 9]);
        assert_eq!(decoded.samples, [127, 130]);
    }

    #[test]
    fn restores_point_transform_and_signed_categories() {
        let decoded = decode(
            &stream(
                Stream {
                    width: 3,
                    height: 1,
                    precision: 8,
                    ids: &[1],
                    scan_ids: &[1],
                    predictor: 1,
                    point_transform: 2,
                    restart: None,
                },
                &[0, 2, -2],
            ),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.samples, [128, 136, 128]);
        let decoded = decode(
            &stream(
                Stream {
                    width: 1,
                    height: 1,
                    precision: 16,
                    ids: &[1],
                    scan_ids: &[1],
                    predictor: 1,
                    point_transform: 0,
                    restart: None,
                },
                &[-32768],
            ),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.samples, [0]);
        let decoded = decode(
            &stream(
                Stream {
                    width: 2,
                    height: 1,
                    precision: 16,
                    ids: &[1],
                    scan_ids: &[1],
                    predictor: 1,
                    point_transform: 0,
                    restart: None,
                },
                &[-32_768, 0],
            ),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.samples, [0, 0]);
    }

    #[test]
    fn decodes_stuffed_entropy_and_restarts() {
        let decoded = decode(
            &stream(
                Stream {
                    width: 2,
                    height: 2,
                    precision: 8,
                    ids: &[1],
                    scan_ids: &[1],
                    predictor: 7,
                    point_transform: 0,
                    restart: Some(2),
                },
                &[1, 2, 0, 0],
            ),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.samples, [129, 131, 128, 128]);
        let mut wrong_restart = stream(
            Stream {
                width: 2,
                height: 2,
                precision: 8,
                ids: &[1],
                scan_ids: &[1],
                predictor: 7,
                point_transform: 0,
                restart: Some(2),
            },
            &[1, 2, 0, 0],
        );
        let restart = wrong_restart.iter().position(|&byte| byte == 0xd0).unwrap();
        wrong_restart[restart] = 0xd1;
        assert_eq!(
            decode(&wrong_restart, DecodeLimits::default()),
            Err(DecodeError::InvalidContainer)
        );
        let stuffed_stream = stream(
            Stream {
                width: 2,
                height: 1,
                precision: 16,
                ids: &[1],
                scan_ids: &[1],
                predictor: 1,
                point_transform: 0,
                restart: None,
            },
            &[0, 32_767],
        );
        assert!(stuffed_stream.windows(2).any(|bytes| bytes == [0xff, 0]));
        let stuffed = decode(&stuffed_stream, DecodeLimits::default()).unwrap();
        assert_eq!(stuffed.samples, [32768, 65535]);
    }

    #[test]
    fn accepts_marker_fill_and_redefined_huffman_tables() {
        let specification = Stream {
            width: 2,
            height: 2,
            precision: 8,
            ids: &[1],
            scan_ids: &[1],
            predictor: 7,
            point_transform: 0,
            restart: Some(2),
        };
        let mut filled = stream(specification, &[1, 2, 0, 0]);
        filled.insert(2, 0xff);
        let restart = filled.iter().position(|&byte| byte == 0xd0).unwrap();
        filled.insert(restart, 0xff);
        let eoi = filled.len() - 1;
        filled.insert(eoi, 0xff);
        assert_eq!(
            decode(&filled, DecodeLimits::default()).unwrap().samples,
            [129, 131, 128, 128]
        );

        let mut redefined = stream(
            Stream {
                width: 1,
                height: 1,
                precision: 8,
                ids: &[1],
                scan_ids: &[1],
                predictor: 1,
                point_transform: 0,
                restart: None,
            },
            &[0],
        );
        let mut counts = [0; 16];
        counts[4] = 17;
        insert_before_sos(
            &mut redefined,
            0xc4,
            &dht_body(counts, &(0..=16).collect::<Vec<_>>()),
        );
        assert_eq!(
            decode(&redefined, DecodeLimits::default()).unwrap().samples,
            [128]
        );
    }

    #[test]
    fn rejects_invalid_huffman_tables() {
        let specification = Stream {
            width: 1,
            height: 1,
            precision: 8,
            ids: &[1],
            scan_ids: &[1],
            predictor: 1,
            point_transform: 0,
            restart: None,
        };
        let cases = [
            dht_body([0; 16], &[]),
            dht_body([3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &[0, 1, 2]),
            dht_body([2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &[0, 1]),
        ];
        for body in cases {
            let mut bytes = stream(specification, &[0]);
            insert_before_sos(&mut bytes, 0xc4, &body);
            assert_eq!(
                decode(&bytes, DecodeLimits::default()),
                Err(DecodeError::InvalidContainer)
            );
        }
    }

    #[test]
    fn rejects_layouts_limits_and_malformed_data_without_panicking() {
        let valid = stream(
            Stream {
                width: 1,
                height: 1,
                precision: 8,
                ids: &[1],
                scan_ids: &[1],
                predictor: 1,
                point_transform: 0,
                restart: None,
            },
            &[0],
        );
        assert_eq!(
            decode(&valid, DecodeLimits::default().with_max_decoded_samples(0)),
            Err(DecodeError::ResourceLimit)
        );
        assert_eq!(
            decode(&valid, DecodeLimits::default().with_max_frame_samples(0)),
            Err(DecodeError::ResourceLimit)
        );
        let mut sampling = valid.clone();
        let sof = sampling.iter().position(|&value| value == 0xc3).unwrap();
        sampling[sof + 10] = 0x21;
        assert_eq!(
            decode(&sampling, DecodeLimits::default()),
            Err(DecodeError::UnsupportedFeature)
        );
        let multi_scan = [
            &valid[..valid.len() - 2],
            &[0xff, 0xda, 0, 8, 1, 1, 0, 1, 0, 0xff, 0xd9],
        ]
        .concat();
        assert_eq!(
            decode(&multi_scan, DecodeLimits::default()),
            Err(DecodeError::InvalidContainer)
        );
        for end in 0..=valid.len() {
            assert!(
                std::panic::catch_unwind(|| decode(&valid[..end], DecodeLimits::default())).is_ok()
            );
        }
    }
}
