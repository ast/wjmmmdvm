use std::path::Path;

use serde::Deserialize;

use crate::error::{Result, SipError};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub sip: SipConfig,
}

#[derive(Debug, Deserialize)]
pub struct SipConfig {
    /// Asterisk PBX host (IP or DNS name).
    pub server: String,
    /// SIP signalling port on the PBX. Defaults to 5060.
    #[serde(default = "default_server_port")]
    pub server_port: u16,
    /// SIP realm / domain used in the From URI. Often equals `server`.
    pub domain: String,
    /// AOR / endpoint user (e.g. "mmdvm").
    pub user: String,
    /// Auth username — typically equals `user`.
    pub auth_user: String,
    /// Auth password.
    pub password: String,
    /// Local UDP port to bind. 0 = ephemeral.
    #[serde(default)]
    pub local_port: u16,
    /// Desired registration expiry in seconds. Re-register at expires/2.
    #[serde(default = "default_expires")]
    pub expires: u32,
    /// User-Agent header value.
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

fn default_server_port() -> u16 {
    5060
}

fn default_expires() -> u32 {
    3600
}

fn default_user_agent() -> String {
    format!("mmdvm_sip/{}", env!("CARGO_PKG_VERSION"))
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(SipError::ConfigNotFound {
                path: path.to_path_buf(),
            });
        }
        let text = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&text)?;
        Ok(config)
    }
}
