use crate::dng_metadata::allocate;
use crate::{ByteOrder, DecodeError, DecodeLimits, ProbeResult};

pub(crate) struct Frame {
    pub samples: Vec<u16>,
    pub component_ids: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(crate) struct TileSpec {
    pub width: u32,
    pub height: u32,
    pub channels: u16,
    pub bits: u16,
    pub compression: u16,
    pub order: ByteOrder,
}

impl TileSpec {
    fn count(self, limits: DecodeLimits) -> ProbeResult<usize> {
        let pixels = u64::from(self.width) * u64::from(self.height);
        let count = pixels
            .checked_mul(u64::from(self.channels))
            .ok_or(DecodeError::ResourceLimit)?;
        if self.width == 0
            || self.height == 0
            || self.width > limits.max_width
            || self.height > limits.max_height
            || pixels > limits.max_pixels
            || count > limits.max_frame_samples
            || count > limits.max_decoded_samples
        {
            return Err(DecodeError::ResourceLimit);
        }
        usize::try_from(count).map_err(|_| DecodeError::ResourceLimit)
    }
}

pub(crate) fn decode(bytes: &[u8], spec: TileSpec, limits: DecodeLimits) -> ProbeResult<Frame> {
    let count = spec.count(limits)?;
    match spec.compression {
        7 => {
            let frame = crate::lossless_jpeg::decode(bytes, limits)?;
            if u32::from(frame.width) != spec.width
                || u32::from(frame.height) != spec.height
                || u16::from(frame.precision) != spec.bits
                || frame.component_ids.len() != spec.channels as usize
            {
                return Err(DecodeError::UnsupportedFeature);
            }
            Ok(Frame {
                samples: frame.samples,
                component_ids: frame.component_ids,
            })
        }
        1 => Ok(Frame {
            samples: unpack(bytes, spec, count)?,
            component_ids: (1..=spec.channels as u8).collect(),
        }),
        34892 => lossy_jpeg(bytes, spec, limits),
        52546 => jpeg_xl(bytes, spec, limits),
        _ => Err(DecodeError::UnsupportedFeature),
    }
}

fn unpack(bytes: &[u8], spec: TileSpec, count: usize) -> ProbeResult<Vec<u16>> {
    let row_samples = spec.width as usize * spec.channels as usize;
    let row_bytes = row_samples
        .checked_mul(spec.bits as usize)
        .ok_or(DecodeError::ResourceLimit)?
        .div_ceil(8);
    if bytes.len()
        != row_bytes
            .checked_mul(spec.height as usize)
            .ok_or(DecodeError::ResourceLimit)?
    {
        return Err(DecodeError::InvalidContainer);
    }
    let mut output = allocate(count)?;
    for row in bytes.chunks_exact(row_bytes) {
        for sample in 0..row_samples {
            let value = if spec.bits == 16 {
                let pair = [row[sample * 2], row[sample * 2 + 1]];
                match spec.order {
                    ByteOrder::LittleEndian => u16::from_le_bytes(pair),
                    ByteOrder::BigEndian => u16::from_be_bytes(pair),
                }
            } else {
                let first = sample * spec.bits as usize;
                (first..first + spec.bits as usize).fold(0u16, |value, bit| {
                    (value << 1) | u16::from((row[bit / 8] >> (7 - bit % 8)) & 1)
                })
            };
            output.push(value);
        }
    }
    Ok(output)
}

fn lossy_jpeg(bytes: &[u8], spec: TileSpec, limits: DecodeLimits) -> ProbeResult<Frame> {
    if spec.bits != 8 {
        return Err(DecodeError::UnsupportedFeature);
    }
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    decoder.set_max_decoding_buffer_size(limits.max_codec_bytes);
    decoder
        .read_info()
        .map_err(|_| DecodeError::InvalidContainer)?;
    let info = decoder.info().ok_or(DecodeError::InvalidContainer)?;
    let channels = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        _ => return Err(DecodeError::UnsupportedFeature),
    };
    if u32::from(info.width) != spec.width
        || u32::from(info.height) != spec.height
        || channels != spec.channels
    {
        return Err(DecodeError::InvalidGeometry);
    }
    let decoded = decoder
        .decode()
        .map_err(|_| DecodeError::InvalidContainer)?;
    let count = spec.count(limits)?;
    if decoded.len() != count {
        return Err(DecodeError::InvalidContainer);
    }
    let mut samples = allocate(count)?;
    samples.extend(decoded.into_iter().map(u16::from));
    Ok(Frame {
        samples,
        component_ids: (1..=channels as u8).collect(),
    })
}

fn jpeg_xl(bytes: &[u8], spec: TileSpec, limits: DecodeLimits) -> ProbeResult<Frame> {
    use jxl_oxide::{AllocTracker, InitializeResult, JxlImage};
    let mut uninit = JxlImage::builder()
        .alloc_tracker(AllocTracker::with_limit(limits.max_codec_bytes))
        .build_uninit();
    let mut cursor = 0;
    let mut image = loop {
        if cursor == bytes.len() {
            return Err(DecodeError::Truncated);
        }
        let end = (cursor + 4096).min(bytes.len());
        let consumed = uninit
            .feed_bytes(&bytes[cursor..end])
            .map_err(|_| DecodeError::InvalidContainer)?;
        if consumed == 0 {
            return Err(DecodeError::InvalidContainer);
        }
        cursor += consumed;
        match uninit
            .try_init()
            .map_err(|_| DecodeError::InvalidContainer)?
        {
            InitializeResult::NeedMoreData(next) => uninit = next,
            InitializeResult::Initialized(image) => break image,
        }
    };
    let header = image.image_header();
    let metadata = &header.metadata;
    let depth = match metadata.bit_depth {
        jxl_oxide::image::BitDepth::IntegerSample { bits_per_sample } if bits_per_sample <= 16 => {
            bits_per_sample
        }
        _ => return Err(DecodeError::UnsupportedFeature),
    };
    let storage_bits = if depth <= 8 { 8 } else { 16 };
    if header.size.width != spec.width
        || header.size.height != spec.height
        || storage_bits != u32::from(spec.bits)
        || metadata.animation.is_some()
        || !metadata.ec_info.is_empty()
        || metadata.grayscale() != (spec.channels == 1)
    {
        return Err(DecodeError::UnsupportedFeature);
    }
    if metadata.xyb_encoded && metadata.colour_encoding.want_icc() {
        return Err(DecodeError::UnsupportedFeature);
    }
    image.set_render_spot_color(false);
    while cursor < bytes.len() {
        let end = (cursor + 4096).min(bytes.len());
        let consumed = image
            .feed_bytes(&bytes[cursor..end])
            .map_err(|_| DecodeError::InvalidContainer)?;
        if consumed == 0 {
            return Err(DecodeError::InvalidContainer);
        }
        cursor += consumed;
    }
    image
        .finalize()
        .map_err(|_| DecodeError::InvalidContainer)?;
    if !image.is_loading_done() || image.num_loaded_keyframes() != 1 {
        return Err(DecodeError::InvalidContainer);
    }
    let render = image
        .render_frame(0)
        .map_err(|_| DecodeError::UnsupportedFeature)?;
    let mut stream = render.stream_no_alpha();
    if stream.channels() != u32::from(spec.channels) {
        return Err(DecodeError::UnsupportedFeature);
    }
    let mut samples = allocate(spec.count(limits)?)?;
    samples.resize(spec.count(limits)?, 0);
    let width = stream.width();
    let height = stream.height();
    for y in 0..height {
        for x in 0..width {
            let mut pixel = [0u16; 3];
            let count = usize::from(spec.channels);
            if stream.write_to_buffer(&mut pixel[..count]) != count {
                return Err(DecodeError::InvalidContainer);
            }
            let (column, row) = match render.orientation() {
                1 => (x, y),
                2 => (width - x - 1, y),
                3 => (width - x - 1, height - y - 1),
                4 => (x, height - y - 1),
                5 => (y, x),
                6 => (y, width - x - 1),
                7 => (height - y - 1, width - x - 1),
                8 => (height - y - 1, x),
                _ => return Err(DecodeError::InvalidContainer),
            };
            let offset = (row as usize * spec.width as usize + column as usize) * count;
            for channel in 0..count {
                samples[offset + channel] = if spec.bits == 8 {
                    ((u32::from(pixel[channel]) + 128) / 257) as u16
                } else {
                    pixel[channel]
                };
            }
        }
    }
    Ok(Frame {
        samples,
        component_ids: (1..=spec.channels as u8).collect(),
    })
}
