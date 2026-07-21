//! Bounded streaming inspection of the GGUF metadata header.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use domain_contracts::{ModelArchitecture, QuantizationFormat, ScalarType};

use crate::source::GgufInspectionLimits;

const GGUF_MAGIC: [u8; 4] = *b"GGUF";
const GGUF_VERSION_2: u32 = 2;
const GGUF_VERSION_3: u32 = 3;
const SKIP_BUFFER_BYTES: usize = 8 * 1024;
const SKIP_BUFFER_BYTES_U64: u64 = 8 * 1024;

const TYPE_UINT8: u32 = 0;
const TYPE_INT8: u32 = 1;
const TYPE_UINT16: u32 = 2;
const TYPE_INT16: u32 = 3;
const TYPE_UINT32: u32 = 4;
const TYPE_INT32: u32 = 5;
const TYPE_FLOAT32: u32 = 6;
const TYPE_BOOL: u32 = 7;
const TYPE_STRING: u32 = 8;
const TYPE_ARRAY: u32 = 9;
const TYPE_UINT64: u32 = 10;
const TYPE_INT64: u32 = 11;
const TYPE_FLOAT64: u32 = 12;

/// Stable metadata required for backend admission and context planning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GgufMetadata {
    architecture_name: String,
    architecture: ModelArchitecture,
    scalar_type: ScalarType,
    quantization: QuantizationFormat,
    vocabulary_size: u32,
    context_length: u32,
    block_count: u32,
    embedding_length: u32,
    attention_head_count: u32,
    attention_head_count_kv: u32,
    file_type: u16,
}

impl GgufMetadata {
    /// Returns the original GGUF architecture name.
    #[must_use]
    pub fn architecture_name(&self) -> &str {
        &self.architecture_name
    }

    /// Returns the portable architecture classification.
    #[must_use]
    pub const fn architecture(&self) -> ModelArchitecture {
        self.architecture
    }

    /// Returns the portable scalar representation.
    #[must_use]
    pub const fn scalar_type(&self) -> ScalarType {
        self.scalar_type
    }

    /// Returns the GGUF quantization description.
    #[must_use]
    pub const fn quantization(&self) -> QuantizationFormat {
        self.quantization
    }

    /// Returns the tokenizer vocabulary size.
    #[must_use]
    pub const fn vocabulary_size(&self) -> u32 {
        self.vocabulary_size
    }

    /// Returns the native model context length.
    #[must_use]
    pub const fn context_length(&self) -> u32 {
        self.context_length
    }

    /// Returns the transformer block count.
    #[must_use]
    pub const fn block_count(&self) -> u32 {
        self.block_count
    }

    /// Returns the embedding width.
    #[must_use]
    pub const fn embedding_length(&self) -> u32 {
        self.embedding_length
    }

    /// Returns the attention head count.
    #[must_use]
    pub const fn attention_head_count(&self) -> u32 {
        self.attention_head_count
    }

    /// Returns the key/value attention head count.
    #[must_use]
    pub const fn attention_head_count_kv(&self) -> u32 {
        self.attention_head_count_kv
    }

    /// Returns `general.file_type` as a stable GGUF code.
    #[must_use]
    pub const fn file_type(&self) -> u16 {
        self.file_type
    }

    pub(crate) fn cache_bytes_per_token(&self) -> Result<u64, MetadataError> {
        if self.attention_head_count == 0
            || !self
                .embedding_length
                .is_multiple_of(self.attention_head_count)
        {
            return Err(MetadataError::InvalidValue);
        }
        let head_width = u64::from(self.embedding_length / self.attention_head_count);
        let key_value_width = head_width
            .checked_mul(u64::from(self.attention_head_count_kv))
            .ok_or(MetadataError::NumericOverflow)?;
        // The loader fixes both native K and V cache types to F16. The first
        // factor of two accounts for K and V; the second is bytes per F16.
        u64::from(self.block_count)
            .checked_mul(key_value_width)
            .and_then(|elements| elements.checked_mul(2))
            .and_then(|elements| elements.checked_mul(2))
            .ok_or(MetadataError::NumericOverflow)
    }
}

/// Failure while reading a bounded GGUF metadata header.
#[derive(Debug)]
pub enum MetadataError {
    /// The model file could not be opened.
    Open(io::Error),
    /// The file ended early or an I/O operation failed.
    Read(io::Error),
    /// The magic, version, value type, or required metadata is invalid.
    InvalidFormat,
    /// A configured metadata limit was exceeded.
    LimitExceeded,
    /// A required numeric value is zero or internally inconsistent.
    InvalidValue,
    /// A conversion or size calculation overflowed.
    NumericOverflow,
    /// A required string is not valid UTF-8.
    InvalidUtf8,
}

impl Display for MetadataError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => write!(formatter, "failed to open GGUF file: {error}"),
            Self::Read(error) => write!(formatter, "failed to read GGUF metadata: {error}"),
            Self::InvalidFormat => formatter.write_str("GGUF metadata format is invalid"),
            Self::LimitExceeded => formatter.write_str("GGUF metadata inspection limit exceeded"),
            Self::InvalidValue => formatter.write_str("GGUF metadata value is invalid"),
            Self::NumericOverflow => formatter.write_str("GGUF metadata numeric value overflowed"),
            Self::InvalidUtf8 => formatter.write_str("GGUF metadata string is not valid UTF-8"),
        }
    }
}

impl Error for MetadataError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open(error) | Self::Read(error) => Some(error),
            Self::InvalidFormat
            | Self::LimitExceeded
            | Self::InvalidValue
            | Self::NumericOverflow
            | Self::InvalidUtf8 => None,
        }
    }
}

/// Inspects GGUF metadata without loading tensor data.
///
/// # Errors
///
/// Returns [`MetadataError`] when the file cannot be opened, metadata is
/// malformed, or a configured inspection bound is exceeded.
pub fn inspect_metadata(
    path: &Path,
    limits: GgufInspectionLimits,
) -> Result<GgufMetadata, MetadataError> {
    let file = File::open(path).map_err(MetadataError::Open)?;
    inspect_reader(BufReader::new(file), limits)
}

fn inspect_reader<R: Read>(
    reader: R,
    limits: GgufInspectionLimits,
) -> Result<GgufMetadata, MetadataError> {
    let mut reader = BoundedReader::new(reader, limits.maximum_header_bytes);
    let mut magic = [0_u8; 4];
    read_exact(&mut reader, &mut magic)?;
    if magic != GGUF_MAGIC {
        return Err(MetadataError::InvalidFormat);
    }
    let version = read_u32(&mut reader)?;
    if version != GGUF_VERSION_2 && version != GGUF_VERSION_3 {
        return Err(MetadataError::InvalidFormat);
    }
    let _tensor_count = read_u64(&mut reader)?;
    let metadata_count = read_u64(&mut reader)?;
    if metadata_count > limits.maximum_metadata_entries {
        return Err(MetadataError::LimitExceeded);
    }

    let mut collected = CollectedMetadata::default();
    for _ in 0..metadata_count {
        let key = read_string(&mut reader, limits.maximum_string_bytes)?;
        let value_type = read_u32(&mut reader)?;
        read_metadata_value(&mut reader, &key, value_type, limits, &mut collected)?;
    }
    collected.finish()
}

#[derive(Default)]
struct CollectedMetadata {
    architecture: Option<String>,
    context_length: Option<ArchitectureValue>,
    block_count: Option<ArchitectureValue>,
    embedding_length: Option<ArchitectureValue>,
    attention_head_count: Option<ArchitectureValue>,
    attention_head_count_kv: Option<ArchitectureValue>,
    vocabulary_size: Option<u32>,
    file_type: Option<u16>,
}

struct ArchitectureValue {
    architecture: String,
    value: u32,
}

impl CollectedMetadata {
    fn finish(self) -> Result<GgufMetadata, MetadataError> {
        let architecture_name = self.architecture.ok_or(MetadataError::InvalidFormat)?;
        let context_length = required_architecture_value(self.context_length, &architecture_name)?;
        let block_count = required_architecture_value(self.block_count, &architecture_name)?;
        let embedding_length =
            required_architecture_value(self.embedding_length, &architecture_name)?;
        let attention_head_count =
            required_architecture_value(self.attention_head_count, &architecture_name)?;
        let attention_head_count_kv = match self.attention_head_count_kv {
            Some(value) => required_architecture_value(Some(value), &architecture_name)?,
            None => attention_head_count,
        };
        if attention_head_count_kv > attention_head_count
            || !attention_head_count.is_multiple_of(attention_head_count_kv)
        {
            return Err(MetadataError::InvalidValue);
        }
        let vocabulary_size = required_non_zero(self.vocabulary_size)?;
        let file_type = self.file_type.ok_or(MetadataError::InvalidFormat)?;
        let (scalar_type, quantization) = scalar_and_quantization(file_type);

        Ok(GgufMetadata {
            architecture: classify_architecture(&architecture_name),
            architecture_name,
            scalar_type,
            quantization,
            vocabulary_size,
            context_length,
            block_count,
            embedding_length,
            attention_head_count,
            attention_head_count_kv,
            file_type,
        })
    }
}

fn required_architecture_value(
    value: Option<ArchitectureValue>,
    architecture: &str,
) -> Result<u32, MetadataError> {
    let value = value.ok_or(MetadataError::InvalidFormat)?;
    if value.architecture != architecture || value.value == 0 {
        return Err(MetadataError::InvalidValue);
    }
    Ok(value.value)
}

const fn required_non_zero(value: Option<u32>) -> Result<u32, MetadataError> {
    match value {
        Some(value) if value != 0 => Ok(value),
        _ => Err(MetadataError::InvalidValue),
    }
}

fn read_metadata_value<R: Read>(
    reader: &mut R,
    key: &str,
    value_type: u32,
    limits: GgufInspectionLimits,
    collected: &mut CollectedMetadata,
) -> Result<(), MetadataError> {
    if key == "general.architecture" {
        if value_type != TYPE_STRING || collected.architecture.is_some() {
            return Err(MetadataError::InvalidFormat);
        }
        collected.architecture = Some(read_string(reader, limits.maximum_string_bytes)?);
        return Ok(());
    }
    if key == "general.file_type" {
        if collected.file_type.is_some() {
            return Err(MetadataError::InvalidFormat);
        }
        let value = read_integer(reader, value_type)?;
        collected.file_type =
            Some(u16::try_from(value).map_err(|_| MetadataError::NumericOverflow)?);
        return Ok(());
    }
    if key == "tokenizer.ggml.tokens" {
        return read_token_array(reader, value_type, limits, collected);
    }

    let numeric_target = if let Some(architecture) = architecture_prefix(key, ".context_length") {
        Some((architecture, &mut collected.context_length))
    } else if let Some(architecture) = architecture_prefix(key, ".block_count") {
        Some((architecture, &mut collected.block_count))
    } else if let Some(architecture) = architecture_prefix(key, ".embedding_length") {
        Some((architecture, &mut collected.embedding_length))
    } else if let Some(architecture) = architecture_prefix(key, ".attention.head_count_kv") {
        Some((architecture, &mut collected.attention_head_count_kv))
    } else if let Some(architecture) = architecture_prefix(key, ".attention.head_count") {
        Some((architecture, &mut collected.attention_head_count))
    } else {
        None
    };
    if let Some((architecture, target)) = numeric_target {
        let value = read_integer(reader, value_type)?;
        store_architecture_value(target, architecture, value)?;
        return Ok(());
    }

    skip_value(reader, value_type, limits)
}

fn architecture_prefix<'a>(key: &'a str, suffix: &str) -> Option<&'a str> {
    key.strip_suffix(suffix)
        .filter(|architecture| !architecture.is_empty() && !architecture.contains('.'))
}

fn store_architecture_value(
    target: &mut Option<ArchitectureValue>,
    architecture: &str,
    value: u64,
) -> Result<(), MetadataError> {
    if target.is_some() {
        return Err(MetadataError::InvalidFormat);
    }
    *target = Some(ArchitectureValue {
        architecture: architecture.to_owned(),
        value: u32::try_from(value).map_err(|_| MetadataError::NumericOverflow)?,
    });
    Ok(())
}

fn read_token_array<R: Read>(
    reader: &mut R,
    value_type: u32,
    limits: GgufInspectionLimits,
    collected: &mut CollectedMetadata,
) -> Result<(), MetadataError> {
    if value_type != TYPE_ARRAY {
        return Err(MetadataError::InvalidFormat);
    }
    let element_type = read_u32(reader)?;
    let count = read_u64(reader)?;
    validate_array_count(count, limits)?;
    if element_type != TYPE_STRING {
        return Err(MetadataError::InvalidFormat);
    }
    if collected.vocabulary_size.is_some() {
        return Err(MetadataError::InvalidFormat);
    }
    collected.vocabulary_size =
        Some(u32::try_from(count).map_err(|_| MetadataError::NumericOverflow)?);
    for _ in 0..count {
        skip_string(reader, limits.maximum_string_bytes)?;
    }
    Ok(())
}

fn skip_value<R: Read>(
    reader: &mut R,
    value_type: u32,
    limits: GgufInspectionLimits,
) -> Result<(), MetadataError> {
    match fixed_width(value_type) {
        Some(width) => skip_bytes(reader, width),
        None if value_type == TYPE_STRING => skip_string(reader, limits.maximum_string_bytes),
        None if value_type == TYPE_ARRAY => {
            let element_type = read_u32(reader)?;
            if element_type == TYPE_ARRAY {
                return Err(MetadataError::InvalidFormat);
            }
            let count = read_u64(reader)?;
            validate_array_count(count, limits)?;
            match fixed_width(element_type) {
                Some(width) => {
                    let bytes = count
                        .checked_mul(width)
                        .ok_or(MetadataError::NumericOverflow)?;
                    skip_bytes(reader, bytes)
                }
                None if element_type == TYPE_STRING => {
                    for _ in 0..count {
                        skip_string(reader, limits.maximum_string_bytes)?;
                    }
                    Ok(())
                }
                _ => Err(MetadataError::InvalidFormat),
            }
        }
        _ => Err(MetadataError::InvalidFormat),
    }
}

fn read_integer<R: Read>(reader: &mut R, value_type: u32) -> Result<u64, MetadataError> {
    match value_type {
        TYPE_UINT8 => Ok(u64::from(read_u8(reader)?)),
        TYPE_UINT16 => Ok(u64::from(read_u16(reader)?)),
        TYPE_UINT32 => Ok(u64::from(read_u32(reader)?)),
        TYPE_UINT64 => read_u64(reader),
        TYPE_INT8 => signed_to_u64(i64::from(read_i8(reader)?)),
        TYPE_INT16 => signed_to_u64(i64::from(read_i16(reader)?)),
        TYPE_INT32 => signed_to_u64(i64::from(read_i32(reader)?)),
        TYPE_INT64 => signed_to_u64(read_i64(reader)?),
        _ => Err(MetadataError::InvalidFormat),
    }
}

fn signed_to_u64(value: i64) -> Result<u64, MetadataError> {
    u64::try_from(value).map_err(|_| MetadataError::InvalidValue)
}

const fn fixed_width(value_type: u32) -> Option<u64> {
    match value_type {
        TYPE_UINT8 | TYPE_INT8 | TYPE_BOOL => Some(1),
        TYPE_UINT16 | TYPE_INT16 => Some(2),
        TYPE_UINT32 | TYPE_INT32 | TYPE_FLOAT32 => Some(4),
        TYPE_UINT64 | TYPE_INT64 | TYPE_FLOAT64 => Some(8),
        _ => None,
    }
}

const fn validate_array_count(
    count: u64,
    limits: GgufInspectionLimits,
) -> Result<(), MetadataError> {
    if count > limits.maximum_array_elements {
        return Err(MetadataError::LimitExceeded);
    }
    Ok(())
}

fn read_string<R: Read>(reader: &mut R, maximum: u64) -> Result<String, MetadataError> {
    let length = read_u64(reader)?;
    if length > maximum {
        return Err(MetadataError::LimitExceeded);
    }
    let length = usize::try_from(length).map_err(|_| MetadataError::NumericOverflow)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| MetadataError::LimitExceeded)?;
    bytes.resize(length, 0);
    read_exact(reader, &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| MetadataError::InvalidUtf8)
}

fn skip_string<R: Read>(reader: &mut R, maximum: u64) -> Result<(), MetadataError> {
    let length = read_u64(reader)?;
    if length > maximum {
        return Err(MetadataError::LimitExceeded);
    }
    skip_bytes(reader, length)
}

fn skip_bytes<R: Read>(reader: &mut R, mut remaining: u64) -> Result<(), MetadataError> {
    let mut buffer = [0_u8; SKIP_BUFFER_BYTES];
    while remaining != 0 {
        let chunk = usize::try_from(remaining.min(SKIP_BUFFER_BYTES_U64))
            .map_err(|_| MetadataError::NumericOverflow)?;
        let Some(output) = buffer.get_mut(..chunk) else {
            return Err(MetadataError::NumericOverflow);
        };
        read_exact(reader, output)?;
        remaining = remaining
            .checked_sub(u64::try_from(chunk).map_err(|_| MetadataError::NumericOverflow)?)
            .ok_or(MetadataError::NumericOverflow)?;
    }
    Ok(())
}

fn read_exact<R: Read>(reader: &mut R, output: &mut [u8]) -> Result<(), MetadataError> {
    reader.read_exact(output).map_err(|error| {
        if error
            .get_ref()
            .is_some_and(<dyn Error + Send + Sync + 'static>::is::<HeaderLimitReached>)
        {
            MetadataError::LimitExceeded
        } else {
            MetadataError::Read(error)
        }
    })
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, MetadataError> {
    let mut bytes = [0_u8; 1];
    read_exact(reader, &mut bytes)?;
    Ok(u8::from_le_bytes(bytes))
}

fn read_i8<R: Read>(reader: &mut R) -> Result<i8, MetadataError> {
    let mut bytes = [0_u8; 1];
    read_exact(reader, &mut bytes)?;
    Ok(i8::from_le_bytes(bytes))
}

fn read_u16<R: Read>(reader: &mut R) -> Result<u16, MetadataError> {
    let mut bytes = [0_u8; 2];
    read_exact(reader, &mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_i16<R: Read>(reader: &mut R) -> Result<i16, MetadataError> {
    let mut bytes = [0_u8; 2];
    read_exact(reader, &mut bytes)?;
    Ok(i16::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, MetadataError> {
    let mut bytes = [0_u8; 4];
    read_exact(reader, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32<R: Read>(reader: &mut R) -> Result<i32, MetadataError> {
    let mut bytes = [0_u8; 4];
    read_exact(reader, &mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, MetadataError> {
    let mut bytes = [0_u8; 8];
    read_exact(reader, &mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_i64<R: Read>(reader: &mut R) -> Result<i64, MetadataError> {
    let mut bytes = [0_u8; 8];
    read_exact(reader, &mut bytes)?;
    Ok(i64::from_le_bytes(bytes))
}

fn classify_architecture(name: &str) -> ModelArchitecture {
    match name {
        "llama" => ModelArchitecture::Llama,
        "mistral" | "mixtral" => ModelArchitecture::Mistral,
        "gemma" | "gemma2" | "gemma3" => ModelArchitecture::Gemma,
        "qwen" | "qwen2" | "qwen3" => ModelArchitecture::Qwen,
        other => ModelArchitecture::Other(fnv1a_32(other.as_bytes())),
    }
}

const fn scalar_and_quantization(file_type: u16) -> (ScalarType, QuantizationFormat) {
    match file_type {
        0 => (ScalarType::F32, QuantizationFormat::None),
        1 => (ScalarType::F16, QuantizationFormat::None),
        other => (ScalarType::Other(other), QuantizationFormat::Gguf(other)),
    }
}

fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

struct BoundedReader<R> {
    inner: R,
    remaining: u64,
}

impl<R> BoundedReader<R> {
    const fn new(inner: R, maximum_bytes: u64) -> Self {
        Self {
            inner,
            remaining: maximum_bytes,
        }
    }
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(io::Error::other(HeaderLimitReached));
        }
        let requested = u64::try_from(output.len()).unwrap_or(u64::MAX);
        let allowed = usize::try_from(self.remaining.min(requested)).map_err(io::Error::other)?;
        let Some(output) = output.get_mut(..allowed) else {
            return Err(io::Error::other(HeaderLimitReached));
        };
        let read = self.inner.read(output)?;
        self.remaining = self
            .remaining
            .checked_sub(u64::try_from(read).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other(HeaderLimitReached))?;
        Ok(read)
    }
}

#[derive(Debug)]
struct HeaderLimitReached;

impl Display for HeaderLimitReached {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("GGUF header inspection byte limit reached")
    }
}

impl Error for HeaderLimitReached {}
