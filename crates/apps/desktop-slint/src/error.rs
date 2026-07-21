//! Slint frontend startup and platform failures.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use application_runtime::ApplicationError;

/// Failure while starting, running, or stopping the Slint frontend.
#[derive(Debug)]
pub enum DesktopError {
    /// Frontend-neutral application runtime failed.
    Application(ApplicationError),
    /// Slint platform or event-loop operation failed.
    Slint(slint::PlatformError),
    /// A supported per-user data directory could not be resolved.
    MissingDataDirectory,
    /// The application-state directory could not be created.
    CreateDataDirectory(std::io::Error),
}

impl Display for DesktopError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Application(error) => Display::fmt(error, formatter),
            Self::Slint(error) => write!(formatter, "Slint failure: {error}"),
            Self::MissingDataDirectory => {
                formatter.write_str("no supported per-user application data directory is available")
            }
            Self::CreateDataDirectory(error) => {
                write!(
                    formatter,
                    "failed to create application data directory: {error}"
                )
            }
        }
    }
}

impl Error for DesktopError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Application(error) => Some(error),
            Self::Slint(error) => Some(error),
            Self::CreateDataDirectory(error) => Some(error),
            Self::MissingDataDirectory => None,
        }
    }
}

impl From<ApplicationError> for DesktopError {
    fn from(value: ApplicationError) -> Self {
        Self::Application(value)
    }
}

impl From<slint::PlatformError> for DesktopError {
    fn from(value: slint::PlatformError) -> Self {
        Self::Slint(value)
    }
}
