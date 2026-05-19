use std::net::SocketAddr;
use std::time::Duration;

use rand::Rng;
use rsip::{Response, SipMessage};
use tracing::{debug, info, warn};

use crate::config::SipConfig;
use crate::digest_auth::{DigestAlgorithm, DigestAuth};
use crate::error::{Result, SipError};
use crate::udp_transport::UdpTransport;

/// Owns per-AOR state (Call-ID, From tag, CSeq counter) and drives the
/// REGISTER → 401 → REGISTER+auth → 200 flow against the configured PBX.
pub struct Registration {
    config: SipConfig,
    local: SocketAddr,
    call_id: String,
    from_tag: String,
    cseq: u32,
}

/// Result of a single register attempt cycle.
#[derive(Debug)]
pub struct Registered {
    /// Expires value the PBX confirmed.
    pub expires: u32,
}

impl Registration {
    pub fn new(config: SipConfig, local: SocketAddr) -> Self {
        Self {
            config,
            local,
            call_id: format!("{}@mmdvm_sip", random_hex(12)),
            from_tag: random_hex(8),
            cseq: 0,
        }
    }

    pub fn config(&self) -> &SipConfig {
        &self.config
    }

    /// Send a REGISTER, follow a 401 challenge if needed, return on 200 OK.
    pub async fn perform(&mut self, transport: &UdpTransport) -> Result<Registered> {
        let request = self.build_register(None);
        debug!(target: "mmdvm_sip::registration", "sending unauthenticated REGISTER:\n{request}");
        transport.send(request.as_bytes()).await?;

        let response = recv_response(transport).await?;
        match response.status_code.kind() {
            rsip::StatusCodeKind::Successful => Ok(Registered {
                expires: extract_expires(&response).unwrap_or(self.config.expires),
            }),
            rsip::StatusCodeKind::RequestFailure
                if response.status_code == rsip::StatusCode::Unauthorized
                    || response.status_code == rsip::StatusCode::ProxyAuthenticationRequired =>
            {
                self.handle_challenge(transport, &response).await
            }
            _ => Err(SipError::RegistrationRejected {
                status: u16::from(response.status_code.clone()),
                reason: response.status_code.to_string(),
            }),
        }
    }

    async fn handle_challenge(
        &mut self,
        transport: &UdpTransport,
        response: &Response,
    ) -> Result<Registered> {
        let challenge = parse_challenge(response)?;
        info!(target: "mmdvm_sip::registration",
            realm = %challenge.realm,
            qop = ?challenge.qop,
            "got auth challenge, retrying with digest"
        );

        let auth = DigestAuth {
            username: self.config.auth_user.clone(),
            realm: challenge.realm,
            password: self.config.password.clone(),
            method: "REGISTER".into(),
            uri: format!("sip:{}", self.config.domain),
            nonce: challenge.nonce,
            opaque: challenge.opaque,
            qop: challenge.qop,
            algorithm: DigestAlgorithm::parse(challenge.algorithm.as_deref())?,
        };
        let digest = auth.compute();
        let auth_header = digest.to_header_value();

        let request = self.build_register(Some(&auth_header));
        debug!(target: "mmdvm_sip::registration", "sending authenticated REGISTER:\n{request}");
        transport.send(request.as_bytes()).await?;

        let response = recv_response(transport).await?;
        if let rsip::StatusCodeKind::Successful = response.status_code.kind() {
            Ok(Registered {
                expires: extract_expires(&response).unwrap_or(self.config.expires),
            })
        } else {
            Err(SipError::RegistrationRejected {
                status: u16::from(response.status_code.clone()),
                reason: response.status_code.to_string(),
            })
        }
    }

    fn build_register(&mut self, auth_header: Option<&str>) -> String {
        self.cseq += 1;
        let server = &self.config.server;
        let _server_port = self.config.server_port;
        let user = &self.config.user;
        let domain = &self.config.domain;
        let local_ip = self.local.ip();
        let local_port = self.local.port();
        let cseq = self.cseq;
        let call_id = &self.call_id;
        let from_tag = &self.from_tag;
        let user_agent = &self.config.user_agent;
        let expires = self.config.expires;
        let branch = format!("z9hG4bK{}", random_hex(8));

        let auth_line = match auth_header {
            Some(h) => format!("Authorization: {h}\r\n"),
            None => String::new(),
        };

        format!(
            "REGISTER sip:{server} SIP/2.0\r\n\
             Via: SIP/2.0/UDP {local_ip}:{local_port};rport;branch={branch}\r\n\
             Max-Forwards: 70\r\n\
             From: <sip:{user}@{domain}>;tag={from_tag}\r\n\
             To: <sip:{user}@{domain}>\r\n\
             Call-ID: {call_id}\r\n\
             CSeq: {cseq} REGISTER\r\n\
             Contact: <sip:{user}@{local_ip}:{local_port}>\r\n\
             Expires: {expires}\r\n\
             Allow: REGISTER, INVITE, ACK, BYE, CANCEL, OPTIONS\r\n\
             User-Agent: {user_agent}\r\n\
             {auth_line}\
             Content-Length: 0\r\n\
             \r\n"
        )
    }
}

async fn recv_response(transport: &UdpTransport) -> Result<Response> {
    // 100 Trying / 1xx provisional responses may arrive first — drain them.
    loop {
        let message = transport.recv(Duration::from_secs(5)).await?;
        let response = match message {
            SipMessage::Response(r) => r,
            SipMessage::Request(r) => {
                warn!(target: "mmdvm_sip::registration",
                    method = %r.method,
                    "expected response, got request — ignoring");
                continue;
            }
        };
        if let rsip::StatusCodeKind::Provisional = response.status_code.kind() {
            debug!(target: "mmdvm_sip::registration",
                status = %response.status_code,
                "got provisional response, waiting for final");
            continue;
        }
        return Ok(response);
    }
}

#[derive(Debug)]
struct Challenge {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    qop: Option<String>,
    algorithm: Option<String>,
}

fn parse_challenge(response: &Response) -> Result<Challenge> {
    use rsip::headers::UntypedHeader;
    use rsip::prelude::HeadersExt;
    let header = response
        .www_authenticate_header()
        .ok_or(SipError::MissingHeader("WWW-Authenticate"))?;
    let raw = header.value();
    let stripped = raw
        .trim_start()
        .strip_prefix("Digest")
        .ok_or_else(|| SipError::SipParse("WWW-Authenticate not a Digest challenge".into()))?
        .trim_start();

    let mut realm = None;
    let mut nonce = None;
    let mut opaque = None;
    let mut qop = None;
    let mut algorithm = None;
    for (k, v) in parse_params(stripped) {
        match k.as_str() {
            "realm" => realm = Some(v),
            "nonce" => nonce = Some(v),
            "opaque" => opaque = Some(v),
            "qop" => qop = Some(v),
            "algorithm" => algorithm = Some(v),
            _ => {}
        }
    }

    Ok(Challenge {
        realm: realm.ok_or_else(|| SipError::SipParse("challenge missing realm".into()))?,
        nonce: nonce.ok_or_else(|| SipError::SipParse("challenge missing nonce".into()))?,
        opaque,
        qop,
        algorithm,
    })
}

/// Parse `key="value", key2=value2` style auth parameters. Values may be
/// quoted or unquoted; whitespace around `=` and `,` is allowed.
fn parse_params(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b',' || bytes[i] == b'\t') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && bytes[i] != b',' {
            i += 1;
        }
        let key = input[key_start..i].trim().to_ascii_lowercase();
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        let value = if i < bytes.len() && bytes[i] == b'"' {
            i += 1;
            let value_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let v = input[value_start..i].to_string();
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let value_start = i;
            while i < bytes.len() && bytes[i] != b',' {
                i += 1;
            }
            input[value_start..i].trim().to_string()
        };
        out.push((key, value));
    }
    out
}

fn extract_expires(response: &Response) -> Option<u32> {
    for header in response.headers.iter() {
        if let rsip::Header::Expires(e) = header {
            if let Ok(n) = e.seconds() {
                return Some(n);
            }
        }
    }
    // Fallback: look for expires=N inside the Contact header.
    use rsip::headers::UntypedHeader;
    for header in response.headers.iter() {
        if let rsip::Header::Contact(c) = header {
            let raw = c.value();
            for (k, v) in parse_params(raw) {
                if k == "expires" {
                    if let Ok(n) = v.parse::<u32>() {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill(&mut buf[..]);
    hex::encode(buf)
}

#[cfg(test)]
mod tests {
    use super::parse_params;

    #[test]
    fn parses_quoted_and_unquoted_params() {
        let input = r#"realm="asterisk", nonce="abc123", algorithm=MD5, qop="auth""#;
        let params: Vec<_> = parse_params(input)
            .into_iter()
            .collect();
        assert_eq!(params[0], ("realm".into(), "asterisk".into()));
        assert_eq!(params[1], ("nonce".into(), "abc123".into()));
        assert_eq!(params[2], ("algorithm".into(), "MD5".into()));
        assert_eq!(params[3], ("qop".into(), "auth".into()));
    }
}
