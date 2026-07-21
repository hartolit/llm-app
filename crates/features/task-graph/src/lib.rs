#![no_std]
#![forbid(unsafe_code)]
#![doc = "Allocation-free task graph representation, validation, and runtime state."]

use core::num::NonZeroU16;

use domain_contracts::{
    ArtifactId, BackendId, CapacityExhausted, CapacityResource, ModelId, TaskId,
};

/// Operation represented by a workflow node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskKind {
    /// Produce an initial response or artifact.
    Draft,
    /// Review an artifact for defects.
    Review,
    /// Run the Rust compiler or another deterministic type checker.
    CompileCheck,
    /// Run a deterministic validator.
    Validate,
    /// Revise an artifact using prior findings.
    Revise,
    /// Aggregate multiple artifacts into one result.
    Aggregate,
    /// Application-defined operation code.
    Other(u16),
}

/// Artifact category produced by a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArtifactKind {
    /// Plain UTF-8 text.
    Text,
    /// Source code.
    SourceCode,
    /// Structured compiler or review findings.
    Diagnostics,
    /// Token sequence.
    Tokens,
    /// Application-defined artifact category.
    Other(u16),
}

/// Output requirements declared before execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskOutputContract {
    /// Required artifact category.
    pub kind: ArtifactKind,
    /// Hard upper bound for persisted output bytes. Zero means externally bounded.
    pub maximum_bytes: u64,
}

/// Model-selection policy interpreted by an orchestration engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelPolicy {
    /// Use one exact logical model.
    Exact(ModelId),
    /// Prefer any compatible model implemented by one backend.
    PreferredBackend(BackendId),
    /// Use any compatible admitted model.
    AnyCompatible,
    /// Run no model because the task is deterministic.
    Deterministic,
}

/// Hard task budgets known before execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskBudget {
    /// Maximum input tokens admitted to the task.
    pub maximum_input_tokens: u32,
    /// Maximum output tokens admitted to the task.
    pub maximum_output_tokens: u32,
    /// Maximum execution attempts including the first attempt.
    pub maximum_attempts: NonZeroU16,
}

impl TaskBudget {
    /// Creates a task budget from validated bounds.
    #[must_use]
    pub const fn new(
        maximum_input_tokens: u32,
        maximum_output_tokens: u32,
        maximum_attempts: NonZeroU16,
    ) -> Self {
        Self {
            maximum_input_tokens,
            maximum_output_tokens,
            maximum_attempts,
        }
    }
}

/// Immutable node definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskNode {
    /// Stable task identity.
    pub id: TaskId,
    /// Operation performed by the node.
    pub kind: TaskKind,
    /// Model-selection policy.
    pub model_policy: ModelPolicy,
    /// Hard execution budgets.
    pub budget: TaskBudget,
    /// Declared output contract.
    pub output: TaskOutputContract,
}

/// Directed dependency requiring one task to succeed before another can start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskDependency {
    /// Task that must succeed first.
    pub prerequisite: TaskId,
    /// Task gated by the prerequisite.
    pub dependent: TaskId,
}

/// Immutable artifact reference passed between tasks by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtifactReference {
    /// Artifact identity.
    pub id: ArtifactId,
    /// Artifact category.
    pub kind: ArtifactKind,
}

/// Borrowed immutable task graph.
#[derive(Clone, Copy, Debug)]
pub struct TaskGraph<'a> {
    /// Node definitions.
    pub nodes: &'a [TaskNode],
    /// Directed dependency edges.
    pub dependencies: &'a [TaskDependency],
}

impl<'a> TaskGraph<'a> {
    /// Creates a borrowed graph view.
    #[must_use]
    pub const fn new(nodes: &'a [TaskNode], dependencies: &'a [TaskDependency]) -> Self {
        Self {
            nodes,
            dependencies,
        }
    }

    /// Returns the node with the requested identity.
    #[must_use]
    pub fn node(&self, task_id: TaskId) -> Option<&TaskNode> {
        self.nodes.iter().find(|node| node.id == task_id)
    }

    /// Returns the node index with the requested identity.
    #[must_use]
    pub fn node_index(&self, task_id: TaskId) -> Option<usize> {
        self.nodes.iter().position(|node| node.id == task_id)
    }
}

/// Caller-owned scratch required for acyclic graph validation.
pub struct GraphValidationScratch<'a> {
    /// Per-node incoming-edge counts.
    pub incoming_counts: &'a mut [u32],
    /// Kahn traversal queue storing node indices.
    pub queue: &'a mut [usize],
}

/// Stable graph or workflow-state failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskGraphError {
    /// Two nodes share one identity.
    DuplicateTask(TaskId),
    /// Two identical dependency edges were supplied.
    DuplicateDependency(TaskDependency),
    /// A dependency references an unknown task.
    UnknownTask(TaskId),
    /// A task depends directly on itself.
    SelfDependency(TaskId),
    /// The graph contains at least one directed cycle.
    CycleDetected,
    /// A caller-owned fixed-capacity buffer is too small.
    CapacityExhausted(CapacityExhausted),
    /// Runtime state storage does not match the graph node count.
    StateLengthMismatch {
        /// Number of graph nodes.
        required: usize,
        /// Number of supplied state entries.
        available: usize,
    },
    /// Requested task transition is invalid.
    InvalidTransition {
        /// Task being transitioned.
        task: TaskId,
        /// Current state.
        state: TaskStatus,
    },
    /// Task exhausted its configured attempt budget.
    AttemptLimitReached(TaskId),
}

impl From<CapacityExhausted> for TaskGraphError {
    fn from(value: CapacityExhausted) -> Self {
        Self::CapacityExhausted(value)
    }
}

/// Validates node identity, dependency integrity, and acyclicity.
///
/// # Errors
///
/// Returns [`TaskGraphError::CapacityExhausted`] when the validation scratch is
/// too small or an edge count overflows; [`TaskGraphError::DuplicateTask`] or
/// [`TaskGraphError::DuplicateDependency`] for duplicate definitions;
/// [`TaskGraphError::UnknownTask`] or [`TaskGraphError::SelfDependency`] for an
/// invalid edge; and [`TaskGraphError::CycleDetected`] when the graph is cyclic.
pub fn validate_graph(
    graph: &TaskGraph<'_>,
    scratch: GraphValidationScratch<'_>,
) -> Result<(), TaskGraphError> {
    validate_scratch(graph.nodes.len(), &scratch)?;
    validate_nodes(graph.nodes)?;
    validate_dependencies(graph)?;

    let GraphValidationScratch {
        incoming_counts,
        queue,
    } = scratch;
    let node_count = graph.nodes.len();
    let incoming_capacity = incoming_counts.len();
    let Some(counts) = incoming_counts.get_mut(..node_count) else {
        return Err(node_capacity(node_count, incoming_capacity));
    };
    counts.fill(0);

    for dependency in graph.dependencies {
        let dependent_index = graph
            .node_index(dependency.dependent)
            .ok_or(TaskGraphError::UnknownTask(dependency.dependent))?;
        let count_capacity = counts.len();
        let Some(count) = counts.get_mut(dependent_index) else {
            return Err(node_capacity(
                dependent_index.saturating_add(1),
                count_capacity,
            ));
        };
        let current = *count;
        let next = current.checked_add(1).ok_or_else(|| {
            TaskGraphError::CapacityExhausted(CapacityExhausted::new(
                CapacityResource::TaskEdges,
                u64::from(u32::MAX) + 1,
                u64::from(current),
            ))
        })?;
        *count = next;
    }

    let mut queue_length = 0_usize;
    for (index, &count) in counts.iter().enumerate() {
        if count == 0 {
            write_queue(queue, queue_length, index)?;
            queue_length += 1;
        }
    }

    let mut head = 0_usize;
    let mut visited = 0_usize;
    while head < queue_length {
        let Some(&node_index) = queue.get(head) else {
            return Err(node_capacity(head.saturating_add(1), queue.len()));
        };
        head += 1;
        visited += 1;
        let Some(node) = graph.nodes.get(node_index) else {
            return Err(node_capacity(
                node_index.saturating_add(1),
                graph.nodes.len(),
            ));
        };

        for dependency in graph
            .dependencies
            .iter()
            .filter(|dependency| dependency.prerequisite == node.id)
        {
            let dependent_index = graph
                .node_index(dependency.dependent)
                .ok_or(TaskGraphError::UnknownTask(dependency.dependent))?;
            let count_capacity = counts.len();
            let Some(count) = counts.get_mut(dependent_index) else {
                return Err(node_capacity(
                    dependent_index.saturating_add(1),
                    count_capacity,
                ));
            };
            if *count == 0 {
                return Err(TaskGraphError::DuplicateDependency(*dependency));
            }
            *count -= 1;
            if *count == 0 {
                write_queue(queue, queue_length, dependent_index)?;
                queue_length += 1;
            }
        }
    }

    if visited == graph.nodes.len() {
        Ok(())
    } else {
        Err(TaskGraphError::CycleDetected)
    }
}

/// Runtime state of one task.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskStatus {
    /// Waiting for all prerequisites.
    #[default]
    Pending,
    /// Currently executing.
    Running,
    /// Completed successfully.
    Succeeded,
    /// Attempt failed and may be retried within budget.
    Failed,
    /// All configured attempts failed.
    Exhausted,
    /// Explicitly cancelled.
    Cancelled,
    /// Cannot run because a prerequisite terminated unsuccessfully.
    Blocked,
}

/// Mutable runtime fields aligned by index with graph nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskRuntimeState {
    /// Current task state.
    pub status: TaskStatus,
    /// Number of attempts already started.
    pub attempts: u16,
}

/// Caller-owned runtime state table.
pub struct TaskStateTable<'a> {
    states: &'a mut [TaskRuntimeState],
}

impl<'a> TaskStateTable<'a> {
    /// Creates a state table after checking exact graph alignment.
    ///
    /// # Errors
    ///
    /// Returns [`TaskGraphError::StateLengthMismatch`] when `states` does not
    /// contain exactly one entry per graph node.
    pub const fn new(
        graph: &TaskGraph<'_>,
        states: &'a mut [TaskRuntimeState],
    ) -> Result<Self, TaskGraphError> {
        if states.len() != graph.nodes.len() {
            return Err(TaskGraphError::StateLengthMismatch {
                required: graph.nodes.len(),
                available: states.len(),
            });
        }
        Ok(Self { states })
    }

    /// Returns immutable state for one task.
    #[must_use]
    pub fn state(&self, graph: &TaskGraph<'_>, task_id: TaskId) -> Option<TaskRuntimeState> {
        let index = graph.node_index(task_id)?;
        self.states.get(index).copied()
    }

    /// Starts one ready task and increments its attempt count.
    ///
    /// # Errors
    ///
    /// Returns [`TaskGraphError::UnknownTask`] when the task or a prerequisite is
    /// absent, [`TaskGraphError::StateLengthMismatch`] when prerequisite state is
    /// unavailable, [`TaskGraphError::InvalidTransition`] when the task is not
    /// pending or failed or its prerequisites have not succeeded, and
    /// [`TaskGraphError::AttemptLimitReached`] when its attempt budget is spent.
    pub fn start(&mut self, graph: &TaskGraph<'_>, task_id: TaskId) -> Result<(), TaskGraphError> {
        let index = graph
            .node_index(task_id)
            .ok_or(TaskGraphError::UnknownTask(task_id))?;
        let node = graph
            .nodes
            .get(index)
            .ok_or(TaskGraphError::UnknownTask(task_id))?;
        let current = self
            .states
            .get(index)
            .copied()
            .ok_or(TaskGraphError::UnknownTask(task_id))?;
        if current.status != TaskStatus::Pending && current.status != TaskStatus::Failed {
            return Err(TaskGraphError::InvalidTransition {
                task: task_id,
                state: current.status,
            });
        }
        if !prerequisites_succeeded(graph, self.states, task_id)? {
            return Err(TaskGraphError::InvalidTransition {
                task: task_id,
                state: current.status,
            });
        }
        if current.attempts >= node.budget.maximum_attempts.get() {
            return Err(TaskGraphError::AttemptLimitReached(task_id));
        }
        let Some(state) = self.states.get_mut(index) else {
            return Err(TaskGraphError::UnknownTask(task_id));
        };
        state.attempts = state.attempts.saturating_add(1);
        state.status = TaskStatus::Running;
        Ok(())
    }

    /// Marks a running task successful.
    ///
    /// # Errors
    ///
    /// Returns [`TaskGraphError::UnknownTask`] when the task or its state is
    /// absent, or [`TaskGraphError::InvalidTransition`] when it is not running.
    pub fn succeed(
        &mut self,
        graph: &TaskGraph<'_>,
        task_id: TaskId,
    ) -> Result<(), TaskGraphError> {
        self.transition_from_running(graph, task_id, TaskStatus::Succeeded)
    }

    /// Marks a running task failed and records terminal exhaustion when no retry remains.
    ///
    /// # Errors
    ///
    /// Returns [`TaskGraphError::UnknownTask`] when the task or its state is
    /// absent, or [`TaskGraphError::InvalidTransition`] when it is not running.
    pub fn fail(&mut self, graph: &TaskGraph<'_>, task_id: TaskId) -> Result<(), TaskGraphError> {
        let index = graph
            .node_index(task_id)
            .ok_or(TaskGraphError::UnknownTask(task_id))?;
        let node = graph
            .nodes
            .get(index)
            .ok_or(TaskGraphError::UnknownTask(task_id))?;
        let Some(state) = self.states.get_mut(index) else {
            return Err(TaskGraphError::UnknownTask(task_id));
        };
        if state.status != TaskStatus::Running {
            return Err(TaskGraphError::InvalidTransition {
                task: task_id,
                state: state.status,
            });
        }
        state.status = if state.attempts >= node.budget.maximum_attempts.get() {
            TaskStatus::Exhausted
        } else {
            TaskStatus::Failed
        };
        Ok(())
    }

    /// Cancels a pending, failed, or running task.
    ///
    /// # Errors
    ///
    /// Returns [`TaskGraphError::UnknownTask`] when the task or its state is
    /// absent, or [`TaskGraphError::InvalidTransition`] when it is not pending,
    /// failed, or running.
    pub fn cancel(&mut self, graph: &TaskGraph<'_>, task_id: TaskId) -> Result<(), TaskGraphError> {
        let index = graph
            .node_index(task_id)
            .ok_or(TaskGraphError::UnknownTask(task_id))?;
        let Some(state) = self.states.get_mut(index) else {
            return Err(TaskGraphError::UnknownTask(task_id));
        };
        match state.status {
            TaskStatus::Pending | TaskStatus::Failed | TaskStatus::Running => {
                state.status = TaskStatus::Cancelled;
                Ok(())
            }
            current => Err(TaskGraphError::InvalidTransition {
                task: task_id,
                state: current,
            }),
        }
    }

    /// Marks all pending descendants of unsuccessful prerequisites as blocked.
    ///
    /// # Errors
    ///
    /// Returns [`TaskGraphError::UnknownTask`] when a dependency references an
    /// absent task, or [`TaskGraphError::StateLengthMismatch`] when runtime state
    /// is unavailable for a graph node.
    pub fn propagate_blocked(&mut self, graph: &TaskGraph<'_>) -> Result<usize, TaskGraphError> {
        let mut total_changed = 0_usize;
        loop {
            let mut changed = 0_usize;
            for (index, node) in graph.nodes.iter().enumerate() {
                let Some(current) = self.states.get(index).copied() else {
                    return Err(TaskGraphError::StateLengthMismatch {
                        required: graph.nodes.len(),
                        available: self.states.len(),
                    });
                };
                if current.status != TaskStatus::Pending {
                    continue;
                }
                if has_unsuccessful_prerequisite(graph, self.states, node.id)? {
                    let Some(state) = self.states.get_mut(index) else {
                        return Err(TaskGraphError::UnknownTask(node.id));
                    };
                    state.status = TaskStatus::Blocked;
                    changed += 1;
                }
            }
            total_changed = total_changed.saturating_add(changed);
            if changed == 0 {
                return Ok(total_changed);
            }
        }
    }

    /// Writes all currently ready task identities into caller-owned output.
    ///
    /// # Errors
    ///
    /// Returns [`TaskGraphError::UnknownTask`] when a dependency references an
    /// absent task, [`TaskGraphError::StateLengthMismatch`] when runtime state is
    /// unavailable for a graph node, or [`TaskGraphError::CapacityExhausted`]
    /// when `output` cannot hold every ready task.
    pub fn ready_tasks(
        &self,
        graph: &TaskGraph<'_>,
        output: &mut [TaskId],
    ) -> Result<usize, TaskGraphError> {
        let mut written = 0_usize;
        for (index, node) in graph.nodes.iter().enumerate() {
            let Some(state) = self.states.get(index) else {
                return Err(TaskGraphError::StateLengthMismatch {
                    required: graph.nodes.len(),
                    available: self.states.len(),
                });
            };
            if (state.status == TaskStatus::Pending || state.status == TaskStatus::Failed)
                && prerequisites_succeeded(graph, self.states, node.id)?
                && state.attempts < node.budget.maximum_attempts.get()
            {
                let available = output.len();
                let Some(slot) = output.get_mut(written) else {
                    return Err(TaskGraphError::CapacityExhausted(CapacityExhausted::new(
                        CapacityResource::TaskNodes,
                        written.saturating_add(1) as u64,
                        available as u64,
                    )));
                };
                *slot = node.id;
                written += 1;
            }
        }
        Ok(written)
    }

    fn transition_from_running(
        &mut self,
        graph: &TaskGraph<'_>,
        task_id: TaskId,
        next: TaskStatus,
    ) -> Result<(), TaskGraphError> {
        let index = graph
            .node_index(task_id)
            .ok_or(TaskGraphError::UnknownTask(task_id))?;
        let Some(state) = self.states.get_mut(index) else {
            return Err(TaskGraphError::UnknownTask(task_id));
        };
        if state.status != TaskStatus::Running {
            return Err(TaskGraphError::InvalidTransition {
                task: task_id,
                state: state.status,
            });
        }
        state.status = next;
        Ok(())
    }
}

fn validate_scratch(
    required: usize,
    scratch: &GraphValidationScratch<'_>,
) -> Result<(), TaskGraphError> {
    let available = scratch.incoming_counts.len().min(scratch.queue.len());
    if available < required {
        return Err(node_capacity(required, available));
    }
    Ok(())
}

fn validate_nodes(nodes: &[TaskNode]) -> Result<(), TaskGraphError> {
    for (left_index, left) in nodes.iter().enumerate() {
        let Some(tail) = nodes.get(left_index.saturating_add(1)..) else {
            continue;
        };
        if tail.iter().any(|right| right.id == left.id) {
            return Err(TaskGraphError::DuplicateTask(left.id));
        }
    }
    Ok(())
}

fn validate_dependencies(graph: &TaskGraph<'_>) -> Result<(), TaskGraphError> {
    for (left_index, dependency) in graph.dependencies.iter().enumerate() {
        if graph.node(dependency.prerequisite).is_none() {
            return Err(TaskGraphError::UnknownTask(dependency.prerequisite));
        }
        if graph.node(dependency.dependent).is_none() {
            return Err(TaskGraphError::UnknownTask(dependency.dependent));
        }
        if dependency.prerequisite == dependency.dependent {
            return Err(TaskGraphError::SelfDependency(dependency.prerequisite));
        }
        let Some(tail) = graph.dependencies.get(left_index.saturating_add(1)..) else {
            continue;
        };
        if tail.iter().any(|right| right == dependency) {
            return Err(TaskGraphError::DuplicateDependency(*dependency));
        }
    }
    Ok(())
}

fn prerequisites_succeeded(
    graph: &TaskGraph<'_>,
    states: &[TaskRuntimeState],
    task_id: TaskId,
) -> Result<bool, TaskGraphError> {
    for dependency in graph
        .dependencies
        .iter()
        .filter(|dependency| dependency.dependent == task_id)
    {
        let index = graph
            .node_index(dependency.prerequisite)
            .ok_or(TaskGraphError::UnknownTask(dependency.prerequisite))?;
        let Some(state) = states.get(index) else {
            return Err(TaskGraphError::StateLengthMismatch {
                required: graph.nodes.len(),
                available: states.len(),
            });
        };
        if state.status != TaskStatus::Succeeded {
            return Ok(false);
        }
    }
    Ok(true)
}

fn has_unsuccessful_prerequisite(
    graph: &TaskGraph<'_>,
    states: &[TaskRuntimeState],
    task_id: TaskId,
) -> Result<bool, TaskGraphError> {
    for dependency in graph
        .dependencies
        .iter()
        .filter(|dependency| dependency.dependent == task_id)
    {
        let index = graph
            .node_index(dependency.prerequisite)
            .ok_or(TaskGraphError::UnknownTask(dependency.prerequisite))?;
        let Some(state) = states.get(index) else {
            return Err(TaskGraphError::StateLengthMismatch {
                required: graph.nodes.len(),
                available: states.len(),
            });
        };
        if matches!(
            state.status,
            TaskStatus::Exhausted | TaskStatus::Cancelled | TaskStatus::Blocked
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_queue(queue: &mut [usize], position: usize, value: usize) -> Result<(), TaskGraphError> {
    let available = queue.len();
    let Some(slot) = queue.get_mut(position) else {
        return Err(node_capacity(position.saturating_add(1), available));
    };
    *slot = value;
    Ok(())
}

const fn node_capacity(required: usize, available: usize) -> TaskGraphError {
    TaskGraphError::CapacityExhausted(CapacityExhausted::new(
        CapacityResource::TaskNodes,
        required as u64,
        available as u64,
    ))
}
