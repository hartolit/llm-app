# Architectural Blueprint: The Modular Philosophy

**CORE IDEOLOGY: DEFEATING THE MONOLITH**
This project strictly rejects monolithic structures. We do not build massive, tightly coupled "engine" or "core" crates where logic is tangled. We operate on a highly modular, decoupled architecture. Every distinct feature, system, or domain must exist as its own independent crate within a Cargo workspace.

---

## ACTIVE ARCHITECTURE SELECTION
*Mark the architecture model currently utilized by this project with `[X]`.*

- [ ] **MODEL A:** Standard Workspace (Small-to-Medium Projects)
- [ X ] **MODEL B:** Layered Workspace (Large-Scale / Complex Projects)

---

### [ ] MODEL A: STANDARD WORKSPACE
*Optimized for focused applications, single-purpose tools, or tight bare-metal implementations.*

**1. CRATE ONTOLOGY**
* **Feature Crates (The Building Blocks):** Isolated, heavily confined modules (e.g., `os-vga`, `game-water`). These must NEVER depend upward on the main engine or horizontally on unrelated feature crates. Design every crate as if it will be published to `crates.io` and used by an entirely different project tomorrow.
* **Engine/Core Crates (The Glue):** These crates do NOT implement core features. Instead, they consume Feature Crates and provide the orchestration, ECS integration, or state management to link them together cleanly.
* **App/Runner Crates (The Boundary):** Thin execution vectors (e.g., `kernel-runner`, `server-cli`). They handle initialization, config loading, and environment I/O, then pass control to the Engine.

---

### [ X ] MODEL B: LAYERED WORKSPACE
*Optimized for expansive applications requiring heavy infrastructure decoupling, multiple execution environments, and complex external dependencies (e.g., LLM applications, heavy desktop software).*

**1. CRATE ONTOLOGY**
* **Feature Crates (Pure Logic & Contracts):** Pure, isolated building blocks and domain types (e.g., `domain-contracts`, `tokenization`, `context-planner`). These strictly define interfaces and mathematical/logical operations. They should ideally be `no_std` and must NEVER depend upward or horizontally.
* **Adapter Crates (Infrastructure):** Heavy infrastructure implementations. This isolates `std`-dependent boundaries, C-FFI, and third-party vendor wrappers (e.g., `candle-backend`, `gguf-backend`, `redb-storage`) away from pure feature logic. Adapters implement the traits defined in Feature Crates.
* **Engine Crates (The Orchestrators):** State managers and task coordinators (e.g., `inference-runtime`, `task-orchestrator`). These consume Features and Adapters to wire the application together.
* **App/Runner Crates (The Boundary):** Thin execution vectors (e.g., `desktop-slint`, `cli-runner`). They handle OS interactions, environment I/O, config loading, and pass control down to the Engines.

---

## 2. UNIVERSAL API BOUNDARIES & COUPLING LAWS
*(These laws are absolute and apply regardless of the selected architecture model.)*

* **Explicit Public APIs:** A crate's internals must be heavily encapsulated. Internal logic, state, and helper functions must remain strictly private. Expose only what is strictly necessary through a stable, well-documented API.
* **Dependency Injection:** Crates should not assume the existence of a global state. Pass necessary contexts, traits, or data down into the crate via its API.
* **Acyclic Dependencies:** Crates must form a clean, directed acyclic graph (DAG). Circular dependencies or "God objects" that weave crates back together are architectural failures.

---

## 3. STRUCTURAL ANTI-PATTERNS (LESSONS LEARNED)
* **Micro-Crate Hell:** Do not over-fragment domain logic into infinitesimally small crates (e.g., separating identifiers, metadata, and buffers into individual `Cargo.toml` files). Consolidate core types into cohesive crates (like `domain-contracts`) to prevent verbose import nightmares and API fragmentation.
* **The Adapter Quarantine:** Heavy ecosystem dependencies and OS-level I/O must remain strictly quarantined inside the Adapters directory. Features must remain pure (and potentially `no_std`). Never bleed infrastructure or vendor-specific code into the logic layer.
* **Engine Consolidation:** Orchestrators share tight lifetimes and high-frequency state. Splitting them into too many granular crates mathematically increases the risk of circular dependencies. Keep engines consolidated (e.g., 1-3 crates max per domain) to guarantee a clean, acyclic dependency tree.

---

## 4. PROJECT-SPECIFIC FEATURE TIERS

The current workspace uses two dependency tiers inside the Feature layer:

- **F0 — `domain-contracts`:** The sole shared leaf contract. It owns the common
  vocabulary that must cross engine/backend boundaries and has no workspace-local
  dependencies.
- **F1 — Algorithmic features:** `tokenization`, `context-planner`, `sampling`,
  and `task-graph`. An F1 crate may depend downward on F0 but may never depend on
  another F1 crate.

Therefore, `sampling -> domain-contracts` is a downward dependency, while
`sampling -> tokenization` would be a forbidden horizontal dependency. Folder
proximity does not define dependency level; the declared tier does.

This exception does not authorize arbitrary shared utility crates. A new F0
crate requires architectural review. Shared identifiers, capacity failures, and
backend-facing contracts remain consolidated in `domain-contracts` to avoid
micro-crate fragmentation.

## 5. PROJECT-SPECIFIC ENGINE TIERS

The engine layer currently contains two consolidated ownership domains:

- **E0 — `inference-runtime`:** owns loaded model generations, active sequences,
  admission control, cancellation, draining, and deterministic resource release.
- **E1 — `application-runtime`:** owns frontend-neutral application use cases:
  Hub resolution, tokenizer validation, persistence, and commands directed to E0.

`application-runtime -> inference-runtime` is the only permitted engine-to-engine
production edge. The inverse edge is forbidden. This tiering avoids placing Hub,
storage, or frontend workflow concerns in the inference resource owner while
keeping the total engine count within the 1–3 crate consolidation rule.

Slint, Tauri, CLI, and other execution vectors depend on E1 instead of directly
reimplementing adapter composition. A browser-only frontend must cross a
transport boundary to a native or remote E1 host rather than importing native
model adapters into WebAssembly.
