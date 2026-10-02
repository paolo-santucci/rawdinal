use std::collections::BTreeMap;

pub struct Fixture {
    pub tags: BTreeMap<u16, (u16, Vec<u8>)>,
    pub blocks: Vec<Vec<u8>>,
}

impl Fixture {
    pub fn rgb(width: u32, height: u32, samples: &[u16]) -> Self {
        let mut fixture = Self {
            tags: BTreeMap::new(),
            blocks: vec![shorts(samples)],
        };
        for (tag, values) in [
            (254, vec![0]),
            (256, vec![width]),
            (257, vec![height]),
            (322, vec![width]),
            (323, vec![height]),
        ] {
            fixture.set(tag, 4, longs(&values));
        }
        for (tag, values) in [
            (258, vec![16; 3]),
            (259, vec![1]),
            (262, vec![34892]),
            (277, vec![3]),
            (284, vec![1]),
        ] {
            fixture.set(tag, 3, shorts(&values));
        }
        fixture.set(271, 2, b"Apple\0".to_vec());
        fixture.set(50706, 1, vec![1, 7, 1, 0]);
        fixture.set(50707, 1, vec![1, 7, 1, 0]);
        fixture
    }

    pub fn set(&mut self, tag: u16, kind: u16, data: Vec<u8>) {
        self.tags.insert(tag, (kind, data));
    }

    pub fn build(self) -> Vec<u8> {
        build(vec![self])
    }
}

pub fn shorts(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}
pub fn longs(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}
pub fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}
pub fn rationals(values: &[(i32, i32)]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|&(a, b)| [a.to_le_bytes(), b.to_le_bytes()].concat())
        .collect()
}

pub fn build(mut fixtures: Vec<Fixture>) -> Vec<u8> {
    for fixture in &mut fixtures {
        let stripped = fixture.tags.contains_key(&278);
        fixture.set(
            if stripped { 273 } else { 324 },
            4,
            longs(&vec![0; fixture.blocks.len()]),
        );
        fixture.set(
            if stripped { 279 } else { 325 },
            4,
            longs(
                &fixture
                    .blocks
                    .iter()
                    .map(|block| block.len() as u32)
                    .collect::<Vec<_>>(),
            ),
        );
    }
    let mut offset = 8;
    let offsets = fixtures
        .iter()
        .map(|fixture| {
            let current = offset;
            offset += 6 + 12 * fixture.tags.len();
            current
        })
        .collect::<Vec<_>>();
    let mut bytes = vec![0; offset];
    bytes[..8].copy_from_slice(b"II*\0\x08\0\0\0");
    for (directory_index, fixture) in fixtures.iter().enumerate() {
        let offset = offsets[directory_index];
        bytes[offset..offset + 2].copy_from_slice(&(fixture.tags.len() as u16).to_le_bytes());
        let next = offset + 2 + 12 * fixture.tags.len();
        bytes[next..next + 4].copy_from_slice(
            &(offsets.get(directory_index + 1).copied().unwrap_or(0) as u32).to_le_bytes(),
        );
        let mut payload_offsets = 0;
        for (index, (&id, (kind, data))) in fixture.tags.iter().enumerate() {
            let entry = offset + 2 + index * 12;
            let unit = match kind {
                1 | 2 | 7 => 1,
                3 => 2,
                4 | 11 => 4,
                5 | 10 | 12 => 8,
                _ => panic!("unsupported fixture type"),
            };
            bytes[entry..entry + 2].copy_from_slice(&id.to_le_bytes());
            bytes[entry + 2..entry + 4].copy_from_slice(&kind.to_le_bytes());
            bytes[entry + 4..entry + 8]
                .copy_from_slice(&((data.len() / unit) as u32).to_le_bytes());
            let value = if data.len() <= 4 {
                bytes[entry + 8..entry + 8 + data.len()].copy_from_slice(data);
                entry + 8
            } else {
                let value = bytes.len();
                bytes[entry + 8..entry + 12].copy_from_slice(&(value as u32).to_le_bytes());
                bytes.extend_from_slice(data);
                value
            };
            if id == 273 || id == 324 {
                payload_offsets = value;
            }
        }
        for (index, block) in fixture.blocks.iter().enumerate() {
            let offset = bytes.len() as u32;
            bytes[payload_offsets + 4 * index..payload_offsets + 4 * index + 4]
                .copy_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(block);
        }
    }
    bytes
}

pub fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
