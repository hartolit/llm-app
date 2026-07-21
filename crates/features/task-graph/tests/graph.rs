//! Integration tests for task-graph validation and state propagation.

use core::num::NonZeroU16;

use domain_contracts::{BackendId, TaskId};
use task_graph::{
    ArtifactKind, GraphValidationScratch, ModelPolicy, TaskBudget, TaskDependency, TaskGraph,
    TaskGraphError, TaskKind, TaskNode, TaskOutputContract, TaskRuntimeState, TaskStateTable,
    TaskStatus, validate_graph,
};

fn node(id: u64, kind: TaskKind) -> Result<TaskNode, &'static str> {
    let attempts = NonZeroU16::new(2).ok_or("attempt count must be non-zero")?;
    Ok(TaskNode {
        id: TaskId::new(id),
        kind,
        model_policy: ModelPolicy::PreferredBackend(BackendId::new(1)),
        budget: TaskBudget::new(1024, 512, attempts),
        output: TaskOutputContract {
            kind: ArtifactKind::Text,
            maximum_bytes: 4096,
        },
    })
}

#[test]
fn acyclic_graph_validates() -> Result<(), &'static str> {
    let nodes = [
        node(1, TaskKind::Draft)?,
        node(2, TaskKind::Review)?,
        node(3, TaskKind::Revise)?,
    ];
    let dependencies = [
        TaskDependency {
            prerequisite: TaskId::new(1),
            dependent: TaskId::new(2),
        },
        TaskDependency {
            prerequisite: TaskId::new(2),
            dependent: TaskId::new(3),
        },
    ];
    let graph = TaskGraph::new(&nodes, &dependencies);
    let mut incoming = [0_u32; 3];
    let mut queue = [0_usize; 3];

    validate_graph(
        &graph,
        GraphValidationScratch {
            incoming_counts: &mut incoming,
            queue: &mut queue,
        },
    )
    .map_err(|_| "valid graph rejected")
}

#[test]
fn directed_cycle_is_rejected() -> Result<(), &'static str> {
    let nodes = [node(1, TaskKind::Draft)?, node(2, TaskKind::Review)?];
    let dependencies = [
        TaskDependency {
            prerequisite: TaskId::new(1),
            dependent: TaskId::new(2),
        },
        TaskDependency {
            prerequisite: TaskId::new(2),
            dependent: TaskId::new(1),
        },
    ];
    let graph = TaskGraph::new(&nodes, &dependencies);
    let mut incoming = [0_u32; 2];
    let mut queue = [0_usize; 2];

    let result = validate_graph(
        &graph,
        GraphValidationScratch {
            incoming_counts: &mut incoming,
            queue: &mut queue,
        },
    );

    assert_eq!(result, Err(TaskGraphError::CycleDetected));
    Ok(())
}

#[test]
fn runtime_state_releases_dependents_only_after_success() -> Result<(), &'static str> {
    let nodes = [node(1, TaskKind::Draft)?, node(2, TaskKind::Review)?];
    let dependencies = [TaskDependency {
        prerequisite: TaskId::new(1),
        dependent: TaskId::new(2),
    }];
    let graph = TaskGraph::new(&nodes, &dependencies);
    let mut states = [TaskRuntimeState::default(); 2];
    let mut table = TaskStateTable::new(&graph, &mut states).map_err(|_| "state table rejected")?;
    let mut ready = [TaskId::new(0); 2];

    assert_eq!(
        table
            .ready_tasks(&graph, &mut ready)
            .map_err(|_| "ready query failed")?,
        1
    );
    assert_eq!(ready[0], TaskId::new(1));

    table
        .start(&graph, TaskId::new(1))
        .map_err(|_| "start failed")?;
    table
        .succeed(&graph, TaskId::new(1))
        .map_err(|_| "success failed")?;

    assert_eq!(
        table
            .ready_tasks(&graph, &mut ready)
            .map_err(|_| "ready query failed")?,
        1
    );
    assert_eq!(ready[0], TaskId::new(2));
    assert_eq!(
        table
            .state(&graph, TaskId::new(1))
            .map(|state| state.status),
        Some(TaskStatus::Succeeded)
    );
    Ok(())
}

#[test]
fn failed_prerequisite_blocks_descendants() -> Result<(), &'static str> {
    let nodes = [
        node(1, TaskKind::Draft)?,
        node(2, TaskKind::Review)?,
        node(3, TaskKind::Revise)?,
    ];
    let dependencies = [
        TaskDependency {
            prerequisite: TaskId::new(1),
            dependent: TaskId::new(2),
        },
        TaskDependency {
            prerequisite: TaskId::new(2),
            dependent: TaskId::new(3),
        },
    ];
    let graph = TaskGraph::new(&nodes, &dependencies);
    let mut states = [TaskRuntimeState::default(); 3];
    let mut table = TaskStateTable::new(&graph, &mut states).map_err(|_| "state table rejected")?;

    table
        .start(&graph, TaskId::new(1))
        .map_err(|_| "first start failed")?;
    table
        .fail(&graph, TaskId::new(1))
        .map_err(|_| "first failure transition failed")?;
    table
        .start(&graph, TaskId::new(1))
        .map_err(|_| "retry start failed")?;
    table
        .fail(&graph, TaskId::new(1))
        .map_err(|_| "terminal failure transition failed")?;
    assert_eq!(
        table
            .propagate_blocked(&graph)
            .map_err(|_| "block propagation failed")?,
        2
    );
    assert_eq!(
        table
            .state(&graph, TaskId::new(2))
            .map(|state| state.status),
        Some(TaskStatus::Blocked)
    );
    assert_eq!(
        table
            .state(&graph, TaskId::new(3))
            .map(|state| state.status),
        Some(TaskStatus::Blocked)
    );
    Ok(())
}
