//! Frontend-neutral application-state contract tests.

use application_runtime::{ApplicationActivity, ApplicationState, LoadedModel, ResolvedModel};
use domain_contracts::{ModelGeneration, ModelHandle, ModelId, ScalarType};

#[test]
fn resolved_selection_controls_load_admission() {
    let state = ApplicationState::default();
    assert_eq!(state.activity(), ApplicationActivity::Idle);
    assert!(!state.can_load("owner/model", "main"));

    let resolved = ResolvedModel {
        repository: "owner/model".to_owned(),
        revision: "main".to_owned(),
        commit: "immutable".to_owned(),
        vocabulary_size: 32,
        scalar_type: Some(ScalarType::F32),
    };
    assert!(resolved.matches_selection(" owner/model ", " main "));
}

#[test]
fn loaded_model_summary_retains_generation_safe_handle() {
    let loaded = LoadedModel {
        handle: ModelHandle::new(ModelId::new(7), ModelGeneration::new(3)),
        vocabulary_size: 128,
    };
    assert_eq!(loaded.handle.generation.get(), 3);
    assert_eq!(loaded.vocabulary_size, 128);
}
