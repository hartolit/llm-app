//! Native workspace runner for architecture validation, formatting, checking, testing, and linting.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

use serde::Deserialize;

const HELP: &str = "\
llm-app workspace runner

USAGE:
    cargo run --bin llm-app -- <command>

COMMANDS:
    architecture Validate layered workspace dependency directions
    benchmark    Measure portable hot-path latency and throughput
    check        Compile every workspace target
    test         Run every workspace test
    fmt          Format the workspace
    fmt-check    Verify formatting without modifying files
    clippy       Lint every target and feature
    verify       Run architecture, fmt-check, check, test, and clippy
    help         Print this message
";

fn main() -> ExitCode {
    match execute() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("workspace runner failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> io::Result<ExitCode> {
    let command_argument = env::args_os()
        .nth(1)
        .unwrap_or_else(|| OsString::from("help"));
    let Some(command) = command_argument.to_str() else {
        eprintln!("workspace runner commands must be valid UTF-8");
        return Ok(ExitCode::from(2));
    };

    let success = match command {
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            return Ok(ExitCode::SUCCESS);
        }
        "architecture" => validate_architecture(),
        "benchmark" => run_cargo(&["bench", "-p", "sampling", "--bench", "sampling_pipeline"]),
        "check" => run_cargo(&["check", "--workspace", "--all-targets", "--all-features"]),
        "test" => run_cargo(&["test", "--workspace", "--all-targets", "--all-features"]),
        "fmt" => run_cargo(&["fmt", "--all"]),
        "fmt-check" => run_cargo(&["fmt", "--all", "--", "--check"]),
        "clippy" => run_cargo(&[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ]),
        "verify" => {
            if !validate_architecture()? {
                return Ok(ExitCode::FAILURE);
            }
            run_sequence(&[
                &["fmt", "--all", "--", "--check"],
                &["check", "--workspace", "--all-targets", "--all-features"],
                &["test", "--workspace", "--all-targets", "--all-features"],
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--",
                    "-D",
                    "warnings",
                ],
            ])
        }
        _ => {
            eprintln!("unknown command: {command}\n");
            print!("{HELP}");
            return Ok(ExitCode::from(2));
        }
    }?;

    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn run_sequence(commands: &[&[&str]]) -> io::Result<bool> {
    for arguments in commands {
        if !run_cargo(arguments)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn run_cargo(arguments: &[&str]) -> io::Result<bool> {
    let Some((subcommand, options)) = arguments.split_first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cargo command requires a subcommand",
        ));
    };
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let status = Command::new(cargo)
        .arg(subcommand)
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .args(options)
        .status()?;
    let success = status.success();

    report_status(arguments, status);
    Ok(success)
}

fn report_status(arguments: &[&str], status: ExitStatus) {
    if status.success() {
        return;
    }

    let rendered = arguments.join(" ");
    match status.code() {
        Some(code) => eprintln!("cargo {rendered} exited with status {code}"),
        None => eprintln!("cargo {rendered} terminated without an exit code"),
    }
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: BTreeSet<String>,
}

#[derive(Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<CargoDependency>,
}

#[derive(Deserialize)]
struct CargoDependency {
    name: String,
    path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Root,
    FeatureFoundation,
    FeatureAlgorithm,
    Adapter,
    EngineFoundation,
    EngineApplication,
    Application,
}

fn validate_architecture() -> io::Result<bool> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            "--format-version",
            "1",
            "--no-deps",
        ])
        .output()?;
    if !output.status.success() {
        eprintln!("cargo metadata failed");
        return Ok(false);
    }
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut valid = true;

    for package in metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
    {
        let source_layer = classify_manifest(root, &package.manifest_path);
        for dependency in package.dependencies.iter().filter(|dependency| {
            dependency
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with(root))
        }) {
            let Some(path) = dependency.path.as_deref() else {
                continue;
            };
            let target_manifest = path.join("Cargo.toml");
            let target_layer = classify_manifest(root, &target_manifest);
            if !allows(source_layer, target_layer) {
                valid = false;
                eprintln!(
                    "forbidden workspace dependency: {} ({source_layer:?}) -> {} ({target_layer:?})",
                    package.name, dependency.name
                );
            }
        }
    }

    if valid {
        println!("layered workspace dependency directions are valid");
    }
    Ok(valid)
}

fn classify_manifest(root: &Path, manifest: &Path) -> Layer {
    let relative = manifest.strip_prefix(root).unwrap_or(manifest);
    if relative == Path::new("Cargo.toml") {
        Layer::Root
    } else if relative.starts_with(Path::new("crates/features/domain-contracts")) {
        Layer::FeatureFoundation
    } else if relative.starts_with(Path::new("crates/features")) {
        Layer::FeatureAlgorithm
    } else if relative.starts_with(Path::new("crates/adapters")) {
        Layer::Adapter
    } else if relative.starts_with(Path::new("crates/engines/inference-runtime")) {
        Layer::EngineFoundation
    } else if relative.starts_with(Path::new("crates/engines")) {
        Layer::EngineApplication
    } else {
        Layer::Application
    }
}

const fn allows(source: Layer, target: Layer) -> bool {
    match source {
        Layer::Root | Layer::FeatureFoundation => false,
        Layer::FeatureAlgorithm => matches!(target, Layer::FeatureFoundation),
        Layer::Adapter => matches!(target, Layer::FeatureFoundation | Layer::FeatureAlgorithm),
        Layer::EngineFoundation => matches!(
            target,
            Layer::FeatureFoundation | Layer::FeatureAlgorithm | Layer::Adapter
        ),
        Layer::EngineApplication => matches!(
            target,
            Layer::FeatureFoundation
                | Layer::FeatureAlgorithm
                | Layer::Adapter
                | Layer::EngineFoundation
        ),
        Layer::Application => matches!(target, Layer::EngineApplication),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Layer, allows, classify_manifest};

    #[test]
    fn manifests_are_classified_into_project_specific_tiers() {
        let root = Path::new("/workspace");
        let cases = [
            ("Cargo.toml", Layer::Root),
            (
                "crates/features/domain-contracts/Cargo.toml",
                Layer::FeatureFoundation,
            ),
            (
                "crates/features/sampling/Cargo.toml",
                Layer::FeatureAlgorithm,
            ),
            ("crates/adapters/candle-backend/Cargo.toml", Layer::Adapter),
            (
                "crates/engines/inference-runtime/Cargo.toml",
                Layer::EngineFoundation,
            ),
            (
                "crates/engines/application-runtime/Cargo.toml",
                Layer::EngineApplication,
            ),
            ("crates/apps/desktop-slint/Cargo.toml", Layer::Application),
        ];

        for (relative, expected) in cases {
            assert_eq!(classify_manifest(root, &root.join(relative)), expected);
        }
    }

    #[test]
    fn feature_and_adapter_boundaries_reject_upward_and_horizontal_edges() {
        assert!(allows(Layer::FeatureAlgorithm, Layer::FeatureFoundation));
        assert!(!allows(Layer::FeatureAlgorithm, Layer::FeatureAlgorithm));
        assert!(!allows(Layer::FeatureAlgorithm, Layer::Adapter));
        assert!(!allows(Layer::FeatureAlgorithm, Layer::EngineFoundation));
        assert!(!allows(Layer::FeatureAlgorithm, Layer::Application));

        assert!(allows(Layer::Adapter, Layer::FeatureFoundation));
        assert!(allows(Layer::Adapter, Layer::FeatureAlgorithm));
        assert!(!allows(Layer::Adapter, Layer::Adapter));
        assert!(!allows(Layer::Adapter, Layer::EngineFoundation));
        assert!(!allows(Layer::Adapter, Layer::Application));
    }

    #[test]
    fn engine_and_application_edges_follow_the_declared_tiers() {
        assert!(allows(Layer::EngineFoundation, Layer::FeatureFoundation));
        assert!(allows(Layer::EngineFoundation, Layer::Adapter));
        assert!(!allows(Layer::EngineFoundation, Layer::EngineApplication));
        assert!(!allows(Layer::EngineFoundation, Layer::Application));

        assert!(allows(Layer::EngineApplication, Layer::EngineFoundation));
        assert!(allows(Layer::EngineApplication, Layer::Adapter));
        assert!(!allows(Layer::EngineApplication, Layer::EngineApplication));
        assert!(!allows(Layer::EngineApplication, Layer::Application));

        assert!(allows(Layer::Application, Layer::EngineApplication));
        assert!(!allows(Layer::Application, Layer::EngineFoundation));
        assert!(!allows(Layer::Application, Layer::Adapter));
    }
}
