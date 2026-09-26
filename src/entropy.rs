// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use crate::{Result, invalid, reader::Reader};

#[derive(Clone, Copy, Default)]
struct Node {
    children: [Option<usize>; 2],
    symbol: Option<u8>,
}

pub(crate) struct Codebook {
    nodes: Vec<Node>,
    prefixes: [(u8, u8); 256],
}

pub(crate) struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
    buffer: u32,
    available: u8,
}

impl<'a> Bits<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            buffer: 0,
            available: 0,
        }
    }

    fn read(&mut self, count: u8) -> Result<u32> {
        self.refill(count);
        if self.available < count {
            return Err(invalid("truncated entropy stream"));
        }
        self.available -= count;
        Ok((self.buffer >> self.available) & ((1 << count) - 1))
    }

    fn prefix(&mut self) -> u8 {
        self.refill(8);
        if self.available >= 8 {
            (self.buffer >> (self.available - 8)) as u8
        } else {
            (self.buffer << (8 - self.available)) as u8
        }
    }

    fn refill(&mut self, count: u8) {
        while self.available < count && self.position < self.bytes.len() {
            self.buffer = (self.buffer << 8) | u32::from(self.bytes[self.position]);
            self.position += 1;
            self.available += 8;
        }
    }
}

impl Codebook {
    pub fn parse(data: Reader<'_>, offset: &mut usize) -> Result<Self> {
        let mut book = Self {
            nodes: vec![Node::default()],
            prefixes: [(0, 0); 256],
        };
        for symbol in 0..=17 {
            let pair = data.bytes(*offset, 2)?;
            *offset += 2;
            if pair[0] == 0 {
                if symbol == 0 {
                    return Err(invalid("empty Huffman table"));
                }
                return Ok(book);
            }
            if symbol > 16 {
                return Err(invalid("too many Huffman symbols"));
            }
            book.insert(pair[0], pair[1], symbol)?;
        }
        Err(invalid("too many Huffman symbols"))
    }

    fn insert(&mut self, length: u8, code: u8, symbol: u8) -> Result<()> {
        if length > 8 || u16::from(code) & (255u16 >> length) != 0 {
            return Err(invalid("invalid Huffman code"));
        }
        let mut index = 0;
        for bit in 0..length {
            if self.nodes[index].symbol.is_some() {
                return Err(invalid("overlapping Huffman codes"));
            }
            let branch = usize::from((code >> (7 - bit)) & 1);
            let child = match self.nodes[index].children[branch] {
                Some(child) => child,
                None => {
                    let child = self.nodes.len();
                    self.nodes.push(Node::default());
                    self.nodes[index].children[branch] = Some(child);
                    child
                }
            };
            index = child;
        }
        if self.nodes[index].symbol.is_some() || self.nodes[index].children != [None; 2] {
            return Err(invalid("overlapping Huffman codes"));
        }
        self.nodes[index].symbol = Some(symbol);
        for prefix in usize::from(code)..usize::from(code) + (1 << (8 - length)) {
            self.prefixes[prefix] = (length, symbol);
        }
        Ok(())
    }

    pub fn difference(&self, bits: &mut Bits<'_>) -> Result<i32> {
        let (length, count) = self.prefixes[usize::from(bits.prefix())];
        if length == 0 {
            return Err(invalid("undefined Huffman code"));
        }
        bits.read(length)?;
        if count == 0 {
            return Ok(0);
        }
        let value = bits.read(count)? as i32;
        Ok(if value < 1 << (count - 1) {
            value - ((1 << count) - 1)
        } else {
            value
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_zero_positive_and_negative_differences() {
        let mut offset = 0;
        let book = Codebook::parse(Reader(&[1, 0, 2, 128, 2, 192, 0, 0]), &mut offset).unwrap();
        let mut bits = Bits::new(&[0b01011001, 0b11111000]);
        let differences: Vec<_> = (0..5)
            .map(|_| book.difference(&mut bits).unwrap())
            .collect();
        assert_eq!(differences, [0, 1, -1, 3, -3]);
    }

    #[test]
    fn rejects_prefix_collision() {
        assert!(Codebook::parse(Reader(&[1, 0, 2, 0, 0, 0]), &mut 0).is_err());
    }

    #[test]
    fn rejects_truncated_stream() {
        let book = Codebook::parse(Reader(&[1, 0, 1, 128, 0, 0]), &mut 0).unwrap();
        assert!(book.difference(&mut Bits::new(&[])).is_err());
    }

    #[test]
    fn decodes_eight_bit_code_without_shift_overflow() {
        let book = Codebook::parse(Reader(&[8, 0, 0, 0]), &mut 0).unwrap();
        assert_eq!(book.difference(&mut Bits::new(&[0])).unwrap(), 0);
    }

    #[test]
    fn supports_sixteen_bit_difference_symbol() {
        let mut table = Vec::new();
        for symbol in 0..=16u8 {
            table.extend([8, symbol]);
        }
        table.extend([0, 0]);
        let book = Codebook::parse(Reader(&table), &mut 0).unwrap();
        assert_eq!(
            book.difference(&mut Bits::new(&[16, 255, 255])).unwrap(),
            65535
        );
    }

    #[test]
    fn lookup_handles_final_short_code_without_reading_past_input() {
        let book = Codebook::parse(Reader(&[1, 0, 0, 0]), &mut 0).unwrap();
        let mut bits = Bits::new(&[0]);
        for _ in 0..8 {
            assert_eq!(book.difference(&mut bits).unwrap(), 0);
        }
        assert!(book.difference(&mut bits).is_err());
    }

    #[test]
    fn buffered_reads_match_bitwise_reference_across_byte_boundaries() {
        let bytes: Vec<_> = (0..128).map(|index| (index * 73 + 19) as u8).collect();
        for count in 1..=16u8 {
            let mut bits = Bits::new(&bytes);
            let mut offset = 0;
            while offset + usize::from(count) <= bytes.len() * 8 {
                let expected =
                    (offset..offset + usize::from(count)).fold(0u32, |value, position| {
                        (value << 1) | u32::from((bytes[position / 8] >> (7 - position % 8)) & 1)
                    });
                assert_eq!(bits.read(count).unwrap(), expected);
                offset += usize::from(count);
            }
            assert!(bits.read(count).is_err());
        }
    }
}
