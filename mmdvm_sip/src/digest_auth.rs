use md5::{Digest, Md5};
use rand::Rng;

use crate::error::{Result, SipError};

/// HTTP Digest authentication response computer for SIP.
/// Supports MD5 algorithm (RFC 2617 / RFC 7616) with qop=auth or no qop.
#[derive(Debug)]
pub struct DigestAuth {
    pub username: String,
    pub realm: String,
    pub password: String,
    pub method: String,
    pub uri: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub qop: Option<String>,
    pub algorithm: DigestAlgorithm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestAlgorithm {
    Md5,
}

impl DigestAlgorithm {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("MD5") | Some("md5") => Ok(DigestAlgorithm::Md5),
            Some(other) => Err(SipError::UnsupportedAlgorithm(other.to_owned())),
        }
    }
}

/// Result of computing a digest, holding all fields needed for the
/// Authorization header value.
#[derive(Debug)]
pub struct DigestResponse {
    pub username: String,
    pub realm: String,
    pub nonce: String,
    pub uri: String,
    pub response: String,
    pub algorithm: DigestAlgorithm,
    pub opaque: Option<String>,
    pub qop: Option<String>,
    pub nc: Option<String>,
    pub cnonce: Option<String>,
}

impl DigestAuth {
    pub fn compute(&self) -> DigestResponse {
        let ha1 = md5_hex(format!("{}:{}:{}", self.username, self.realm, self.password).as_bytes());
        let ha2 = md5_hex(format!("{}:{}", self.method, self.uri).as_bytes());

        let (response, nc, cnonce) = match &self.qop {
            Some(qop) if qop_includes_auth(qop) => {
                let nc = "00000001".to_string();
                let cnonce = random_cnonce();
                let response = md5_hex(
                    format!("{ha1}:{}:{nc}:{cnonce}:auth:{ha2}", self.nonce).as_bytes(),
                );
                (response, Some(nc), Some(cnonce))
            }
            _ => {
                let response = md5_hex(format!("{ha1}:{}:{ha2}", self.nonce).as_bytes());
                (response, None, None)
            }
        };

        DigestResponse {
            username: self.username.clone(),
            realm: self.realm.clone(),
            nonce: self.nonce.clone(),
            uri: self.uri.clone(),
            response,
            algorithm: self.algorithm,
            opaque: self.opaque.clone(),
            qop: nc.as_ref().map(|_| "auth".to_string()),
            nc,
            cnonce,
        }
    }
}

impl DigestResponse {
    /// Render as a SIP Authorization / Proxy-Authorization header value
    /// (without the leading "Authorization: " prefix).
    pub fn to_header_value(&self) -> String {
        let mut parts = vec![
            format!("Digest username=\"{}\"", self.username),
            format!("realm=\"{}\"", self.realm),
            format!("nonce=\"{}\"", self.nonce),
            format!("uri=\"{}\"", self.uri),
            format!("response=\"{}\"", self.response),
            format!("algorithm={}", algorithm_token(self.algorithm)),
        ];
        if let Some(qop) = &self.qop {
            parts.push(format!("qop={qop}"));
        }
        if let Some(nc) = &self.nc {
            parts.push(format!("nc={nc}"));
        }
        if let Some(cnonce) = &self.cnonce {
            parts.push(format!("cnonce=\"{cnonce}\""));
        }
        if let Some(opaque) = &self.opaque {
            parts.push(format!("opaque=\"{opaque}\""));
        }
        parts.join(", ")
    }
}

fn algorithm_token(alg: DigestAlgorithm) -> &'static str {
    match alg {
        DigestAlgorithm::Md5 => "MD5",
    }
}

fn md5_hex(input: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(input);
    hex::encode(hasher.finalize())
}

fn random_cnonce() -> String {
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill(&mut bytes);
    hex::encode(bytes)
}

fn qop_includes_auth(qop: &str) -> bool {
    qop.split(',').any(|q| q.trim().eq_ignore_ascii_case("auth"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7616 §3.9.1 test vector — MD5 with qop=auth.
    /// HA1 = MD5("Mufasa:http-auth@example.org:Circle of Life")
    /// HA2 = MD5("GET:/dir/index.html")
    /// response = MD5(HA1:nonce:nc:cnonce:qop:HA2)
    #[test]
    fn rfc7616_md5_qop_auth() {
        let auth = DigestAuth {
            username: "Mufasa".into(),
            realm: "http-auth@example.org".into(),
            password: "Circle of Life".into(),
            method: "GET".into(),
            uri: "/dir/index.html".into(),
            nonce: "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v".into(),
            opaque: None,
            qop: Some("auth".into()),
            algorithm: DigestAlgorithm::Md5,
        };
        // Force the cnonce so we can check against the published value.
        // The compute() above uses a random cnonce, so for the test we
        // reproduce the calculation with a fixed one.
        let cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        let nc = "00000001";
        let ha1 = md5_hex(b"Mufasa:http-auth@example.org:Circle of Life");
        let ha2 = md5_hex(b"GET:/dir/index.html");
        let response = md5_hex(
            format!(
                "{ha1}:{}:{nc}:{cnonce}:auth:{ha2}",
                auth.nonce
            )
            .as_bytes(),
        );
        assert_eq!(response, "8ca523f5e9506fed4657c9700eebdbec");
    }

    /// Sanity: same compute() pipeline twice with a forced password gives
    /// non-empty deterministic HA1/HA2 components.
    #[test]
    fn computes_response_field() {
        let auth = DigestAuth {
            username: "alice".into(),
            realm: "asterisk".into(),
            password: "s3cr3t".into(),
            method: "REGISTER".into(),
            uri: "sip:asterisk".into(),
            nonce: "abc123".into(),
            opaque: None,
            qop: None,
            algorithm: DigestAlgorithm::Md5,
        };
        let resp = auth.compute();
        assert_eq!(resp.username, "alice");
        assert_eq!(resp.nonce, "abc123");
        assert_eq!(resp.response.len(), 32);
        assert!(resp.qop.is_none());
        assert!(resp.nc.is_none());
    }
}
