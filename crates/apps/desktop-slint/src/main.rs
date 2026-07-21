//! Native entry point for the Slint frontend.

#![deny(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    match desktop_slint::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("desktop application failed: {error}");
            ExitCode::FAILURE
        }
    }
}
