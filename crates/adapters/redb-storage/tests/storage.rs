//! Persistence round-trip tests.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use redb_storage::{ApplicationSettings, ModelRecord, RedbStorage, StoredScalarType};

static NEXT_DATABASE: AtomicU64 = AtomicU64::new(0);

fn database_path() -> PathBuf {
    let identifier = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "llm-app-redb-storage-{}-{identifier}.redb",
        std::process::id()
    ))
}

#[test]
fn settings_and_catalogue_survive_round_trip() -> Result<(), Box<dyn Error>> {
    let path = database_path();
    {
        let storage = RedbStorage::open(&path)?;
        let settings = ApplicationSettings {
            default_repository: "acme/llama".to_owned(),
            default_revision: "main".to_owned(),
            maximum_host_memory_bytes: 8 * 1024 * 1024 * 1024,
            maximum_device_memory_bytes: 0,
            drain_timeout_milliseconds: 2_000,
        };
        storage.save_settings(&settings)?;
        assert_eq!(storage.load_settings()?, Some(settings));

        let model = ModelRecord {
            name: "local-llama".to_owned(),
            repository: "acme/llama".to_owned(),
            revision: "main".to_owned(),
            scalar_type: StoredScalarType::F32,
            last_used_unix_milliseconds: 42,
        };
        storage.upsert_model(&model)?;
        assert_eq!(storage.load_model("local-llama")?, Some(model.clone()));
        assert_eq!(storage.list_models()?, vec![model]);
        assert!(storage.remove_model("local-llama")?);
        assert!(storage.load_model("local-llama")?.is_none());
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}
