//! End-to-end compatibility tests for the Candle CPU Llama adapter.

use std::collections::HashMap;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use candle_backend::{
    CandleLlamaLoader, CandleLlamaModel, CandleLlamaSequence, CandleLlamaSource, CandleScalarType,
};
use candle_core::{DType, Device, Tensor};
use domain_contracts::{
    BackendId, BackendSequence, CancellationReason, CancellationStatus, CapabilitySet,
    DecodeBuffers, DecodeInput, DecodeOutcome, DeviceId, DeviceKind, DrainTimeout, LifecycleAction,
    LoadConfiguration, LoadedModel, MemoryBudget, ModelDescriptor, ModelGeneration, ModelHandle,
    ModelId, ModelLifecycle, ModelLoader, MonotonicMillis, PrefillBuffers, PrefillInput,
    PrefillOutcome, SequenceConfiguration, SequenceId, SequenceState, TokenId, UnloadPolicy,
    decode_checked, prefill_checked,
};

type TestResult = Result<(), &'static str>;

#[test]
fn loads_two_sequences_and_unloads_after_bounded_drain() -> TestResult {
    let fixture = TinyLlamaFixture::create()?;
    let source = fixture.source()?;
    let mut loader = CandleLlamaLoader::new(BackendId::new(1));
    let configuration = load_configuration();
    let descriptor = loader.inspect(&source).map_err(|_| "inspect model")?;
    assert_capabilities(&descriptor);

    let mut model = loader
        .load(&source, &configuration)
        .map_err(|_| "load model")?;
    let sequence_configuration = SequenceConfiguration::new(
        NonZeroU32::new(16).ok_or("maximum tokens")?,
        NonZeroU32::new(8).ok_or("maximum prefill")?,
    );
    let mut first = model
        .create_sequence(SequenceId::new(1), &sequence_configuration)
        .map_err(|_| "create first sequence")?;
    let mut second = model
        .create_sequence(SequenceId::new(2), &sequence_configuration)
        .map_err(|_| "create second sequence")?;

    exercise_sequences(&mut model, &mut first, &mut second)?;
    unload_after_bounded_drain(model, first, second)
}

fn assert_capabilities(descriptor: &ModelDescriptor) {
    let operations = descriptor.capabilities.operations;
    assert!(operations.contains(CapabilitySet::PREFILL));
    assert!(operations.contains(CapabilitySet::MULTIPLE_SEQUENCES));
    assert!(!operations.contains(CapabilitySet::ALLOCATION_FREE_HOT_PATH));
    assert!(!operations.contains(CapabilitySet::SEQUENCE_RESET));
}

fn exercise_sequences(
    model: &mut CandleLlamaModel,
    first: &mut CandleLlamaSequence,
    second: &mut CandleLlamaSequence,
) -> TestResult {
    let prompt = [TokenId::new(1), TokenId::new(2)];
    let mut first_logits = [0.0_f32; 16];
    let first_prefill = prefill_checked(
        model,
        first,
        PrefillInput::new(&prompt, true),
        PrefillBuffers::new(&mut first_logits),
        CancellationStatus::Running,
    )
    .map_err(|_| "prefill first sequence")?;
    assert_eq!(
        first_prefill,
        PrefillOutcome::Ready {
            consumed_tokens: 2,
            position: 2,
            logits_written: 16,
        }
    );

    let mut second_logits = [0.0_f32; 16];
    let second_prefill = prefill_checked(
        model,
        second,
        PrefillInput::new(&prompt, true),
        PrefillBuffers::new(&mut second_logits),
        CancellationStatus::Running,
    )
    .map_err(|_| "prefill second sequence")?;
    assert_eq!(
        second_prefill,
        PrefillOutcome::Ready {
            consumed_tokens: 2,
            position: 2,
            logits_written: 16,
        }
    );

    let first_decode = decode_checked(
        model,
        first,
        DecodeInput::new(TokenId::new(3)),
        DecodeBuffers::new(&mut first_logits),
        CancellationStatus::Running,
    )
    .map_err(|_| "decode first sequence")?;
    assert_eq!(
        first_decode,
        DecodeOutcome::Ready {
            position: 3,
            logits_written: 16,
        }
    );

    let second_decode = decode_checked(
        model,
        second,
        DecodeInput::new(TokenId::new(4)),
        DecodeBuffers::new(&mut second_logits),
        CancellationStatus::Running,
    )
    .map_err(|_| "decode second sequence")?;
    assert_eq!(
        second_decode,
        DecodeOutcome::Ready {
            position: 3,
            logits_written: 16,
        }
    );

    let cancelled_position = first.position();
    let cancelled = decode_checked(
        model,
        first,
        DecodeInput::new(TokenId::new(5)),
        DecodeBuffers::new(&mut first_logits),
        CancellationStatus::Requested(CancellationReason::UserRequested),
    )
    .map_err(|_| "cancel first sequence")?;
    assert_eq!(
        cancelled,
        DecodeOutcome::Finished(domain_contracts::FinishReason::Cancelled(
            CancellationReason::UserRequested,
        ))
    );
    assert_eq!(first.position(), cancelled_position);
    assert_eq!(
        model.reset_sequence(first),
        Err(domain_contracts::SequenceError::Unsupported)
    );
    assert_eq!(first.state(), SequenceState::Ready);
    assert_eq!(first.position(), cancelled_position);
    Ok(())
}

fn unload_after_bounded_drain(
    mut model: CandleLlamaModel,
    first: CandleLlamaSequence,
    second: CandleLlamaSequence,
) -> TestResult {
    let mut lifecycle = ModelLifecycle::new();
    lifecycle.begin_load().map_err(|_| "begin lifecycle load")?;
    lifecycle
        .complete_load()
        .map_err(|_| "complete lifecycle load")?;
    lifecycle
        .start_request()
        .map_err(|_| "start first request")?;
    lifecycle
        .start_request()
        .map_err(|_| "start second request")?;
    let timeout = DrainTimeout::from_millis(10).map_err(|_| "create drain timeout")?;
    let action = lifecycle
        .request_unload(UnloadPolicy::Drain { timeout }, MonotonicMillis::new(100))
        .map_err(|_| "request drain")?;
    assert_eq!(action, LifecycleAction::None);
    assert_eq!(
        lifecycle
            .poll(MonotonicMillis::new(109))
            .map_err(|_| "poll drain")?,
        LifecycleAction::None
    );
    assert_eq!(
        lifecycle
            .poll(MonotonicMillis::new(110))
            .map_err(|_| "expire drain")?,
        LifecycleAction::CancelActive {
            reason: CancellationReason::DrainTimeout,
        }
    );
    assert_eq!(
        lifecycle
            .finish_request()
            .map_err(|_| "finish first request")?,
        LifecycleAction::None
    );
    assert_eq!(
        lifecycle
            .finish_request()
            .map_err(|_| "finish second request")?,
        LifecycleAction::ReleaseModel
    );

    drop(first);
    drop(second);
    model.synchronize().map_err(|_| "synchronize model")?;
    model.prepare_unload().map_err(|_| "prepare unload")?;
    drop(model);
    assert_eq!(
        lifecycle.complete_unload().map_err(|_| "complete unload")?,
        LifecycleAction::UnloadComplete
    );
    Ok(())
}

#[test]
fn rejects_weight_dtype_mismatch() -> TestResult {
    let fixture = TinyLlamaFixture::create()?;
    let source = CandleLlamaSource::new(
        fixture.config_path.clone(),
        vec![fixture.weight_path.clone()],
        CandleScalarType::F16,
    )
    .map_err(|_| "create mismatched source")?;
    let mut loader = CandleLlamaLoader::new(BackendId::new(3));
    assert!(matches!(
        loader.load(&source, &load_configuration()),
        Err(domain_contracts::LoadError::UnsupportedFormat)
    ));
    Ok(())
}

#[test]
fn rejects_non_cpu_and_insufficient_memory_plans() -> TestResult {
    let fixture = TinyLlamaFixture::create()?;
    let source = fixture.source()?;
    let loader = CandleLlamaLoader::new(BackendId::new(2));
    let mut configuration = load_configuration();
    configuration.device_kind = DeviceKind::Cuda;
    assert!(loader.plan_load(&source, &configuration).is_err());

    configuration.device_kind = DeviceKind::Cpu;
    configuration.device = DeviceId::new(1);
    assert!(loader.plan_load(&source, &configuration).is_err());

    configuration.device = DeviceId::new(0);
    configuration.memory_budget.host_bytes = 1;
    assert!(loader.plan_load(&source, &configuration).is_err());
    Ok(())
}

const fn load_configuration() -> LoadConfiguration {
    LoadConfiguration {
        handle: ModelHandle::new(ModelId::new(9), ModelGeneration::new(1)),
        device: DeviceId::new(0),
        device_kind: DeviceKind::Cpu,
        memory_budget: MemoryBudget {
            host_bytes: u64::MAX,
            device_bytes: 0,
        },
    }
}

struct TinyLlamaFixture {
    directory: PathBuf,
    config_path: PathBuf,
    weight_path: PathBuf,
}

impl TinyLlamaFixture {
    fn create() -> Result<Self, &'static str> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "system clock")?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "llm-app-candle-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).map_err(|_| "create fixture directory")?;
        let config_path = directory.join("config.json");
        let weight_path = directory.join("model.safetensors");
        fs::write(&config_path, TINY_CONFIG).map_err(|_| "write config")?;
        write_weights(&weight_path)?;

        Ok(Self {
            directory,
            config_path,
            weight_path,
        })
    }

    fn source(&self) -> Result<CandleLlamaSource, &'static str> {
        CandleLlamaSource::new(
            self.config_path.clone(),
            vec![self.weight_path.clone()],
            CandleScalarType::F32,
        )
        .map_err(|_| "create source")
    }
}

impl Drop for TinyLlamaFixture {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.directory);
    }
}

fn write_weights(path: &Path) -> Result<(), &'static str> {
    let device = Device::Cpu;
    let mut tensors = HashMap::new();
    insert_matrix(&mut tensors, "model.embed_tokens.weight", 16, 8, &device)?;
    insert_matrix(&mut tensors, "lm_head.weight", 16, 8, &device)?;
    insert_vector(&mut tensors, "model.norm.weight", 8, &device)?;
    insert_matrix(
        &mut tensors,
        "model.layers.0.self_attn.q_proj.weight",
        8,
        8,
        &device,
    )?;
    insert_matrix(
        &mut tensors,
        "model.layers.0.self_attn.k_proj.weight",
        8,
        8,
        &device,
    )?;
    insert_matrix(
        &mut tensors,
        "model.layers.0.self_attn.v_proj.weight",
        8,
        8,
        &device,
    )?;
    insert_matrix(
        &mut tensors,
        "model.layers.0.self_attn.o_proj.weight",
        8,
        8,
        &device,
    )?;
    insert_vector(
        &mut tensors,
        "model.layers.0.input_layernorm.weight",
        8,
        &device,
    )?;
    insert_vector(
        &mut tensors,
        "model.layers.0.post_attention_layernorm.weight",
        8,
        &device,
    )?;
    insert_matrix(
        &mut tensors,
        "model.layers.0.mlp.gate_proj.weight",
        16,
        8,
        &device,
    )?;
    insert_matrix(
        &mut tensors,
        "model.layers.0.mlp.up_proj.weight",
        16,
        8,
        &device,
    )?;
    insert_matrix(
        &mut tensors,
        "model.layers.0.mlp.down_proj.weight",
        8,
        16,
        &device,
    )?;
    candle_core::safetensors::save(&tensors, path).map_err(|_| "save weights")
}

fn insert_matrix(
    tensors: &mut HashMap<String, Tensor>,
    name: &str,
    rows: usize,
    columns: usize,
    device: &Device,
) -> Result<(), &'static str> {
    let tensor = Tensor::zeros((rows, columns), DType::F32, device).map_err(|_| "create matrix")?;
    if tensors.insert(name.to_owned(), tensor).is_some() {
        return Err("duplicate matrix name");
    }
    Ok(())
}

fn insert_vector(
    tensors: &mut HashMap<String, Tensor>,
    name: &str,
    length: usize,
    device: &Device,
) -> Result<(), &'static str> {
    let tensor = Tensor::ones(length, DType::F32, device).map_err(|_| "create vector")?;
    if tensors.insert(name.to_owned(), tensor).is_some() {
        return Err("duplicate vector name");
    }
    Ok(())
}

const TINY_CONFIG: &str = r#"{
  "hidden_size": 8,
  "intermediate_size": 16,
  "vocab_size": 16,
  "num_hidden_layers": 1,
  "num_attention_heads": 2,
  "num_key_value_heads": 2,
  "rms_norm_eps": 0.00001,
  "rope_theta": 10000.0,
  "bos_token_id": 1,
  "eos_token_id": 2,
  "rope_scaling": null,
  "max_position_embeddings": 16,
  "tie_word_embeddings": false
}"#;
