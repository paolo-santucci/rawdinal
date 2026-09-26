// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use crate::{Result, invalid};

#[derive(Clone, Copy)]
pub(crate) struct Reader<'a>(pub &'a [u8]);

impl<'a> Reader<'a> {
    pub fn bytes(self, offset: usize, count: usize) -> Result<&'a [u8]> {
        let end = offset
            .checked_add(count)
            .ok_or_else(|| invalid("offset overflow"))?;
        self.0
            .get(offset..end)
            .ok_or_else(|| invalid("truncated data"))
    }

    pub fn u16(self, offset: usize) -> Result<u16> {
        let bytes = self.bytes(offset, 2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn u32(self, offset: usize) -> Result<u32> {
        let bytes = self.bytes(offset, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn size(self, offset: usize) -> Result<usize> {
        usize::try_from(self.u32(offset)?).map_err(|_| invalid("offset exceeds address space"))
    }

    pub fn string(self, offset: usize) -> Result<&'a str> {
        let bytes = self
            .0
            .get(offset..)
            .ok_or_else(|| invalid("invalid string offset"))?;
        let length = bytes
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| invalid("unterminated string"))?;
        std::str::from_utf8(&bytes[..length]).map_err(|_| invalid("invalid UTF-8 metadata"))
    }

    pub fn signature(self, offset: usize, signature: &[u8]) -> Result<()> {
        if self.bytes(offset, signature.len())? != signature {
            return Err(invalid("invalid section signature"));
        }
        Ok(())
    }
}
