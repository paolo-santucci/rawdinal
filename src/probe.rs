use crate::dng_metadata::{DirectoryRange, TagRange, number};
use std::fmt;

const TIFF_MAGIC: u16 = 42;
const BIG_TIFF_MAGIC: u16 = 43;
const TAG_NEW_SUBFILE_TYPE: u16 = 254;
const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_MAKE: u16 = 271;
const TAG_MODEL: u16 = 272;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_SUB_IFDS: u16 = 330;
const TAG_SAMPLE_FORMAT: u16 = 339;
const TAG_DNG_VERSION: u16 = 50_706;
const TAG_DNG_BACKWARD_VERSION: u16 = 50_707;
const TAG_LINEARIZATION_TABLE: u16 = 50_712;
const TAG_BLACK_LEVEL: u16 = 50_714;
const TAG_BLACK_LEVEL_REPEAT_DIM: u16 = 50_713;
const TAG_BLACK_LEVEL_DELTA_H: u16 = 50_715;
const TAG_BLACK_LEVEL_DELTA_V: u16 = 50_716;
const TAG_WHITE_LEVEL: u16 = 50_717;
const TAG_DEFAULT_CROP_ORIGIN: u16 = 50_719;
const TAG_DEFAULT_CROP_SIZE: u16 = 50_720;
const TAG_ACTIVE_AREA: u16 = 50_829;
const TAG_COLORIMETRIC_REFERENCE: u16 = 50_879;
const TAG_SUB_TILE_BLOCK_SIZE: u16 = 50_974;
const TAG_ROW_INTERLEAVE_FACTOR: u16 = 50_975;
const TAG_BASELINE_EXPOSURE: u16 = 50_730;
const TAG_PROFILE_GAIN_TABLE_MAP: u16 = 52_525;
const TAG_PROFILE_TONE_CURVE: u16 = 50_940;
const TAG_ORIENTATION: u16 = 274;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_OPCODE_LIST_1: u16 = 51_008;
const TAG_OPCODE_LIST_2: u16 = 51_009;
const TAG_OPCODE_LIST_3: u16 = 51_022;
const PHOTOMETRIC_CFA: u16 = 32_803;
const PHOTOMETRIC_LINEAR_RAW: u16 = 34_892;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeLimits {
    pub max_input_bytes: usize,
    pub max_ifds: usize,
    pub max_ifd_entries: usize,
    pub max_value_bytes: usize,
    /// Bound for deferred profile payloads, independent of small numeric TIFF values.
    pub max_profile_gain_table_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
    pub max_frame_samples: u64,
    pub max_decoded_samples: u64,
    pub max_metadata_bytes: usize,
    pub max_codec_bytes: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 512 * 1024 * 1024,
            max_ifds: 64,
            max_ifd_entries: 4_096,
            max_value_bytes: 256 * 1024,
            max_profile_gain_table_bytes: 16 * 1024 * 1024,
            max_width: 65_536,
            max_height: 65_536,
            max_pixels: 50 * 1024 * 1024,
            max_frame_samples: 160 * 1024 * 1024,
            max_decoded_samples: 160 * 1024 * 1024,
            max_metadata_bytes: 64 * 1024 * 1024,
            max_codec_bytes: 1024 * 1024 * 1024,
        }
    }
}

impl DecodeLimits {
    pub fn with_max_metadata_bytes(mut self, value: usize) -> Self {
        self.max_metadata_bytes = value;
        self
    }
    pub fn with_max_codec_bytes(mut self, value: usize) -> Self {
        self.max_codec_bytes = value;
        self
    }
    pub fn with_max_input_bytes(mut self, value: usize) -> Self {
        self.max_input_bytes = value;
        self
    }
    pub fn with_max_ifds(mut self, value: usize) -> Self {
        self.max_ifds = value;
        self
    }
    pub fn with_max_ifd_entries(mut self, value: usize) -> Self {
        self.max_ifd_entries = value;
        self
    }
    pub fn with_max_value_bytes(mut self, value: usize) -> Self {
        self.max_value_bytes = value;
        self
    }
    /// Sets the maximum preserved profile gain-table payload; parsing borrows its byte range.
    pub fn with_max_profile_gain_table_bytes(mut self, value: usize) -> Self {
        self.max_profile_gain_table_bytes = value;
        self
    }
    pub fn with_max_dimensions(mut self, width: u32, height: u32) -> Self {
        self.max_width = width;
        self.max_height = height;
        self
    }
    pub fn with_max_pixels(mut self, value: u64) -> Self {
        self.max_pixels = value;
        self
    }
    pub fn with_max_frame_samples(mut self, value: u64) -> Self {
        self.max_frame_samples = value;
        self
    }
    pub fn with_max_decoded_samples(mut self, value: u64) -> Self {
        self.max_decoded_samples = value;
        self
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    NotRecognized,
    InvalidContainer,
    UnsupportedFormat,
    UnsupportedFeature,
    ResourceLimit,
    Truncated,
    InvalidOffset,
    InvalidTag,
    InvalidGeometry,
    Allocation,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotRecognized => "not recognized",
            Self::InvalidContainer => "invalid container",
            Self::UnsupportedFormat => "unsupported format",
            Self::UnsupportedFeature => "unsupported feature",
            Self::ResourceLimit => "resource limit exceeded",
            Self::Truncated => "truncated input",
            Self::InvalidOffset => "invalid offset",
            Self::InvalidTag => "invalid tag",
            Self::InvalidGeometry => "invalid geometry",
            Self::Allocation => "allocation failed",
        })
    }
}

impl std::error::Error for DecodeError {}

pub type ProbeResult<T> = std::result::Result<T, DecodeError>;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerProbe {
    Unknown,
    X3f(X3fFacts),
    Dng(DngFacts),
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X3fFacts {
    pub version: u32,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ByteOrder {
    #[default]
    LittleEndian,
    BigEndian,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DngFacts {
    pub byte_order: ByteOrder,
    pub dng_version: [u8; 4],
    pub dng_backward_version: Option<[u8; 4]>,
    pub width: u32,
    pub height: u32,
    pub samples_per_pixel: u16,
    pub bits_per_sample: Vec<u16>,
    pub compression: u16,
    pub photometric_interpretation: u16,
    pub sample_format: Vec<u16>,
    pub planar_configuration: u16,
    pub has_strip_offsets: bool,
    pub has_tile_offsets: bool,
    pub make: Option<String>,
    pub model: Option<String>,
    pub is_linear_raw: bool,
}

pub fn probe(bytes: &[u8], limits: DecodeLimits) -> ProbeResult<ContainerProbe> {
    if bytes.len() > limits.max_input_bytes {
        return Err(DecodeError::ResourceLimit);
    }
    if bytes.starts_with(b"FOVb") {
        return x3f_facts(bytes).map(ContainerProbe::X3f);
    }
    if bytes.starts_with(b"II") || bytes.starts_with(b"MM") {
        return match recognize_dng(bytes, limits) {
            Some(()) => dng_facts(bytes, limits).map(ContainerProbe::Dng),
            None => Ok(ContainerProbe::Unknown),
        };
    }
    Ok(ContainerProbe::Unknown)
}

fn recognize_dng(bytes: &[u8], limits: DecodeLimits) -> Option<()> {
    let order = ByteOrder::parse(bytes).ok()?;
    let reader = TiffReader { bytes, order };
    if reader.u16(2).ok()? != TIFF_MAGIC {
        return None;
    }
    let ifd = reader.offset(4).ok()?;
    let count = usize::from(reader.u16(ifd).ok()?);
    if count > limits.max_ifd_entries {
        return None;
    }
    let entries = ifd.checked_add(2)?;
    let size = count.checked_mul(12)?;
    reader.bytes(entries, size).ok()?;
    for index in 0..count {
        let entry = entries.checked_add(index.checked_mul(12)?)?;
        if reader.u16(entry).ok()? == TAG_DNG_VERSION
            && reader.u16(entry + 2).ok()? == 1
            && reader.u32(entry + 4).ok()? == 4
        {
            let value = reader.bytes(entry + 8, 4).ok()?;
            if value != [0; 4] {
                return Some(());
            }
        }
    }
    None
}

fn x3f_facts(bytes: &[u8]) -> ProbeResult<X3fFacts> {
    Ok(X3fFacts {
        version: read_u32_le(bytes, 4)?,
    })
}

fn dng_facts(bytes: &[u8], limits: DecodeLimits) -> ProbeResult<DngFacts> {
    let order = ByteOrder::parse(bytes)?;
    let reader = TiffReader { bytes, order };
    match reader.u16(2)? {
        TIFF_MAGIC => {}
        BIG_TIFF_MAGIC => return Err(DecodeError::UnsupportedFeature),
        _ => return Err(DecodeError::UnsupportedFormat),
    }
    let first_ifd = reader.offset(4)?;
    let mut parser = Parser::new(reader, limits);
    parser.parse(first_ifd)?;
    let version = parser.dng_version.ok_or(DecodeError::InvalidContainer)?;
    if version == [0; 4] {
        return Err(DecodeError::InvalidContainer);
    }
    let mut candidates = parser.candidates.into_iter();
    let candidate = candidates.next().ok_or(DecodeError::InvalidGeometry)?;
    if candidates.next().is_some() {
        return Err(DecodeError::InvalidGeometry);
    }
    candidate.into_facts(
        order,
        version,
        parser.backward_version,
        parser.ifd0_make,
        parser.ifd0_model,
        limits,
    )
}

pub(crate) fn dng_raw_facts(bytes: &[u8], limits: DecodeLimits) -> ProbeResult<DngRawFacts> {
    if bytes.len() > limits.max_input_bytes {
        return Err(DecodeError::ResourceLimit);
    }
    let order = ByteOrder::parse(bytes)?;
    let reader = TiffReader { bytes, order };
    match reader.u16(2)? {
        TIFF_MAGIC => {}
        BIG_TIFF_MAGIC => return Err(DecodeError::UnsupportedFeature),
        _ => return Err(DecodeError::UnsupportedFormat),
    }
    let mut parser = Parser::new(reader, limits);
    parser.retain_metadata = true;
    parser.parse(reader.offset(4)?)?;
    if parser.dng_version.ok_or(DecodeError::InvalidContainer)? == [0; 4] {
        return Err(DecodeError::InvalidContainer);
    }
    let mut candidates = parser.candidates.into_iter();
    let candidate = candidates.next().ok_or(DecodeError::InvalidGeometry)?;
    if candidates.next().is_some() {
        return Err(DecodeError::InvalidGeometry);
    }
    let mut facts = candidate.into_raw_facts(
        order,
        DngMetadata {
            version: parser.dng_version.ok_or(DecodeError::InvalidContainer)?,
            backward_version: parser.backward_version,
            make: parser.ifd0_make,
            model: parser.ifd0_model,
            orientation: parser.ifd0_orientation,
            colorimetric_reference: parser.ifd0_colorimetric_reference,
            has_semantic_masks: parser.has_semantic_masks,
            has_baseline_exposure: parser.has_baseline_exposure,
            has_profile_tone_curve: parser.has_profile_tone_curve,
        },
        limits,
    )?;
    facts.root_offset = reader.u32(4)?;
    facts.directories = parser.directories;
    Ok(facts)
}

impl ByteOrder {
    fn parse(bytes: &[u8]) -> ProbeResult<Self> {
        match bytes.get(..2) {
            Some(b"II") => Ok(Self::LittleEndian),
            Some(b"MM") => Ok(Self::BigEndian),
            Some(_) => Err(DecodeError::InvalidContainer),
            None => Err(DecodeError::Truncated),
        }
    }
}

#[derive(Clone, Copy)]
struct TiffReader<'a> {
    bytes: &'a [u8],
    order: ByteOrder,
}

impl<'a> TiffReader<'a> {
    fn bytes(&self, offset: usize, count: usize) -> ProbeResult<&'a [u8]> {
        let end = offset
            .checked_add(count)
            .ok_or(DecodeError::InvalidOffset)?;
        self.bytes.get(offset..end).ok_or(DecodeError::Truncated)
    }
    fn u16(&self, offset: usize) -> ProbeResult<u16> {
        let bytes = self.bytes(offset, 2)?;
        Ok(match self.order {
            ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
        })
    }
    fn u32(&self, offset: usize) -> ProbeResult<u32> {
        let bytes = self.bytes(offset, 4)?;
        Ok(match self.order {
            ByteOrder::LittleEndian => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::BigEndian => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }
    fn offset(&self, offset: usize) -> ProbeResult<usize> {
        usize::try_from(self.u32(offset)?).map_err(|_| DecodeError::InvalidOffset)
    }
}

struct Parser<'a> {
    reader: TiffReader<'a>,
    limits: DecodeLimits,
    visited: Vec<usize>,
    candidates: Vec<IfdFacts>,
    dng_version: Option<[u8; 4]>,
    backward_version: Option<[u8; 4]>,
    ifd0_make: Option<String>,
    ifd0_model: Option<String>,
    ifd0_orientation: Option<u32>,
    ifd0_colorimetric_reference: Option<u32>,
    has_semantic_masks: bool,
    has_baseline_exposure: bool,
    has_profile_tone_curve: bool,
    directories: Vec<DirectoryRange>,
    metadata_bytes: usize,
    retain_metadata: bool,
}

impl<'a> Parser<'a> {
    fn new(reader: TiffReader<'a>, limits: DecodeLimits) -> Self {
        Self {
            reader,
            limits,
            visited: Vec::new(),
            candidates: Vec::new(),
            dng_version: None,
            backward_version: None,
            ifd0_make: None,
            ifd0_model: None,
            ifd0_orientation: None,
            ifd0_colorimetric_reference: None,
            has_semantic_masks: false,
            has_baseline_exposure: false,
            has_profile_tone_curve: false,
            directories: Vec::new(),
            metadata_bytes: 0,
            retain_metadata: false,
        }
    }

    fn parse(&mut self, first_ifd: usize) -> ProbeResult<()> {
        let mut work = Vec::new();
        self.push_work(&mut work, first_ifd, true, None)?;
        while let Some((offset, is_ifd0, parent)) = work.pop() {
            self.visit(&mut work, offset, is_ifd0, parent)?;
        }
        Ok(())
    }

    fn push_work(
        &self,
        work: &mut Vec<(usize, bool, Option<u32>)>,
        offset: usize,
        is_ifd0: bool,
        parent: Option<u32>,
    ) -> ProbeResult<()> {
        if work.len() >= self.limits.max_ifds {
            return Err(DecodeError::ResourceLimit);
        }
        work.try_reserve(1).map_err(|_| DecodeError::Allocation)?;
        work.push((offset, is_ifd0, parent));
        Ok(())
    }

    fn visit(
        &mut self,
        work: &mut Vec<(usize, bool, Option<u32>)>,
        offset: usize,
        is_ifd0: bool,
        parent: Option<u32>,
    ) -> ProbeResult<()> {
        if self.visited.len() >= self.limits.max_ifds {
            return Err(DecodeError::ResourceLimit);
        }
        if self.visited.contains(&offset) {
            return Err(DecodeError::InvalidOffset);
        }
        self.visited
            .try_reserve(1)
            .map_err(|_| DecodeError::Allocation)?;
        self.visited.push(offset);
        let count = usize::from(self.reader.u16(offset)?);
        if count > self.limits.max_ifd_entries {
            return Err(DecodeError::ResourceLimit);
        }
        let entries = offset.checked_add(2).ok_or(DecodeError::InvalidOffset)?;
        let size = count.checked_mul(12).ok_or(DecodeError::InvalidOffset)?;
        self.reader.bytes(entries, size)?;
        let next = entries
            .checked_add(size)
            .ok_or(DecodeError::InvalidOffset)?;
        let next_ifd = self.reader.offset(next)?;
        let mut facts = IfdFacts {
            offset: offset as u32,
            ..IfdFacts::default()
        };
        let mut directory = DirectoryRange {
            offset: offset as u32,
            parent_offset: parent,
            tags: Vec::new(),
        };
        let mut sub_ifds = None;
        let mut seen = TagPresence::default();
        for index in 0..count {
            let entry = entries
                .checked_add(index.checked_mul(12).ok_or(DecodeError::InvalidOffset)?)
                .ok_or(DecodeError::InvalidOffset)?;
            seen.record(self.reader.u16(entry)?)?;
        }
        for index in 0..count {
            let entry = entries
                .checked_add(index.checked_mul(12).ok_or(DecodeError::InvalidOffset)?)
                .ok_or(DecodeError::InvalidOffset)?;
            let tag = self.reader.u16(entry)?;
            let kind = self.reader.u16(entry + 2)?;
            let count = self.reader.u32(entry + 4)?;
            let value = entry + 8;
            if self.retain_metadata {
                let size = type_size(kind)?
                    .checked_mul(count as usize)
                    .ok_or(DecodeError::ResourceLimit)?;
                self.metadata_bytes = self
                    .metadata_bytes
                    .checked_add(size)
                    .ok_or(DecodeError::ResourceLimit)?;
                if self.metadata_bytes > self.limits.max_metadata_bytes {
                    return Err(DecodeError::ResourceLimit);
                }
                let start = if size <= 4 {
                    value
                } else {
                    self.reader.offset(value)?
                };
                self.reader.bytes(start, size)?;
                directory
                    .tags
                    .try_reserve(1)
                    .map_err(|_| DecodeError::Allocation)?;
                directory.tags.push(TagRange {
                    id: tag,
                    field_type: kind,
                    count,
                    range: start..start + size,
                });
            }
            match tag {
                TAG_DNG_VERSION if is_ifd0 => {
                    self.dng_version = Some(self.version(value, kind, count)?)
                }
                TAG_DNG_BACKWARD_VERSION if is_ifd0 => {
                    self.backward_version = Some(self.version(value, kind, count)?)
                }
                TAG_NEW_SUBFILE_TYPE => facts.new_subfile_type = self.scalar(value, kind, count)?,
                TAG_IMAGE_WIDTH => facts.width = self.scalar(value, kind, count)?,
                TAG_IMAGE_LENGTH => facts.height = self.scalar(value, kind, count)?,
                TAG_BITS_PER_SAMPLE => {
                    facts.bits_per_sample = Some(self.components(value, kind, count)?)
                }
                TAG_COMPRESSION => facts.compression = self.scalar(value, kind, count)?,
                TAG_PHOTOMETRIC => facts.photometric = self.scalar(value, kind, count)?,
                TAG_MAKE if is_ifd0 => self.ifd0_make = self.text(value, kind, count)?,
                TAG_MODEL if is_ifd0 => self.ifd0_model = self.text(value, kind, count)?,
                TAG_ORIENTATION if is_ifd0 => {
                    self.ifd0_orientation = self.scalar(value, kind, count)?
                }
                TAG_STRIP_OFFSETS => {
                    facts.has_strip_offsets = true;
                    facts.strip_offsets = Some(self.offsets(value, kind, count)?);
                }
                278 => facts.rows_per_strip = self.scalar(value, kind, count)?,
                279 => facts.strip_byte_counts = Some(self.offsets(value, kind, count)?),
                TAG_SAMPLES_PER_PIXEL => {
                    facts.samples_per_pixel = self.scalar(value, kind, count)?
                }
                TAG_PLANAR_CONFIGURATION => {
                    facts.planar_configuration = self.scalar(value, kind, count)?
                }
                TAG_TILE_WIDTH => facts.tile_width = self.scalar(value, kind, count)?,
                TAG_TILE_LENGTH => facts.tile_length = self.scalar(value, kind, count)?,
                TAG_TILE_OFFSETS => {
                    facts.has_tile_offsets = true;
                    facts.tile_offsets = Some(self.offsets(value, kind, count)?);
                }
                TAG_TILE_BYTE_COUNTS => {
                    facts.tile_byte_counts = Some(self.offsets(value, kind, count)?)
                }
                TAG_LINEARIZATION_TABLE => {
                    facts.linearization_table = Some(self.sample_components(value, kind, count)?)
                }
                TAG_BLACK_LEVEL => {
                    facts.black_level =
                        Some(self.real_components(value, kind, count, &[3, 4, 5])?)
                }
                TAG_BLACK_LEVEL_REPEAT_DIM => {
                    facts.black_level_repeat_dim = Some(self.pair(value, kind, count)?)
                }
                TAG_BLACK_LEVEL_DELTA_H => {
                    facts.black_level_delta_h =
                        Some(self.real_components(value, kind, count, &[10])?)
                }
                TAG_BLACK_LEVEL_DELTA_V => {
                    facts.black_level_delta_v =
                        Some(self.real_components(value, kind, count, &[10])?)
                }
                TAG_WHITE_LEVEL => {
                    facts.white_level = Some(self.real_components(value, kind, count, &[3, 4])?)
                }
                TAG_DEFAULT_CROP_ORIGIN => {
                    facts.default_crop_origin_exact = Some(self.real_pair(value, kind, count)?);
                    facts.default_crop_origin =
                        integral_pair(facts.default_crop_origin_exact.unwrap());
                }
                TAG_DEFAULT_CROP_SIZE => {
                    facts.default_crop_size_exact = Some(self.real_pair(value, kind, count)?);
                    facts.default_crop_size = integral_pair(facts.default_crop_size_exact.unwrap());
                }
                TAG_ACTIVE_AREA => facts.active_area = Some(self.quad(value, kind, count)?),
                TAG_COLORIMETRIC_REFERENCE if is_ifd0 => {
                    self.ifd0_colorimetric_reference = Some(self.short_scalar(value, kind, count)?)
                }
                TAG_COLORIMETRIC_REFERENCE => {
                    self.short_scalar(value, kind, count)?;
                    return Err(DecodeError::UnsupportedFeature);
                }
                TAG_SUB_TILE_BLOCK_SIZE => {
                    facts.sub_tile_block_size = Some(self.short_or_long_pair(value, kind, count)?)
                }
                TAG_ROW_INTERLEAVE_FACTOR => {
                    facts.row_interleave_factor =
                        Some(self.short_or_long_scalar(value, kind, count)?)
                }
                52547 => {
                    facts.column_interleave_factor =
                        Some(self.short_or_long_scalar(value, kind, count)?)
                }
                TAG_BASELINE_EXPOSURE => {
                    self.baseline_exposure(value, kind, count)?;
                    if is_ifd0 {
                        self.has_baseline_exposure = true;
                    }
                }
                TAG_PROFILE_GAIN_TABLE_MAP => {
                    facts.profile_gain_table_map =
                        Some(self.profile_gain_table_range(value, kind, count)?)
                }
                52544 => {
                    self.profile_gain_table_range(value, kind, count)?;
                }
                TAG_OPCODE_LIST_1 => {
                    facts.opcode_lists[0] = Some(self.profile_gain_table_range(value, kind, count)?)
                }
                TAG_OPCODE_LIST_2 => {
                    facts.opcode_lists[1] = Some(self.profile_gain_table_range(value, kind, count)?)
                }
                TAG_OPCODE_LIST_3 => {
                    facts.opcode_lists[2] = Some(self.profile_gain_table_range(value, kind, count)?)
                }
                TAG_PROFILE_TONE_CURVE => {
                    self.profile_tone_curve(value, kind, count)?;
                    if is_ifd0 {
                        self.has_profile_tone_curve = true;
                    }
                }
                TAG_ORIENTATION => facts.orientation = self.scalar(value, kind, count)?,
                TAG_SUB_IFDS => sub_ifds = Some((value, kind, count)),
                34665 | 34853 | 40965 => {
                    let child = self.short_or_long_scalar(value, kind, count)?;
                    if child != 0 {
                        self.push_work(work, child as usize, false, Some(offset as u32))?;
                    }
                }
                TAG_SAMPLE_FORMAT => {
                    facts.sample_format = Some(self.components(value, kind, count)?)
                }
                _ => {}
            }
        }
        self.directories
            .try_reserve(1)
            .map_err(|_| DecodeError::Allocation)?;
        self.directories.push(directory);
        if facts.new_subfile_type == Some(65_540) {
            self.has_semantic_masks = true;
        }
        if facts.is_primary_raw() {
            self.candidates
                .try_reserve(1)
                .map_err(|_| DecodeError::Allocation)?;
            self.candidates.push(facts);
        }
        if let Some((value, kind, count)) = sub_ifds {
            for index in 0..count {
                self.push_work(
                    work,
                    self.sub_ifd_offset(value, kind, count, index)?,
                    false,
                    Some(offset as u32),
                )?;
            }
        }
        if next_ifd != 0 {
            self.push_work(work, next_ifd, false, parent)?;
        }
        Ok(())
    }

    fn value(&self, value: usize, kind: u16, count: u32) -> ProbeResult<&'a [u8]> {
        let unit = type_size(kind)?;
        let count = usize::try_from(count).map_err(|_| DecodeError::InvalidTag)?;
        let size = unit.checked_mul(count).ok_or(DecodeError::InvalidTag)?;
        if size > self.limits.max_value_bytes {
            return Err(DecodeError::ResourceLimit);
        }
        if size <= 4 {
            return self.reader.bytes(value, size);
        }
        let offset = self.reader.offset(value)?;
        let end = offset.checked_add(size).ok_or(DecodeError::InvalidOffset)?;
        self.reader
            .bytes
            .get(offset..end)
            .ok_or(DecodeError::InvalidOffset)
    }

    fn scalar(&self, value: usize, kind: u16, count: u32) -> ProbeResult<Option<u32>> {
        if count != 1 {
            return if count == 0 {
                Ok(None)
            } else {
                Err(DecodeError::InvalidTag)
            };
        }
        let bytes = self.value(value, kind, count)?;
        let value = match kind {
            1 => u32::from(bytes[0]),
            3 => self.u16(bytes, 0).map(u32::from)?,
            4 => self.u32(bytes, 0)?,
            _ => return Err(DecodeError::InvalidTag),
        };
        Ok(Some(value))
    }

    fn short_scalar(&self, value: usize, kind: u16, count: u32) -> ProbeResult<u32> {
        if kind != 3 || count != 1 {
            return Err(DecodeError::InvalidTag);
        }
        Ok(u32::from(self.u16(self.value(value, kind, count)?, 0)?))
    }

    fn short_or_long_scalar(&self, value: usize, kind: u16, count: u32) -> ProbeResult<u32> {
        if count != 1 || !matches!(kind, 3 | 4) {
            return Err(DecodeError::InvalidTag);
        }
        self.numeric_component(value, kind, count, 0)
    }

    fn short_or_long_pair(&self, value: usize, kind: u16, count: u32) -> ProbeResult<[u32; 2]> {
        if count != 2 || !matches!(kind, 3 | 4) {
            return Err(DecodeError::InvalidTag);
        }
        Ok([
            self.numeric_component(value, kind, count, 0)?,
            self.numeric_component(value, kind, count, 1)?,
        ])
    }

    fn baseline_exposure(&self, value: usize, kind: u16, count: u32) -> ProbeResult<()> {
        if kind != 10 || count != 1 {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self.value(value, kind, count)?;
        if self.u32(bytes, 4)? == 0 {
            return Err(DecodeError::InvalidTag);
        }
        Ok(())
    }

    fn profile_tone_curve(&self, value: usize, kind: u16, count: u32) -> ProbeResult<()> {
        if kind != 11 || count < 4 || count % 2 != 0 {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self.value(value, kind, count)?;
        for index in 0..usize::try_from(count).map_err(|_| DecodeError::InvalidTag)? {
            if !f32::from_bits(self.u32(bytes, index * 4)?).is_finite() {
                return Err(DecodeError::InvalidTag);
            }
        }
        Ok(())
    }

    fn components(&self, value: usize, kind: u16, count: u32) -> ProbeResult<Vec<u16>> {
        if !matches!(kind, 1 | 3) {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self.value(value, kind, count)?;
        let count = usize::try_from(count).map_err(|_| DecodeError::InvalidTag)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::Allocation)?;
        for index in 0..count {
            values.push(if kind == 1 {
                u16::from(bytes[index])
            } else {
                self.u16(bytes, index * 2)?
            });
        }
        Ok(values)
    }

    fn sample_components(&self, value: usize, kind: u16, count: u32) -> ProbeResult<Vec<u16>> {
        match kind {
            1 | 3 => self.components(value, kind, count),
            4 | 5 | 8 | 9 | 10 | 11 | 12 => Err(DecodeError::UnsupportedFeature),
            _ => Err(DecodeError::InvalidTag),
        }
    }

    fn real_components(
        &self,
        value: usize,
        kind: u16,
        count: u32,
        kinds: &[u16],
    ) -> ProbeResult<Vec<f64>> {
        if !kinds.contains(&kind) || count == 0 {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self.value(value, kind, count)?;
        let mut values = crate::dng_metadata::allocate(count as usize)?;
        for index in 0..count as usize {
            values.push(number(bytes, kind, index, self.reader.order)?);
        }
        Ok(values)
    }

    fn real_pair(&self, value: usize, kind: u16, count: u32) -> ProbeResult<[f64; 2]> {
        if count != 2 {
            return Err(DecodeError::InvalidTag);
        }
        let values = self.real_components(value, kind, count, &[3, 4, 5])?;
        Ok([values[0], values[1]])
    }

    fn profile_gain_table_range(
        &self,
        value: usize,
        kind: u16,
        count: u32,
    ) -> ProbeResult<std::ops::Range<usize>> {
        match kind {
            1 | 7 => {}
            2..=13 => return Err(DecodeError::UnsupportedFeature),
            _ => return Err(DecodeError::InvalidTag),
        }
        let size = usize::try_from(count).map_err(|_| DecodeError::ResourceLimit)?;
        if size > self.limits.max_profile_gain_table_bytes {
            return Err(DecodeError::ResourceLimit);
        }
        let offset = if size <= 4 {
            value
        } else {
            self.reader.offset(value)?
        };
        self.reader.bytes(offset, size)?;
        Ok(offset..offset + size)
    }

    fn pair(&self, value: usize, kind: u16, count: u32) -> ProbeResult<[u32; 2]> {
        if count != 2 {
            return Err(DecodeError::InvalidTag);
        }
        Ok([
            self.numeric_component(value, kind, count, 0)?,
            self.numeric_component(value, kind, count, 1)?,
        ])
    }

    fn quad(&self, value: usize, kind: u16, count: u32) -> ProbeResult<[u32; 4]> {
        if count != 4 {
            return Err(DecodeError::InvalidTag);
        }
        Ok([
            self.numeric_component(value, kind, count, 0)?,
            self.numeric_component(value, kind, count, 1)?,
            self.numeric_component(value, kind, count, 2)?,
            self.numeric_component(value, kind, count, 3)?,
        ])
    }

    fn numeric_component(
        &self,
        value: usize,
        kind: u16,
        count: u32,
        index: usize,
    ) -> ProbeResult<u32> {
        let bytes = self.value(value, kind, count)?;
        match kind {
            1 => Ok(u32::from(bytes[index])),
            3 => self
                .u16(bytes, index.checked_mul(2).ok_or(DecodeError::InvalidTag)?)
                .map(u32::from),
            4 => self.u32(bytes, index.checked_mul(4).ok_or(DecodeError::InvalidTag)?),
            5 => {
                let offset = index.checked_mul(8).ok_or(DecodeError::InvalidTag)?;
                let numerator = self.u32(bytes, offset)?;
                let denominator =
                    self.u32(bytes, offset.checked_add(4).ok_or(DecodeError::InvalidTag)?)?;
                if denominator != 0 && numerator % denominator == 0 {
                    Ok(numerator / denominator)
                } else {
                    Err(DecodeError::UnsupportedFeature)
                }
            }
            _ => Err(DecodeError::InvalidTag),
        }
    }

    fn version(&self, value: usize, kind: u16, count: u32) -> ProbeResult<[u8; 4]> {
        if kind != 1 || count != 4 {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self.value(value, kind, count)?;
        Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    fn text(&self, value: usize, kind: u16, count: u32) -> ProbeResult<Option<String>> {
        if kind != 2 || count == 0 {
            return Ok(None);
        }
        let bytes = self.value(value, kind, count)?;
        let text = bytes.split(|&byte| byte == 0).next().unwrap_or_default();
        if !text.is_ascii() {
            return Err(DecodeError::InvalidTag);
        }
        let mut value = String::new();
        value
            .try_reserve(text.len())
            .map_err(|_| DecodeError::Allocation)?;
        value.push_str(std::str::from_utf8(text).map_err(|_| DecodeError::InvalidTag)?);
        Ok(Some(value))
    }

    fn sub_ifd_offset(
        &self,
        value: usize,
        kind: u16,
        count: u32,
        index: u32,
    ) -> ProbeResult<usize> {
        if !matches!(kind, 4 | 13) || index >= count {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self.value(value, kind, count)?;
        let offset = usize::try_from(index)
            .map_err(|_| DecodeError::InvalidTag)?
            .checked_mul(4)
            .ok_or(DecodeError::InvalidTag)?;
        usize::try_from(self.u32(bytes, offset)?).map_err(|_| DecodeError::InvalidOffset)
    }
    fn offsets(&self, value: usize, kind: u16, count: u32) -> ProbeResult<Vec<u32>> {
        if !matches!(kind, 3 | 4) {
            return Err(DecodeError::InvalidTag);
        }
        let bytes = self.value(value, kind, count)?;
        let count = usize::try_from(count).map_err(|_| DecodeError::InvalidTag)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::Allocation)?;
        for index in 0..count {
            values.push(if kind == 3 {
                u32::from(self.u16(bytes, index.checked_mul(2).ok_or(DecodeError::InvalidTag)?)?)
            } else {
                self.u32(bytes, index.checked_mul(4).ok_or(DecodeError::InvalidTag)?)?
            });
        }
        Ok(values)
    }

    fn u16(&self, bytes: &[u8], offset: usize) -> ProbeResult<u16> {
        self.reader_with(bytes).u16(offset)
    }
    fn u32(&self, bytes: &[u8], offset: usize) -> ProbeResult<u32> {
        self.reader_with(bytes).u32(offset)
    }
    fn reader_with<'b>(&self, bytes: &'b [u8]) -> TiffReader<'b> {
        TiffReader {
            bytes,
            order: self.reader.order,
        }
    }
}

fn type_size(kind: u16) -> ProbeResult<usize> {
    match kind {
        1 | 2 | 6 | 7 => Ok(1),
        3 | 8 => Ok(2),
        4 | 9 | 11 | 13 => Ok(4),
        5 | 10 | 12 => Ok(8),
        _ => Err(DecodeError::InvalidTag),
    }
}
fn read_u32_le(bytes: &[u8], offset: usize) -> ProbeResult<u32> {
    let end = offset.checked_add(4).ok_or(DecodeError::InvalidOffset)?;
    let bytes = bytes.get(offset..end).ok_or(DecodeError::Truncated)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[derive(Default, Clone)]
struct IfdFacts {
    offset: u32,
    new_subfile_type: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    bits_per_sample: Option<Vec<u16>>,
    compression: Option<u32>,
    photometric: Option<u32>,
    samples_per_pixel: Option<u32>,
    sample_format: Option<Vec<u16>>,
    planar_configuration: Option<u32>,
    has_strip_offsets: bool,
    strip_offsets: Option<Vec<u32>>,
    strip_byte_counts: Option<Vec<u32>>,
    rows_per_strip: Option<u32>,
    has_tile_offsets: bool,
    tile_width: Option<u32>,
    tile_length: Option<u32>,
    tile_offsets: Option<Vec<u32>>,
    tile_byte_counts: Option<Vec<u32>>,
    linearization_table: Option<Vec<u16>>,
    black_level: Option<Vec<f64>>,
    black_level_repeat_dim: Option<[u32; 2]>,
    black_level_delta_h: Option<Vec<f64>>,
    black_level_delta_v: Option<Vec<f64>>,
    white_level: Option<Vec<f64>>,
    active_area: Option<[u32; 4]>,
    orientation: Option<u32>,
    default_crop_origin: Option<[u32; 2]>,
    default_crop_size: Option<[u32; 2]>,
    default_crop_origin_exact: Option<[f64; 2]>,
    default_crop_size_exact: Option<[f64; 2]>,
    opcode_lists: [Option<std::ops::Range<usize>>; 3],
    profile_gain_table_map: Option<std::ops::Range<usize>>,
    sub_tile_block_size: Option<[u32; 2]>,
    row_interleave_factor: Option<u32>,
    column_interleave_factor: Option<u32>,
}

struct TagPresence([u64; 1024]);

impl Default for TagPresence {
    fn default() -> Self {
        Self([0; 1024])
    }
}

impl TagPresence {
    fn record(&mut self, tag: u16) -> ProbeResult<()> {
        let index = usize::from(tag);
        let word = index / u64::BITS as usize;
        let mask = 1_u64 << (index % u64::BITS as usize);
        if self.0[word] & mask != 0 {
            return Err(DecodeError::InvalidTag);
        }
        self.0[word] |= mask;
        Ok(())
    }
}

struct DngMetadata {
    version: [u8; 4],
    backward_version: Option<[u8; 4]>,
    make: Option<String>,
    model: Option<String>,
    orientation: Option<u32>,
    colorimetric_reference: Option<u32>,
    has_semantic_masks: bool,
    has_baseline_exposure: bool,
    has_profile_tone_curve: bool,
}

#[derive(Debug)]
pub(crate) struct DngRawFacts {
    pub root_offset: u32,
    pub raw_offset: u32,
    pub directories: Vec<DirectoryRange>,
    pub dng_version: [u8; 4],
    pub byte_order: ByteOrder,
    pub dng_backward_version: Option<[u8; 4]>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub width: u32,
    pub height: u32,
    pub bits_per_sample: Vec<u16>,
    pub compression: u16,
    pub photometric: u16,
    pub samples_per_pixel: u16,
    pub sample_format: Vec<u16>,
    pub planar_configuration: u16,
    pub has_strip_offsets: bool,
    pub strip_offsets: Option<Vec<u32>>,
    pub strip_byte_counts: Option<Vec<u32>>,
    pub rows_per_strip: Option<u32>,
    pub tile_width: Option<u32>,
    pub tile_length: Option<u32>,
    pub tile_offsets: Option<Vec<u32>>,
    pub tile_byte_counts: Option<Vec<u32>>,
    pub linearization_table: Option<Vec<u16>>,
    pub black_level: Option<Vec<f64>>,
    pub black_level_repeat_dim: Option<[u32; 2]>,
    pub black_level_delta_h: Option<Vec<f64>>,
    pub black_level_delta_v: Option<Vec<f64>>,
    pub white_level: Option<Vec<f64>>,
    pub colorimetric_reference: Option<u32>,
    pub sub_tile_block_size: Option<[u32; 2]>,
    pub row_interleave_factor: Option<u32>,
    pub column_interleave_factor: Option<u32>,
    pub active_area: Option<[u32; 4]>,
    pub orientation: Option<u32>,
    pub default_crop_origin: Option<[u32; 2]>,
    pub default_crop_size: Option<[u32; 2]>,
    pub default_crop_origin_exact: Option<[f64; 2]>,
    pub default_crop_size_exact: Option<[f64; 2]>,
    pub opcode_lists: [Option<std::ops::Range<usize>>; 3],
    pub profile_gain_table_map: Option<std::ops::Range<usize>>,
    pub has_semantic_masks: bool,
    pub has_baseline_exposure: bool,
    pub has_profile_tone_curve: bool,
}

impl IfdFacts {
    fn is_primary_raw(&self) -> bool {
        self.new_subfile_type == Some(0)
            && matches!(self.photometric, Some(value) if value == u32::from(PHOTOMETRIC_CFA) || value == u32::from(PHOTOMETRIC_LINEAR_RAW))
    }
    fn into_facts(
        self,
        byte_order: ByteOrder,
        dng_version: [u8; 4],
        dng_backward_version: Option<[u8; 4]>,
        make: Option<String>,
        model: Option<String>,
        limits: DecodeLimits,
    ) -> ProbeResult<DngFacts> {
        let width = self.width.ok_or(DecodeError::InvalidGeometry)?;
        let height = self.height.ok_or(DecodeError::InvalidGeometry)?;
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or(DecodeError::InvalidGeometry)?;
        if width == 0 || height == 0 {
            return Err(DecodeError::InvalidGeometry);
        }
        if width > limits.max_width || height > limits.max_height || pixels > limits.max_pixels {
            return Err(DecodeError::ResourceLimit);
        }
        let samples_per_pixel = self.samples_per_pixel.unwrap_or(1);
        let samples_per_pixel =
            u16::try_from(samples_per_pixel).map_err(|_| DecodeError::InvalidTag)?;
        if samples_per_pixel == 0 {
            return Err(DecodeError::InvalidTag);
        }
        let bits_per_sample = self.bits_per_sample.ok_or(DecodeError::InvalidTag)?;
        if bits_per_sample.len() != usize::from(samples_per_pixel) || bits_per_sample.contains(&0) {
            return Err(DecodeError::InvalidTag);
        }
        let sample_format = match self.sample_format {
            Some(value) => value,
            None => default_components(usize::from(samples_per_pixel))?,
        };
        if sample_format.len() != usize::from(samples_per_pixel)
            || sample_format.iter().any(|&value| !(1..=6).contains(&value))
        {
            return Err(DecodeError::InvalidTag);
        }
        let compression =
            u16::try_from(self.compression.unwrap_or(1)).map_err(|_| DecodeError::InvalidTag)?;
        let planar_configuration = u16::try_from(self.planar_configuration.unwrap_or(1))
            .map_err(|_| DecodeError::InvalidTag)?;
        if compression == 0 || !(1..=2).contains(&planar_configuration) {
            return Err(DecodeError::InvalidTag);
        }
        Ok(DngFacts {
            byte_order,
            dng_version,
            dng_backward_version,
            width,
            height,
            samples_per_pixel,
            bits_per_sample,
            compression,
            photometric_interpretation: u16::try_from(
                self.photometric.ok_or(DecodeError::InvalidTag)?,
            )
            .map_err(|_| DecodeError::InvalidTag)?,
            sample_format,
            planar_configuration,
            has_strip_offsets: self.has_strip_offsets,
            has_tile_offsets: self.has_tile_offsets,
            make,
            model,
            is_linear_raw: self.photometric == Some(u32::from(PHOTOMETRIC_LINEAR_RAW)),
        })
    }
    fn into_raw_facts(
        self,
        byte_order: ByteOrder,
        metadata: DngMetadata,
        limits: DecodeLimits,
    ) -> ProbeResult<DngRawFacts> {
        let facts = self.clone().into_facts(
            byte_order,
            metadata.version,
            metadata.backward_version,
            metadata.make.clone(),
            metadata.model.clone(),
            limits,
        )?;
        Ok(DngRawFacts {
            root_offset: 0,
            raw_offset: self.offset,
            directories: Vec::new(),
            dng_version: metadata.version,
            byte_order,
            dng_backward_version: metadata.backward_version,
            make: metadata.make,
            model: metadata.model,
            width: facts.width,
            height: facts.height,
            bits_per_sample: facts.bits_per_sample,
            compression: facts.compression,
            photometric: facts.photometric_interpretation,
            samples_per_pixel: facts.samples_per_pixel,
            sample_format: facts.sample_format,
            planar_configuration: facts.planar_configuration,
            has_strip_offsets: facts.has_strip_offsets,
            strip_offsets: self.strip_offsets,
            strip_byte_counts: self.strip_byte_counts,
            rows_per_strip: self.rows_per_strip,
            tile_width: self.tile_width,
            tile_length: self.tile_length,
            tile_offsets: self.tile_offsets,
            tile_byte_counts: self.tile_byte_counts,
            linearization_table: self.linearization_table,
            black_level: self.black_level,
            black_level_repeat_dim: self.black_level_repeat_dim,
            black_level_delta_h: self.black_level_delta_h,
            black_level_delta_v: self.black_level_delta_v,
            white_level: self.white_level,
            colorimetric_reference: metadata.colorimetric_reference,
            sub_tile_block_size: self.sub_tile_block_size,
            row_interleave_factor: self.row_interleave_factor,
            column_interleave_factor: self.column_interleave_factor,
            active_area: self.active_area,
            orientation: metadata.orientation.or(self.orientation),
            default_crop_origin: self.default_crop_origin,
            default_crop_size: self.default_crop_size,
            default_crop_origin_exact: self.default_crop_origin_exact,
            default_crop_size_exact: self.default_crop_size_exact,
            opcode_lists: self.opcode_lists,
            profile_gain_table_map: self.profile_gain_table_map,
            has_semantic_masks: metadata.has_semantic_masks,
            has_baseline_exposure: metadata.has_baseline_exposure,
            has_profile_tone_curve: metadata.has_profile_tone_curve,
        })
    }
}

fn default_components(count: usize) -> ProbeResult<Vec<u16>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| DecodeError::Allocation)?;
    values.resize(count, 1);
    Ok(values)
}

fn integral_pair(values: [f64; 2]) -> Option<[u32; 2]> {
    values
        .iter()
        .all(|value| *value >= 0.0 && *value <= f64::from(u32::MAX) && value.fract() == 0.0)
        .then_some([values[0] as u32, values[1] as u32])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(bytes: &mut [u8], offset: usize, value: u16, order: ByteOrder) {
        match order {
            ByteOrder::LittleEndian => {
                bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes())
            }
            ByteOrder::BigEndian => bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes()),
        }
    }
    fn put32(bytes: &mut [u8], offset: usize, value: u32, order: ByteOrder) {
        match order {
            ByteOrder::LittleEndian => {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
            }
            ByteOrder::BigEndian => bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes()),
        }
    }
    fn entry(
        bytes: &mut [u8],
        offset: usize,
        tag: u16,
        kind: u16,
        count: u32,
        value: u32,
        order: ByteOrder,
    ) {
        put16(bytes, offset, tag, order);
        put16(bytes, offset + 2, kind, order);
        put32(bytes, offset + 4, count, order);
        let value = if order == ByteOrder::BigEndian && kind == 3 && count == 1 {
            value << 16
        } else if order == ByteOrder::BigEndian && tag == TAG_DNG_VERSION && kind == 1 && count == 4
        {
            value.swap_bytes()
        } else {
            value
        };
        put32(bytes, offset + 8, value, order);
    }

    fn dng(order: ByteOrder, sub_ifd_kind: u16) -> Vec<u8> {
        let mut bytes = vec![0; 256];
        bytes[..2].copy_from_slice(match order {
            ByteOrder::LittleEndian => b"II",
            ByteOrder::BigEndian => b"MM",
        });
        put16(&mut bytes, 2, TIFF_MAGIC, order);
        put32(&mut bytes, 4, 8, order);
        put16(&mut bytes, 8, 2, order);
        entry(&mut bytes, 10, TAG_DNG_VERSION, 1, 4, 0x0000_0601, order);
        entry(&mut bytes, 22, TAG_SUB_IFDS, sub_ifd_kind, 1, 100, order);
        put32(&mut bytes, 34, 0, order);
        put16(&mut bytes, 100, 8, order);
        entry(&mut bytes, 102, TAG_NEW_SUBFILE_TYPE, 4, 1, 0, order);
        entry(&mut bytes, 114, TAG_IMAGE_WIDTH, 4, 1, 4032, order);
        entry(&mut bytes, 126, TAG_IMAGE_LENGTH, 4, 1, 3024, order);
        entry(&mut bytes, 138, TAG_BITS_PER_SAMPLE, 3, 3, 220, order);
        entry(
            &mut bytes,
            150,
            TAG_PHOTOMETRIC,
            3,
            1,
            u32::from(PHOTOMETRIC_LINEAR_RAW),
            order,
        );
        entry(&mut bytes, 162, TAG_SAMPLES_PER_PIXEL, 3, 1, 3, order);
        entry(&mut bytes, 174, TAG_STRIP_OFFSETS, 4, 1, 240, order);
        entry(&mut bytes, 186, TAG_SAMPLE_FORMAT, 3, 3, 226, order);
        put32(&mut bytes, 198, 0, order);
        for index in 0..3 {
            put16(&mut bytes, 220 + index * 2, 12, order);
            put16(&mut bytes, 226 + index * 2, 1, order);
        }
        bytes
    }

    fn facts(bytes: &[u8]) -> DngFacts {
        match probe(bytes, DecodeLimits::default()).unwrap() {
            ContainerProbe::Dng(value) => value,
            _ => panic!("expected DNG"),
        }
    }

    fn dng_with_duplicate_sub_ifd_tag(tag: u16, kind: u16, first: u32, second: u32) -> Vec<u8> {
        let order = ByteOrder::LittleEndian;
        let mut bytes = dng(order, 4);
        let entries = if tag == TAG_NEW_SUBFILE_TYPE { 9 } else { 10 };
        put16(&mut bytes, 100, entries, order);
        if tag == TAG_NEW_SUBFILE_TYPE {
            entry(&mut bytes, 102, tag, kind, 1, first, order);
            entry(&mut bytes, 198, tag, kind, 1, second, order);
            put32(&mut bytes, 210, 0, order);
        } else {
            entry(&mut bytes, 198, tag, kind, 1, first, order);
            entry(&mut bytes, 210, tag, kind, 1, second, order);
            put32(&mut bytes, 222, 0, order);
        }
        bytes
    }

    #[test]
    fn probes_linear_raw_in_both_byte_orders() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let value = facts(&dng(order, 4));
            assert_eq!(value.dng_version, [1, 6, 0, 0]);
            assert_eq!(value.samples_per_pixel, 3);
            assert_eq!(value.bits_per_sample, [12; 3]);
            assert_eq!(value.sample_format, [1; 3]);
            assert!(value.is_linear_raw);
        }
    }

    #[test]
    fn recognizes_cfa_without_calling_it_linear_raw() {
        let mut bytes = dng(ByteOrder::LittleEndian, 4);
        entry(
            &mut bytes,
            150,
            TAG_PHOTOMETRIC,
            3,
            1,
            u32::from(PHOTOMETRIC_CFA),
            ByteOrder::LittleEndian,
        );
        assert!(!facts(&bytes).is_linear_raw);
    }

    #[test]
    fn allows_new_subfile_type_in_distinct_ifds_and_selects_raw_subifd_over_larger_preview() {
        let mut bytes = dng(ByteOrder::LittleEndian, 4);
        put16(&mut bytes, 8, 6, ByteOrder::LittleEndian);
        entry(
            &mut bytes,
            10,
            TAG_DNG_VERSION,
            1,
            4,
            0x0000_0601,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            22,
            TAG_NEW_SUBFILE_TYPE,
            4,
            1,
            1,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            34,
            TAG_IMAGE_WIDTH,
            4,
            1,
            10_000,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            46,
            TAG_IMAGE_LENGTH,
            4,
            1,
            10_000,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            58,
            TAG_PHOTOMETRIC,
            3,
            1,
            2,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            70,
            TAG_SUB_IFDS,
            4,
            1,
            100,
            ByteOrder::LittleEndian,
        );
        put32(&mut bytes, 82, 0, ByteOrder::LittleEndian);
        assert_eq!((facts(&bytes).width, facts(&bytes).height), (4032, 3024));
    }

    #[test]
    fn rejects_identical_and_conflicting_duplicate_decode_critical_tags() {
        for (tag, kind, value) in [
            (TAG_NEW_SUBFILE_TYPE, 4, 0),
            (TAG_LINEARIZATION_TABLE, 3, 1),
            (TAG_TILE_OFFSETS, 4, 240),
            (TAG_COMPRESSION, 3, 7),
        ] {
            for duplicate in [value, value + 1] {
                assert_eq!(
                    probe(
                        &dng_with_duplicate_sub_ifd_tag(tag, kind, value, duplicate),
                        DecodeLimits::default()
                    ),
                    Err(DecodeError::InvalidTag),
                    "tag {tag}, values {value} and {duplicate}"
                );
            }
        }
    }

    #[test]
    fn rejects_duplicate_unknown_tags() {
        assert_eq!(
            probe(
                &dng_with_duplicate_sub_ifd_tag(65_000, 1, 1, 2),
                DecodeLimits::default()
            ),
            Err(DecodeError::InvalidTag)
        );
    }

    #[test]
    fn ignores_unknown_metadata_and_supports_ifd_pointers() {
        let mut bytes = dng(ByteOrder::LittleEndian, 13);
        put16(&mut bytes, 100, 9, ByteOrder::LittleEndian);
        entry(
            &mut bytes,
            198,
            700,
            1,
            100_000,
            250,
            ByteOrder::LittleEndian,
        );
        put32(&mut bytes, 210, 0, ByteOrder::LittleEndian);
        assert!(facts(&bytes).is_linear_raw);
    }

    #[test]
    fn rejects_geometry_ambiguity_and_limits() {
        let mut zero = dng(ByteOrder::LittleEndian, 4);
        entry(
            &mut zero,
            114,
            TAG_IMAGE_WIDTH,
            4,
            1,
            0,
            ByteOrder::LittleEndian,
        );
        assert_eq!(
            probe(&zero, DecodeLimits::default()),
            Err(DecodeError::InvalidGeometry)
        );
        assert_eq!(
            probe(
                &dng(ByteOrder::LittleEndian, 4),
                DecodeLimits::default().with_max_dimensions(100, 100)
            ),
            Err(DecodeError::ResourceLimit)
        );
        assert_eq!(
            probe(
                &dng(ByteOrder::LittleEndian, 4),
                DecodeLimits::default().with_max_ifds(1)
            ),
            Err(DecodeError::ResourceLimit)
        );
        assert_eq!(
            probe(
                &dng(ByteOrder::LittleEndian, 4),
                DecodeLimits::default().with_max_pixels(1)
            ),
            Err(DecodeError::ResourceLimit)
        );
    }

    #[test]
    fn rejects_ambiguous_primary_raw_ifds_and_cycles() {
        let mut bytes = dng(ByteOrder::LittleEndian, 4);
        bytes.resize(400, 0);
        put32(&mut bytes, 198, 240, ByteOrder::LittleEndian);
        put16(&mut bytes, 240, 6, ByteOrder::LittleEndian);
        entry(
            &mut bytes,
            242,
            TAG_NEW_SUBFILE_TYPE,
            4,
            1,
            0,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            254,
            TAG_IMAGE_WIDTH,
            4,
            1,
            10,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            266,
            TAG_IMAGE_LENGTH,
            4,
            1,
            10,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            278,
            TAG_BITS_PER_SAMPLE,
            3,
            3,
            330,
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            290,
            TAG_PHOTOMETRIC,
            3,
            1,
            u32::from(PHOTOMETRIC_LINEAR_RAW),
            ByteOrder::LittleEndian,
        );
        entry(
            &mut bytes,
            302,
            TAG_SAMPLES_PER_PIXEL,
            3,
            1,
            3,
            ByteOrder::LittleEndian,
        );
        put32(&mut bytes, 314, 0, ByteOrder::LittleEndian);
        for index in 0..3 {
            put16(&mut bytes, 330 + index * 2, 12, ByteOrder::LittleEndian);
        }
        assert_eq!(
            probe(&bytes, DecodeLimits::default()),
            Err(DecodeError::InvalidGeometry)
        );
        let mut cycle = dng(ByteOrder::LittleEndian, 4);
        put32(&mut cycle, 198, 8, ByteOrder::LittleEndian);
        assert_eq!(
            probe(&cycle, DecodeLimits::default()),
            Err(DecodeError::InvalidOffset)
        );
    }

    #[test]
    fn classifies_non_dng_tiff_and_truncation() {
        let mut bytes = dng(ByteOrder::LittleEndian, 4);
        put16(&mut bytes, 10, 1, ByteOrder::LittleEndian);
        assert_eq!(
            probe(&bytes, DecodeLimits::default()),
            Ok(ContainerProbe::Unknown)
        );
        assert_eq!(
            probe(b"II", DecodeLimits::default()),
            Ok(ContainerProbe::Unknown)
        );
    }

    #[test]
    fn rejects_invalid_recognized_value_offsets() {
        let mut bytes = dng(ByteOrder::LittleEndian, 4);
        put32(&mut bytes, 146, 500, ByteOrder::LittleEndian);
        assert_eq!(
            probe(&bytes, DecodeLimits::default()),
            Err(DecodeError::InvalidOffset)
        );
    }

    #[test]
    fn rejects_invalid_sample_domains() {
        let mut samples = dng(ByteOrder::LittleEndian, 4);
        entry(
            &mut samples,
            162,
            TAG_SAMPLES_PER_PIXEL,
            3,
            1,
            0,
            ByteOrder::LittleEndian,
        );
        assert_eq!(
            probe(&samples, DecodeLimits::default()),
            Err(DecodeError::InvalidTag)
        );
        let mut bits = dng(ByteOrder::LittleEndian, 4);
        put16(&mut bits, 220, 0, ByteOrder::LittleEndian);
        assert_eq!(
            probe(&bits, DecodeLimits::default()),
            Err(DecodeError::InvalidTag)
        );
        let mut format = dng(ByteOrder::LittleEndian, 4);
        put16(&mut format, 226, 7, ByteOrder::LittleEndian);
        assert_eq!(
            probe(&format, DecodeLimits::default()),
            Err(DecodeError::InvalidTag)
        );
        let mut compression = dng(ByteOrder::LittleEndian, 4);
        entry(
            &mut compression,
            174,
            TAG_COMPRESSION,
            3,
            1,
            0,
            ByteOrder::LittleEndian,
        );
        assert_eq!(
            probe(&compression, DecodeLimits::default()),
            Err(DecodeError::InvalidTag)
        );
        let mut planar = dng(ByteOrder::LittleEndian, 4);
        entry(
            &mut planar,
            174,
            TAG_PLANAR_CONFIGURATION,
            3,
            1,
            3,
            ByteOrder::LittleEndian,
        );
        assert_eq!(
            probe(&planar, DecodeLimits::default()),
            Err(DecodeError::InvalidTag)
        );
    }

    #[test]
    fn enforces_input_entry_and_value_limits() {
        let bytes = dng(ByteOrder::LittleEndian, 4);
        assert!(
            probe(
                &bytes,
                DecodeLimits::default().with_max_input_bytes(bytes.len())
            )
            .is_ok()
        );
        assert_eq!(
            probe(
                &bytes,
                DecodeLimits::default().with_max_input_bytes(bytes.len() - 1)
            ),
            Err(DecodeError::ResourceLimit)
        );
        assert_eq!(
            probe(&bytes, DecodeLimits::default().with_max_ifd_entries(1)),
            Ok(ContainerProbe::Unknown)
        );
        assert_eq!(
            probe(&bytes, DecodeLimits::default().with_max_value_bytes(5)),
            Err(DecodeError::ResourceLimit)
        );
    }

    #[test]
    fn recognizes_x3f_and_classifies_early_tiff_inputs_as_unknown() {
        assert_eq!(
            probe(b"FOVb\x01\x00\x00\x00", DecodeLimits::default()),
            Ok(ContainerProbe::X3f(X3fFacts { version: 1 }))
        );
        assert_eq!(
            probe(b"FOVb", DecodeLimits::default()),
            Err(DecodeError::Truncated)
        );
        let mut big_tiff = vec![0; 8];
        big_tiff[..2].copy_from_slice(b"II");
        put16(&mut big_tiff, 2, BIG_TIFF_MAGIC, ByteOrder::LittleEndian);
        assert_eq!(
            probe(&big_tiff, DecodeLimits::default()),
            Ok(ContainerProbe::Unknown)
        );
    }

    #[test]
    fn dng_prefixes_are_safe_and_unknown_before_recognition() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let bytes = dng(order, 4);
            for length in 0..=bytes.len() {
                let prefix = &bytes[..length];
                let result = probe(prefix, DecodeLimits::default());
                if recognize_dng(prefix, DecodeLimits::default()).is_none() {
                    assert_eq!(
                        result,
                        Ok(ContainerProbe::Unknown),
                        "{order:?} prefix {length}"
                    );
                }
            }
        }
    }

    #[test]
    fn defaults_bound_common_proraw_output() {
        let limits = DecodeLimits::default();
        assert_eq!(limits.max_pixels, 50 * 1024 * 1024);
        assert_eq!(limits.max_frame_samples, 160 * 1024 * 1024);
        assert_eq!(limits.max_decoded_samples, 160 * 1024 * 1024);
        assert!(48_000_000_u64 <= limits.max_pixels);
        assert!(144_000_000_u64 <= limits.max_decoded_samples);
    }
}
