use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SipError {
    #[error("config file not found: {path}")]
    ConfigNotFound { path: PathBuf },

    #[error("config parse error: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("SIP parse error: {0}")]
    SipParse(String),

    #[error("missing required header: {0}")]
    MissingHeader(&'static str),

    #[error("unexpected response: {status}")]
    UnexpectedResponse { status: u16 },

    #[error("authentication required but no credentials configured")]
    AuthRequiredButMissing,

    #[error("unsupported digest algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("registration rejected: {status} {reason}")]
    RegistrationRejected { status: u16, reason: String },

    #[error("timed out waiting for response")]
    ResponseTimeout,
}

impl From<rsip::Error> for SipError {
    fn from(value: rsip::Error) -> Self {
        SipError::SipParse(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SipError>;
