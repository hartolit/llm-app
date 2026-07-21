//! GGUF metadata inspection and source-bound tests.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::num::{NonZeroI32, NonZeroU32};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use domain_contracts::{ModelArchitecture, QuantizationFormat, ScalarType};
use gguf_backend::{
    GgufExecutionConfiguration, GgufInspectionLimits, MetadataError, inspect_metadata,
};

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[test]
fn inspects_required_transformer_metadata() -> TestResult {
    let path = write_fixture(3)?;
    let metadata = inspect_metadata(&path, GgufInspectionLimits::default())?;
    fs::remove_file(&path)?;

    assert_eq!(metadata.architecture_name(), "llama");
    assert_eq!(metadata.architecture(), ModelArchitecture::Llama);
    assert_eq!(metadata.scalar_type(), ScalarType::Other(7));
    assert_eq!(metadata.quantization(), QuantizationFormat::Gguf(7));
    assert_eq!(metadata.vocabulary_size(), 3);
    assert_eq!(metadata.context_length(), 4_096);
    assert_eq!(metadata.block_count(), 2);
    assert_eq!(metadata.embedding_length(), 16);
    assert_eq!(metadata.attention_head_count(), 4);
    assert_eq!(metadata.attention_head_count_kv(), 2);
    Ok(())
}

#[test]
fn rejects_dimensions_from_a_different_architecture_namespace() -> TestResult {
    let path = write_fixture_with_context_key(3, "mistral.context_length")?;
    let result = inspect_metadata(&path, GgufInspectionLimits::default());
    fs::remove_file(&path)?;
    assert!(matches!(
        result,
        Err(MetadataError::InvalidValue | MetadataError::InvalidFormat)
    ));
    Ok(())
}

#[test]
fn rejects_metadata_arrays_above_the_configured_bound() -> TestResult {
    let path = write_fixture(3)?;
    let result = inspect_metadata(
        &path,
        GgufInspectionLimits {
            maximum_header_bytes: 1_048_576,
            maximum_metadata_entries: 64,
            maximum_string_bytes: 1_024,
            maximum_array_elements: 2,
        },
    );
    fs::remove_file(&path)?;
    assert!(matches!(result, Err(MetadataError::LimitExceeded)));
    Ok(())
}

#[test]
fn rejects_headers_above_the_configured_byte_bound() -> TestResult {
    let path = write_fixture(3)?;
    let result = inspect_metadata(
        &path,
        GgufInspectionLimits {
            maximum_header_bytes: 16,
            ..GgufInspectionLimits::default()
        },
    );
    fs::remove_file(&path)?;
    assert!(matches!(result, Err(MetadataError::LimitExceeded)));
    Ok(())
}

#[test]
fn rejects_inconsistent_execution_bounds() -> TestResult {
    let result = GgufExecutionConfiguration::new(
        non_zero_u32(1_024)?,
        non_zero_u32(512)?,
        non_zero_u32(1_024)?,
        non_zero_u32(1)?,
        non_zero_i32(4)?,
        non_zero_i32(4)?,
    );
    assert!(result.is_err());
    Ok(())
}

#[test]
fn rejects_negative_native_thread_counts() -> TestResult {
    let result = GgufExecutionConfiguration::new(
        non_zero_u32(1_024)?,
        non_zero_u32(512)?,
        non_zero_u32(256)?,
        non_zero_u32(1)?,
        non_zero_i32(-1)?,
        non_zero_i32(4)?,
    );
    assert!(result.is_err());
    Ok(())
}

fn write_fixture(vocabulary_size: u64) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    write_fixture_with_context_key(vocabulary_size, "llama.context_length")
}

fn write_fixture_with_context_key(
    vocabulary_size: u64,
    context_key: &str,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"GGUF");
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&9_u64.to_le_bytes());

    write_string_value(&mut bytes, "general.architecture", "llama")?;
    write_u32_value(&mut bytes, "general.file_type", 7)?;
    write_u32_value(&mut bytes, context_key, 4_096)?;
    // This key deliberately ends in `context_length` but is not the model's
    // primary context field. Exact architecture-key matching must ignore it.
    write_u32_value(
        &mut bytes,
        "llama.rope.scaling.original_context_length",
        16_384,
    )?;
    write_u32_value(&mut bytes, "llama.block_count", 2)?;
    write_u32_value(&mut bytes, "llama.embedding_length", 16)?;
    write_u32_value(&mut bytes, "llama.attention.head_count", 4)?;
    write_u32_value(&mut bytes, "llama.attention.head_count_kv", 2)?;
    write_token_array(&mut bytes, vocabulary_size)?;

    let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "llm-app-gguf-metadata-{}-{sequence}.gguf",
        std::process::id()
    ));
    fs::write(&path, bytes)?;
    Ok(path)
}

fn write_key(output: &mut Vec<u8>, key: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let length = u64::try_from(key.len())?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(key.as_bytes());
    Ok(())
}

fn write_string_value(
    output: &mut Vec<u8>,
    key: &str,
    value: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    write_key(output, key)?;
    output.extend_from_slice(&8_u32.to_le_bytes());
    let length = u64::try_from(value.len())?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_u32_value(
    output: &mut Vec<u8>,
    key: &str,
    value: u32,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    write_key(output, key)?;
    output.extend_from_slice(&4_u32.to_le_bytes());
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_token_array(
    output: &mut Vec<u8>,
    vocabulary_size: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    write_key(output, "tokenizer.ggml.tokens")?;
    output.extend_from_slice(&9_u32.to_le_bytes());
    output.extend_from_slice(&8_u32.to_le_bytes());
    output.extend_from_slice(&vocabulary_size.to_le_bytes());
    for index in 0..vocabulary_size {
        let token = format!("token-{index}");
        let length = u64::try_from(token.len())?;
        output.extend_from_slice(&length.to_le_bytes());
        output.extend_from_slice(token.as_bytes());
    }
    Ok(())
}

fn non_zero_u32(value: u32) -> Result<NonZeroU32, TestInvariantError> {
    NonZeroU32::new(value).ok_or(TestInvariantError)
}

fn non_zero_i32(value: i32) -> Result<NonZeroI32, TestInvariantError> {
    NonZeroI32::new(value).ok_or(TestInvariantError)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TestInvariantError;

impl Display for TestInvariantError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("test fixture requested a zero non-zero integer")
    }
}

impl Error for TestInvariantError {}
