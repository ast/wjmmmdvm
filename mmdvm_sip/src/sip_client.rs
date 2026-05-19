use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::error::{Result, SipError};
use crate::registration::Registration;
use crate::udp_transport::UdpTransport;

/// Top-level orchestrator: owns the UDP transport and the Registration
/// state machine. Runs an infinite register/refresh loop until cancelled.
pub struct SipClient {
    transport: UdpTransport,
    registration: Registration,
}

impl SipClient {
    pub async fn new(config: Config) -> Result<Self> {
        let remote = resolve_remote(&config.sip.server, config.sip.server_port)?;
        info!(target: "mmdvm_sip", %remote, "resolved Asterisk endpoint");

        let transport = UdpTransport::bind_and_connect(config.sip.local_port, remote).await?;
        info!(target: "mmdvm_sip", local = %transport.local(), "bound local UDP socket");

        let registration = Registration::new(config.sip, transport.local());
        Ok(Self {
            transport,
            registration,
        })
    }

    pub async fn run_register_loop(&mut self) -> anyhow::Result<()> {
        let mut backoff = Duration::from_secs(5);
        let max_backoff = Duration::from_secs(60);
        loop {
            match self.registration.perform(&self.transport).await {
                Ok(registered) => {
                    // Refresh at expires/2, but no later than 5 s before
                    // the contact actually expires and no sooner than 30 s.
                    let half = registered.expires / 2;
                    let refresh_in = half.min(registered.expires.saturating_sub(5)).max(30);
                    info!(
                        target: "mmdvm_sip",
                        expires = registered.expires,
                        refresh_in = refresh_in,
                        user = %self.registration.config().user,
                        "registered — next refresh in {refresh_in}s"
                    );
                    backoff = Duration::from_secs(5);
                    sleep(Duration::from_secs(refresh_in as u64)).await;
                }
                Err(SipError::ResponseTimeout) => {
                    warn!(target: "mmdvm_sip", "register timed out, retrying in {:?}", backoff);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                }
                Err(e) => {
                    error!(target: "mmdvm_sip", error = %e, "register failed, retrying in {:?}", backoff);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                }
            }
        }
    }
}

fn resolve_remote(host: &str, port: u16) -> Result<SocketAddr> {
    (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| {
            SipError::SipParse(format!("could not resolve {host}:{port}"))
        })
}
