//! The one place the network is touched: [`Client`] talks to GitHub and
//! nowhere else, over bundled roots.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Names the software making the request. GitHub's API rejects a request
/// carrying no `User-Agent`.
pub const USER_AGENT: &str = concat!(
    "readinglog/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/huangziwei/readinglog)"
);

/// Time to a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Time to the response head. A quiet socket past this is not coming back.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// Whole-body time for the release asset.
const BODY_TIMEOUT: Duration = Duration::from_secs(600);

/// Refuse a release list larger than this. Thirty releases run to ~100 KB.
const MAX_TEXT: u64 = 4 * 1024 * 1024;
/// Refuse an asset larger than this. The release zip is a few megabytes.
const MAX_ASSET: u64 = 64 * 1024 * 1024;

/// Bytes per read from the socket, and per [`Client::download`] progress.
const CHUNK: usize = 64 * 1024;

#[derive(Debug)]
pub enum Error {
    /// Could not reach the host at all — no route, no DNS, no TLS.
    Unreachable(String),
    /// Reached GitHub and it said no.
    Status { code: u16, url: String },
    /// Reached it, but the body was unreadable, absurdly large, or could not
    /// be written where it was going.
    Body(String),
    /// A tap on the banner, mid-transfer.
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unreachable(e) => write!(f, "cannot reach github.com: {e}"),
            Error::Status { code, url } => write!(f, "github.com returned {code} for {url}"),
            Error::Body(e) => write!(f, "unreadable response: {e}"),
            Error::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// One short line for the banner.
    pub fn hint(&self) -> &'static str {
        match self {
            Error::Unreachable(_) => "no network",
            Error::Status { code: 403, .. } | Error::Status { code: 429, .. } => "rate limited",
            Error::Status { code: 404, .. } => "not found",
            Error::Status { .. } => "GitHub said no",
            Error::Body(_) => "bad reply",
            Error::Cancelled => "cancelled",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Client {
    agent: ureq::Agent,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        // ureq is built with `rustls-no-provider`: a missing provider is a
        // compile error.
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .unversioned_rustls_crypto_provider(Arc::new(rustls_rustcrypto::provider()))
            .build();

        let config = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(BODY_TIMEOUT))
            .tls_config(tls)
            .build();

        Client {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// A release list, or a `.sha256` sidecar. Read into memory.
    pub fn text(&self, url: &str, accept: &str) -> Result<String> {
        let mut res = self
            .agent
            .get(url)
            .header("Accept", accept)
            .call()
            .map_err(|e| classify(e, url))?;
        res.body_mut()
            .with_config()
            .limit(MAX_TEXT)
            .read_to_string()
            .map_err(|e| Error::Body(e.to_string()))
    }

    /// A release asset, straight to `dest`. `progress` takes the bytes
    /// transferred and the declared length. A partial file is removed on
    /// failure.
    pub fn download(
        &self,
        url: &str,
        dest: &Path,
        cancel: &AtomicBool,
        progress: &dyn Fn(u64, Option<u64>),
    ) -> Result<u64> {
        let mut res = self.agent.get(url).call().map_err(|e| classify(e, url))?;
        let total = res
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let written = self.stream(&mut res, dest, total, cancel, progress);
        if written.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        written
    }

    fn stream(
        &self,
        res: &mut ureq::http::Response<ureq::Body>,
        dest: &Path,
        total: Option<u64>,
        cancel: &AtomicBool,
        progress: &dyn Fn(u64, Option<u64>),
    ) -> Result<u64> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Body(e.to_string()))?;
        }
        let file = File::create(dest).map_err(|e| Error::Body(e.to_string()))?;
        let mut sink = BufWriter::new(file);
        let mut source = res.body_mut().as_reader();

        let mut buf = vec![0u8; CHUNK];
        let mut got = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let n = source
                .read(&mut buf)
                .map_err(|e| Error::Body(e.to_string()))?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > MAX_ASSET {
                return Err(Error::Body(format!("over {MAX_ASSET} bytes")));
            }
            sink.write_all(&buf[..n])
                .map_err(|e| Error::Body(e.to_string()))?;
            progress(got, total);
        }
        sink.flush().map_err(|e| Error::Body(e.to_string()))?;

        // A transfer cut short writes a whole-looking file.
        if total.is_some_and(|want| want != got) {
            return Err(Error::Body(format!(
                "{got} bytes of {}",
                total.unwrap_or_default()
            )));
        }
        Ok(got)
    }
}

fn classify(e: ureq::Error, url: &str) -> Error {
    match e {
        ureq::Error::StatusCode(code) => Error::Status {
            code,
            url: url.to_string(),
        },
        // Everything else — DNS, refused connection, TLS failure, timeout —
        // is the device not being able to reach GitHub, which is the one
        // distinction the banner acts on.
        other => Error::Unreachable(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_agent_names_the_software_and_resolves() {
        assert!(USER_AGENT.starts_with("readinglog/"));
        // GitHub rejects a request with no User-Agent, and a `+URL` that 404s
        // reads as a fake courtesy. This one points at the repository rather
        // than at whoever is holding the Kindle.
        assert!(USER_AGENT.contains(super::super::REPO));
        assert!(!USER_AGENT.contains("Mozilla"), "no browser spoofing");
    }

    #[test]
    fn every_failure_has_a_line_short_enough_for_the_banner() {
        let failures = [
            Error::Unreachable("dns".into()),
            Error::Status {
                code: 403,
                url: "u".into(),
            },
            Error::Status {
                code: 404,
                url: "u".into(),
            },
            Error::Status {
                code: 500,
                url: "u".into(),
            },
            Error::Body("truncated".into()),
            Error::Cancelled,
        ];
        for failure in &failures {
            let hint = failure.hint();
            assert!(!hint.is_empty() && hint.len() <= 20, "{hint}");
            assert!(!failure.to_string().is_empty());
        }
        // Rate limiting is what an unauthenticated caller actually hits, and
        // it is worth telling apart from a repository that moved.
        assert_ne!(
            failures[1].hint(),
            Error::Status {
                code: 404,
                url: "u".into()
            }
            .hint()
        );
    }
}
