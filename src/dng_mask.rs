use crate::dng_codec::{self, TileSpec};
use crate::dng_metadata::allocate;
use crate::{DecodeError, DecodeLimits, DngDirectory, DngMetadata, DngSemanticMask, ProbeResult};

pub(crate) fn decode(
    bytes: &[u8],
    metadata: &DngMetadata,
    limits: DecodeLimits,
) -> ProbeResult<Vec<DngSemanticMask>> {
    let mut masks = Vec::new();
    let mut total = 0u64;
    for directory in &metadata.directories {
        if scalar(directory, metadata, 254, 0)? != 65540 {
            continue;
        }
        let width = scalar(directory, metadata, 256, 0)?;
        let height = scalar(directory, metadata, 257, 0)?;
        total = total
            .checked_add(u64::from(width) * u64::from(height))
            .ok_or(DecodeError::ResourceLimit)?;
        if width == 0
            || height == 0
            || width > limits.max_width
            || height > limits.max_height
            || u64::from(width) * u64::from(height) > limits.max_pixels
            || total > limits.max_decoded_samples
        {
            return Err(DecodeError::ResourceLimit);
        }
        if scalar(directory, metadata, 262, 0)? != 52527
            || scalar(directory, metadata, 258, 8)? != 8
            || scalar(directory, metadata, 277, 1)? != 1
            || scalar(directory, metadata, 284, 1)? != 1
            || scalar(directory, metadata, 339, 1)? != 1
            || scalar(directory, metadata, 50975, 1)? != 1
            || scalar(directory, metadata, 52547, 1)? != 1
            || scalar(directory, metadata, 266, 1)? != 1
            || scalar(directory, metadata, 317, 1)? != 1
        {
            return Err(DecodeError::UnsupportedFeature);
        }
        let samples = decode_mask(bytes, directory, metadata, limits, [width, height])?;
        let text = |id| {
            directory
                .tag(id)
                .map(|tag| tag.text().map(str::to_owned))
                .transpose()
        };
        let sub_area = directory
            .tag(52536)
            .map(|tag| -> ProbeResult<_> {
                if tag.count != 4 || tag.field_type != 4 {
                    return Err(DecodeError::InvalidTag);
                }
                let values = tag.numbers(metadata.byte_order)?;
                Ok([
                    values[0] as u32,
                    values[1] as u32,
                    values[2] as u32,
                    values[3] as u32,
                ])
            })
            .transpose()?
            .filter(|area| *area != [0; 4]);
        if let Some([top, left, full_width, full_height]) = sub_area {
            if left
                .checked_add(width)
                .is_none_or(|right| right > full_width)
                || top
                    .checked_add(height)
                    .is_none_or(|bottom| bottom > full_height)
            {
                return Err(DecodeError::InvalidGeometry);
            }
        }
        masks.try_reserve(1).map_err(|_| DecodeError::Allocation)?;
        masks.push(DngSemanticMask {
            ifd_offset: directory.offset,
            width,
            height,
            name: text(52526)?,
            instance_id: text(52528)?,
            sub_area,
            samples,
        });
    }
    Ok(masks)
}

fn scalar(
    directory: &DngDirectory,
    metadata: &DngMetadata,
    id: u16,
    default: u32,
) -> ProbeResult<u32> {
    directory.tag(id).map_or(Ok(default), |tag| {
        if tag.count != 1 || !matches!(tag.field_type, 3 | 4) {
            return Err(DecodeError::InvalidTag);
        }
        Ok(tag.number(0, metadata.byte_order)? as u32)
    })
}

fn decode_mask(
    bytes: &[u8],
    directory: &DngDirectory,
    metadata: &DngMetadata,
    limits: DecodeLimits,
    dimensions: [u32; 2],
) -> ProbeResult<Vec<u8>> {
    let [width, height] = dimensions;
    let tiled = directory.tag(324).is_some();
    if tiled && directory.tag(273).is_some() {
        return Err(DecodeError::InvalidTag);
    }
    let tile_width = if tiled {
        scalar(directory, metadata, 322, 0)?
    } else {
        width
    };
    let tile_height = if tiled {
        scalar(directory, metadata, 323, 0)?
    } else {
        scalar(directory, metadata, 278, height)?.min(height)
    };
    if tile_width == 0 || tile_height == 0 {
        return Err(DecodeError::InvalidGeometry);
    }
    let offsets_tag = directory
        .tag(if tiled { 324 } else { 273 })
        .ok_or(DecodeError::InvalidTag)?;
    let counts_tag = directory
        .tag(if tiled { 325 } else { 279 })
        .ok_or(DecodeError::InvalidTag)?;
    let columns = width.div_ceil(tile_width) as usize;
    let tiles = columns
        .checked_mul(height.div_ceil(tile_height) as usize)
        .ok_or(DecodeError::ResourceLimit)?;
    if offsets_tag.count as usize != tiles
        || counts_tag.count as usize != tiles
        || !matches!(offsets_tag.field_type, 3 | 4)
        || !matches!(counts_tag.field_type, 3 | 4)
    {
        return Err(DecodeError::InvalidGeometry);
    }
    let offsets = offsets_tag.numbers(metadata.byte_order)?;
    let counts = counts_tag.numbers(metadata.byte_order)?;
    let mut output = allocate(width as usize * height as usize)?;
    output.resize(width as usize * height as usize, 0);
    for tile in 0..tiles {
        let start = offsets[tile] as usize;
        let end = start
            .checked_add(counts[tile] as usize)
            .ok_or(DecodeError::InvalidOffset)?;
        let payload = bytes.get(start..end).ok_or(DecodeError::Truncated)?;
        let x = tile % columns * tile_width as usize;
        let y = tile / columns * tile_height as usize;
        let block_height = if tiled {
            tile_height
        } else {
            tile_height.min(height - y as u32)
        };
        let mut compression = scalar(directory, metadata, 259, 1)?;
        if compression == 7 && jpeg_frame_marker(payload)? != 0xc3 {
            compression = 34892;
        }
        let frame = dng_codec::decode(
            payload,
            TileSpec {
                width: tile_width,
                height: block_height,
                channels: 1,
                bits: 8,
                compression: u16::try_from(compression).map_err(|_| DecodeError::InvalidTag)?,
                order: metadata.byte_order,
            },
            limits,
        )?;
        for row in 0..(height as usize - y).min(block_height as usize) {
            for column in 0..(width as usize - x).min(tile_width as usize) {
                output[(y + row) * width as usize + x + column] =
                    u8::try_from(frame.samples[row * tile_width as usize + column])
                        .map_err(|_| DecodeError::InvalidContainer)?;
            }
        }
    }
    Ok(output)
}

fn jpeg_frame_marker(bytes: &[u8]) -> ProbeResult<u8> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(DecodeError::InvalidContainer);
    }
    let mut position = 2;
    while position < bytes.len() {
        if bytes[position] != 0xff {
            return Err(DecodeError::InvalidContainer);
        }
        while bytes.get(position) == Some(&0xff) {
            position += 1;
        }
        let marker = *bytes.get(position).ok_or(DecodeError::Truncated)?;
        position += 1;
        if matches!(marker, 0xc0..=0xc3) {
            return Ok(marker);
        }
        let length = bytes
            .get(position..position + 2)
            .ok_or(DecodeError::Truncated)?;
        let length = u16::from_be_bytes(length.try_into().unwrap()) as usize;
        if length < 2 {
            return Err(DecodeError::InvalidContainer);
        }
        position = position
            .checked_add(length)
            .ok_or(DecodeError::InvalidOffset)?;
    }
    Err(DecodeError::Truncated)
}
