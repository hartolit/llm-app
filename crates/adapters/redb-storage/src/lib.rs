//! Versioned desktop persistence over redb.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

const SETTINGS_KEY: &str = "application";
const SETTINGS_MAGIC: [u8; 4] = *b"LAS1";
const MODEL_MAGIC: [u8; 4] = *b"LAM1";
const RECORD_VERSION: u16 = 1;
const SETTINGS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("application_settings_v1");
const MODELS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("model_catalogue_v1");

/// Persisted application-level runtime limits and default Hub selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationSettings {
    /// Default Hub repository presented by the desktop application.
    pub default_repository: String,
    /// Default Hub revision.
    pub default_revision: String,
    /// Aggregate host-memory admission bound.
    pub maximum_host_memory_bytes: u64,
    /// Aggregate device-memory admission bound.
    pub maximum_device_memory_bytes: u64,
    /// Mandatory drain timeout used before forced cancellation.
    pub drain_timeout_milliseconds: u64,
}

impl ApplicationSettings {
    /// Validates semantic fields before persistence.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidField`] when the repository or revision is
    /// empty or consists only of whitespace, or when the drain timeout is zero.
    pub fn validate(&self) -> Result<(), StorageError> {
        validate_non_empty(&self.default_repository, Field::Repository)?;
        validate_non_empty(&self.default_revision, Field::Revision)?;
        if self.drain_timeout_milliseconds == 0 {
            return Err(StorageError::InvalidField(Field::DrainTimeout));
        }
        Ok(())
    }
}

/// Scalar format selected when loading one catalogue entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoredScalarType {
    /// IEEE-754 32-bit floating point.
    F32,
    /// IEEE-754 16-bit floating point.
    F16,
    /// Brain floating point.
    Bf16,
}

/// Persisted logical model selection. Cache paths are deliberately not stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRecord {
    /// User-visible unique catalogue name.
    pub name: String,
    /// Hugging Face repository.
    pub repository: String,
    /// Branch, tag, reference, or commit.
    pub revision: String,
    /// Scalar format requested from the backend.
    pub scalar_type: StoredScalarType,
    /// Last successful use in Unix milliseconds, supplied by the application.
    pub last_used_unix_milliseconds: u64,
}

impl ModelRecord {
    /// Validates catalogue identity and Hub selection.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidField`] when the name, repository, or
    /// revision is empty or consists only of whitespace.
    pub fn validate(&self) -> Result<(), StorageError> {
        validate_non_empty(&self.name, Field::ModelName)?;
        validate_non_empty(&self.repository, Field::Repository)?;
        validate_non_empty(&self.revision, Field::Revision)
    }
}

/// Invalid persisted field category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// Model catalogue name.
    ModelName,
    /// Hub repository identifier.
    Repository,
    /// Hub revision identifier.
    Revision,
    /// Drain timeout.
    DrainTimeout,
}

/// Stable storage adapter failure.
#[derive(Debug)]
pub enum StorageError {
    /// redb operation failed.
    Database(redb::Error),
    /// A required semantic field was empty or invalid.
    InvalidField(Field),
    /// Record header did not match its table.
    InvalidRecordKind,
    /// Record version is newer or otherwise unsupported.
    UnsupportedVersion(u16),
    /// Record ended before all declared values were available.
    TruncatedRecord,
    /// A string length exceeded the binary record limit.
    StringTooLong {
        /// UTF-8 byte length that did not fit the persistent format.
        length: usize,
    },
    /// Persisted bytes were not valid UTF-8.
    InvalidUtf8(std::string::FromUtf8Error),
    /// Persisted scalar code is unknown.
    InvalidScalarType(u8),
    /// Bytes remained after a complete versioned record was decoded.
    TrailingBytes,
}

impl Display for StorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "database operation failed: {error}"),
            Self::InvalidField(field) => write!(formatter, "invalid storage field: {field:?}"),
            Self::InvalidRecordKind => formatter.write_str("invalid persistent record kind"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported persistent record version: {version}"
                )
            }
            Self::TruncatedRecord => formatter.write_str("persistent record is truncated"),
            Self::StringTooLong { length } => {
                write!(
                    formatter,
                    "persistent string has {length} bytes and exceeds u32"
                )
            }
            Self::InvalidUtf8(error) => {
                write!(formatter, "persistent string is invalid UTF-8: {error}")
            }
            Self::InvalidScalarType(code) => {
                write!(formatter, "unknown persistent scalar type: {code}")
            }
            Self::TrailingBytes => formatter.write_str("persistent record contains trailing bytes"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidUtf8(error) => Some(error),
            Self::InvalidField(_)
            | Self::InvalidRecordKind
            | Self::UnsupportedVersion(_)
            | Self::TruncatedRecord
            | Self::StringTooLong { .. }
            | Self::InvalidScalarType(_)
            | Self::TrailingBytes => None,
        }
    }
}

/// Open redb-backed desktop state.
pub struct RedbStorage {
    database: Database,
}

impl RedbStorage {
    /// Opens or creates the database file and initializes the versioned tables.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] if the database cannot be created or
    /// opened, either table cannot be initialized, or the initialization
    /// transaction cannot be committed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let database =
            Database::create(path).map_err(|error| StorageError::Database(error.into()))?;
        let write = database
            .begin_write()
            .map_err(|error| StorageError::Database(error.into()))?;
        {
            let _settings = write
                .open_table(SETTINGS_TABLE)
                .map_err(|error| StorageError::Database(error.into()))?;
            let _models = write
                .open_table(MODELS_TABLE)
                .map_err(|error| StorageError::Database(error.into()))?;
        }
        write
            .commit()
            .map_err(|error| StorageError::Database(error.into()))?;
        Ok(Self { database })
    }

    /// Atomically replaces the application settings record.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidField`] if `settings` fails validation,
    /// [`StorageError::StringTooLong`] if a persisted string exceeds the binary
    /// format's length limit, or [`StorageError::Database`] if the write fails.
    pub fn save_settings(&self, settings: &ApplicationSettings) -> Result<(), StorageError> {
        settings.validate()?;
        let encoded = encode_settings(settings)?;
        self.insert(SETTINGS_TABLE, SETTINGS_KEY, encoded.as_slice())
    }

    /// Reads the application settings when present.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] if the read fails. Returns a record
    /// decoding or validation error if the stored settings bytes are malformed,
    /// unsupported, or contain invalid fields.
    pub fn load_settings(&self) -> Result<Option<ApplicationSettings>, StorageError> {
        let Some(bytes) = self.get(SETTINGS_TABLE, SETTINGS_KEY)? else {
            return Ok(None);
        };
        decode_settings(bytes.as_slice()).map(Some)
    }

    /// Atomically inserts or replaces one model catalogue entry.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidField`] if `record` fails validation,
    /// [`StorageError::StringTooLong`] if a persisted string exceeds the binary
    /// format's length limit, or [`StorageError::Database`] if the write fails.
    pub fn upsert_model(&self, record: &ModelRecord) -> Result<(), StorageError> {
        record.validate()?;
        let encoded = encode_model(record)?;
        self.insert(MODELS_TABLE, record.name.as_str(), encoded.as_slice())
    }

    /// Reads one model catalogue entry by its exact logical name.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidField`] if `name` is empty or consists
    /// only of whitespace, or [`StorageError::Database`] if the read fails.
    /// Returns a record decoding or validation error if the stored model bytes
    /// are malformed, unsupported, or contain invalid fields.
    pub fn load_model(&self, name: &str) -> Result<Option<ModelRecord>, StorageError> {
        validate_non_empty(name, Field::ModelName)?;
        let Some(bytes) = self.get(MODELS_TABLE, name)? else {
            return Ok(None);
        };
        decode_model(bytes.as_slice()).map(Some)
    }

    /// Removes one catalogue entry and reports whether it existed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidField`] if `name` is empty or consists
    /// only of whitespace. Returns [`StorageError::Database`] if the removal or
    /// its transaction fails.
    pub fn remove_model(&self, name: &str) -> Result<bool, StorageError> {
        validate_non_empty(name, Field::ModelName)?;
        let write = self
            .database
            .begin_write()
            .map_err(|error| StorageError::Database(error.into()))?;
        let removed = {
            let mut table = write
                .open_table(MODELS_TABLE)
                .map_err(|error| StorageError::Database(error.into()))?;
            table
                .remove(name)
                .map_err(|error| StorageError::Database(error.into()))?
                .is_some()
        };
        write
            .commit()
            .map_err(|error| StorageError::Database(error.into()))?;
        Ok(removed)
    }

    /// Reads all catalogue entries in redb key order.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] if opening or iterating the table
    /// fails. Returns a record decoding or validation error if any stored model
    /// is malformed, unsupported, or contains invalid fields.
    pub fn list_models(&self) -> Result<Vec<ModelRecord>, StorageError> {
        let read = self
            .database
            .begin_read()
            .map_err(|error| StorageError::Database(error.into()))?;
        let table = read
            .open_table(MODELS_TABLE)
            .map_err(|error| StorageError::Database(error.into()))?;
        let mut records = Vec::new();
        let iterator = table
            .iter()
            .map_err(|error| StorageError::Database(error.into()))?;
        for entry in iterator {
            let (_key, value) = entry.map_err(|error| StorageError::Database(error.into()))?;
            records.push(decode_model(value.value())?);
        }
        Ok(records)
    }

    fn insert(
        &self,
        definition: TableDefinition<&str, &[u8]>,
        key: &str,
        value: &[u8],
    ) -> Result<(), StorageError> {
        let write = self
            .database
            .begin_write()
            .map_err(|error| StorageError::Database(error.into()))?;
        {
            let mut table = write
                .open_table(definition)
                .map_err(|error| StorageError::Database(error.into()))?;
            table
                .insert(key, value)
                .map_err(|error| StorageError::Database(error.into()))?;
        }
        write
            .commit()
            .map_err(|error| StorageError::Database(error.into()))
    }

    fn get(
        &self,
        definition: TableDefinition<&str, &[u8]>,
        key: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let read = self
            .database
            .begin_read()
            .map_err(|error| StorageError::Database(error.into()))?;
        let table = read
            .open_table(definition)
            .map_err(|error| StorageError::Database(error.into()))?;
        let value = table
            .get(key)
            .map_err(|error| StorageError::Database(error.into()))?;
        Ok(value.map(|guard| guard.value().to_vec()))
    }
}

fn encode_settings(settings: &ApplicationSettings) -> Result<Vec<u8>, StorageError> {
    let mut output = Vec::new();
    output.extend_from_slice(&SETTINGS_MAGIC);
    output.extend_from_slice(&RECORD_VERSION.to_le_bytes());
    encode_string(&mut output, &settings.default_repository)?;
    encode_string(&mut output, &settings.default_revision)?;
    output.extend_from_slice(&settings.maximum_host_memory_bytes.to_le_bytes());
    output.extend_from_slice(&settings.maximum_device_memory_bytes.to_le_bytes());
    output.extend_from_slice(&settings.drain_timeout_milliseconds.to_le_bytes());
    Ok(output)
}

fn decode_settings(bytes: &[u8]) -> Result<ApplicationSettings, StorageError> {
    let mut decoder = Decoder::new(bytes, SETTINGS_MAGIC)?;
    let settings = ApplicationSettings {
        default_repository: decoder.string()?,
        default_revision: decoder.string()?,
        maximum_host_memory_bytes: decoder.u64()?,
        maximum_device_memory_bytes: decoder.u64()?,
        drain_timeout_milliseconds: decoder.u64()?,
    };
    decoder.finish()?;
    settings.validate()?;
    Ok(settings)
}

fn encode_model(record: &ModelRecord) -> Result<Vec<u8>, StorageError> {
    let mut output = Vec::new();
    output.extend_from_slice(&MODEL_MAGIC);
    output.extend_from_slice(&RECORD_VERSION.to_le_bytes());
    encode_string(&mut output, &record.name)?;
    encode_string(&mut output, &record.repository)?;
    encode_string(&mut output, &record.revision)?;
    output.push(match record.scalar_type {
        StoredScalarType::F32 => 0,
        StoredScalarType::F16 => 1,
        StoredScalarType::Bf16 => 2,
    });
    output.extend_from_slice(&record.last_used_unix_milliseconds.to_le_bytes());
    Ok(output)
}

fn decode_model(bytes: &[u8]) -> Result<ModelRecord, StorageError> {
    let mut decoder = Decoder::new(bytes, MODEL_MAGIC)?;
    let record = ModelRecord {
        name: decoder.string()?,
        repository: decoder.string()?,
        revision: decoder.string()?,
        scalar_type: match decoder.u8()? {
            0 => StoredScalarType::F32,
            1 => StoredScalarType::F16,
            2 => StoredScalarType::Bf16,
            code => return Err(StorageError::InvalidScalarType(code)),
        },
        last_used_unix_milliseconds: decoder.u64()?,
    };
    decoder.finish()?;
    record.validate()?;
    Ok(record)
}

fn validate_non_empty(value: &str, field: Field) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        Err(StorageError::InvalidField(field))
    } else {
        Ok(())
    }
}

fn encode_string(output: &mut Vec<u8>, value: &str) -> Result<(), StorageError> {
    let length = u32::try_from(value.len()).map_err(|_| StorageError::StringTooLong {
        length: value.len(),
    })?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Decoder<'record> {
    bytes: &'record [u8],
    offset: usize,
}

impl<'record> Decoder<'record> {
    fn new(bytes: &'record [u8], expected_magic: [u8; 4]) -> Result<Self, StorageError> {
        let magic = bytes.get(..4).ok_or(StorageError::TruncatedRecord)?;
        if magic != expected_magic {
            return Err(StorageError::InvalidRecordKind);
        }
        let version_bytes: [u8; 2] = bytes
            .get(4..6)
            .ok_or(StorageError::TruncatedRecord)?
            .try_into()
            .map_err(|_| StorageError::TruncatedRecord)?;
        let version = u16::from_le_bytes(version_bytes);
        if version != RECORD_VERSION {
            return Err(StorageError::UnsupportedVersion(version));
        }
        Ok(Self { bytes, offset: 6 })
    }

    fn u8(&mut self) -> Result<u8, StorageError> {
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or(StorageError::TruncatedRecord)?;
        self.offset = self
            .offset
            .checked_add(1)
            .ok_or(StorageError::TruncatedRecord)?;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, StorageError> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or(StorageError::TruncatedRecord)?;
        let value: [u8; 4] = self
            .bytes
            .get(self.offset..end)
            .ok_or(StorageError::TruncatedRecord)?
            .try_into()
            .map_err(|_| StorageError::TruncatedRecord)?;
        self.offset = end;
        Ok(u32::from_le_bytes(value))
    }

    fn u64(&mut self) -> Result<u64, StorageError> {
        let end = self
            .offset
            .checked_add(8)
            .ok_or(StorageError::TruncatedRecord)?;
        let value: [u8; 8] = self
            .bytes
            .get(self.offset..end)
            .ok_or(StorageError::TruncatedRecord)?
            .try_into()
            .map_err(|_| StorageError::TruncatedRecord)?;
        self.offset = end;
        Ok(u64::from_le_bytes(value))
    }

    fn string(&mut self) -> Result<String, StorageError> {
        let length = usize::try_from(self.u32()?).map_err(|_| StorageError::TruncatedRecord)?;
        let end = self
            .offset
            .checked_add(length)
            .ok_or(StorageError::TruncatedRecord)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(StorageError::TruncatedRecord)?;
        self.offset = end;
        String::from_utf8(bytes.to_vec()).map_err(StorageError::InvalidUtf8)
    }

    const fn finish(self) -> Result<(), StorageError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(StorageError::TrailingBytes)
        }
    }
}
