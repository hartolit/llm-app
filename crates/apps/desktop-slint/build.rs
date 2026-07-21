//! Compiles the Slint component definitions for the desktop runner.

#![forbid(unsafe_code)]

fn main() -> Result<(), slint_build::CompileError> {
    slint_build::compile("ui/app-window.slint")
}
