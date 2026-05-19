use std::net::SocketAddr;
use std::time::Duration;

use rsip::SipMessage;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::error::{Result, SipError};

/// One-peer UDP SIP transport. Connected to a fixed remote address so we
/// can use `send`/`recv` without re-specifying it on every call.
pub struct UdpTransport {
    socket: UdpSocket,
    remote: SocketAddr,
    local: SocketAddr,
}

impl UdpTransport {
    pub async fn bind_and_connect(local_port: u16, remote: SocketAddr) -> Result<Self> {
        let bind_addr: SocketAddr = format!("0.0.0.0:{local_port}")
            .parse()
            .expect("hardcoded bind addr parses");
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.connect(remote).await?;
        let local = socket.local_addr()?;
        Ok(Self { socket, remote, local })
    }

    pub fn local(&self) -> SocketAddr {
        self.local
    }

    pub fn remote(&self) -> SocketAddr {
        self.remote
    }

    pub async fn send(&self, bytes: &[u8]) -> Result<()> {
        self.socket.send(bytes).await?;
        Ok(())
    }

    /// Receive the next datagram and parse it as a SIP message. Times out
    /// after `wait` to avoid blocking forever if the peer never replies.
    pub async fn recv(&self, wait: Duration) -> Result<SipMessage> {
        let mut buf = vec![0u8; 4096];
        let n = timeout(wait, self.socket.recv(&mut buf))
            .await
            .map_err(|_| SipError::ResponseTimeout)??;
        let msg = SipMessage::try_from(&buf[..n])?;
        Ok(msg)
    }
}
