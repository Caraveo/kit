//! The Circuit API client: list Circuits, read their releases and download
//! packages. Requests carry the developer's existing Rex sign-in token.

use std::io::Read;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

use crate::package::{PublisherKey, ReleaseManifest};

/// The Circuit API (`KIT_CIRCUIT_API` overrides it, e.g. for `wrangler dev`).
pub const DEFAULT_API: &str = "https://circuit.kitgameengine.win/api/v1";
/// The same service on its workers.dev address, used while DNS settles.
pub const FALLBACK_API: &str = "https://kit-ai.dios-39b.workers.dev/api/v1";

pub fn api_base() -> String {
    std::env::var("KIT_CIRCUIT_API").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_API.to_owned())
}

#[derive(Clone, Debug, Deserialize)]
pub struct Publisher {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub official: bool,
    #[serde(default)]
    pub keys: Vec<PublisherKey>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseSummary {
    pub version: String,
    pub channel: String,
    pub kit: String,
    pub platforms: Vec<String>,
    pub published_at: i64,
    #[serde(default)]
    pub yanked: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Circuit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub publisher: Publisher,
    #[serde(default)]
    pub official: bool,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub documentation: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub latest: Option<ReleaseSummary>,
    #[serde(default)]
    pub release_count: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    #[serde(flatten)]
    pub summary: ReleaseSummary,
    /// The full release manifest (circuit.toml + package checks + notes).
    pub release: ReleaseManifest,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Page {
    pub circuits: Vec<Circuit>,
    pub total: u32,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Clone, Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    fn network(e: impl std::fmt::Display) -> Self {
        Self { status: 0, code: "network".into(), message: format!("Couldn't reach the Circuit API: {e}") }
    }
    /// The sign-in is missing, expired or revoked.
    pub fn needs_sign_in(&self) -> bool {
        self.status == 401
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}
#[derive(Deserialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

#[derive(Clone)]
pub struct Client {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

/// List filters.
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub search: String,
    pub official_only: bool,
    pub platform: Option<String>,
    pub page: u32,
}

impl Client {
    pub fn new(base: impl Into<String>, token: Option<String>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(600)))
            .user_agent(concat!("Kit/", env!("CARGO_PKG_VERSION"), " kit_circuit"))
            .build()
            .into();
        Self { base: base.into().trim_end_matches('/').to_owned(), token, agent }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn get(&self, url: &str) -> Result<ureq::http::Response<ureq::Body>, ApiError> {
        let mut req = self.agent.get(url);
        if let Some(t) = &self.token {
            req = req.header("authorization", &format!("Bearer {t}"));
        }
        let resp = req.call().map_err(ApiError::network)?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status().as_u16();
        let mut resp = resp;
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        Err(match serde_json::from_str::<ErrorBody>(&text) {
            Ok(b) => ApiError { status, code: b.error.code, message: b.error.message },
            Err(_) => ApiError { status, code: format!("http_{status}"), message: format!("The Circuit API answered {status}.") },
        })
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, ApiError> {
        let mut resp = self.get(&format!("{}{path}", self.base))?;
        resp.body_mut().with_config().limit(32 * 1024 * 1024).read_json::<T>().map_err(ApiError::network)
    }

    pub fn list(&self, q: &Query) -> Result<Page, ApiError> {
        let mut path = format!("/circuits?page={}&per_page=50", q.page.max(1));
        if !q.search.trim().is_empty() {
            path.push_str(&format!("&q={}", encode(q.search.trim())));
        }
        if q.official_only {
            path.push_str("&official=1");
        }
        if let Some(p) = &q.platform {
            path.push_str(&format!("&platform={}", encode(p)));
        }
        self.get_json(&path)
    }

    pub fn circuit(&self, id: &str) -> Result<Circuit, ApiError> {
        #[derive(Deserialize)]
        struct R {
            circuit: Circuit,
        }
        Ok(self.get_json::<R>(&format!("/circuits/{}", encode(id)))?.circuit)
    }

    pub fn releases(&self, id: &str) -> Result<Vec<Release>, ApiError> {
        #[derive(Deserialize)]
        struct R {
            releases: Vec<Release>,
        }
        Ok(self.get_json::<R>(&format!("/circuits/{}/releases", encode(id)))?.releases)
    }

    pub fn download_url(&self, id: &str, version: &str) -> String {
        format!("{}/circuits/{}/releases/{}/download", self.base, encode(id), encode(version))
    }

    /// Download a package, reporting `(bytes so far, total)`. The caller
    /// verifies it with [`crate::package::verify`] before installing.
    pub fn download(&self, id: &str, version: &str, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, ApiError> {
        #[derive(Deserialize)]
        struct Link {
            url: String,
            size: u64,
        }
        // Ask the API where the package is (it checks the sign-in), then fetch
        // it without sending the token to the file host.
        let link: Link = self.get_json(&format!("/circuits/{}/releases/{}/download?format=json", encode(id), encode(version)))?;
        if link.size > crate::package::MAX_PACKAGE_BYTES {
            return Err(ApiError { status: 0, code: "too_large".into(), message: "The package is larger than Kit allows.".into() });
        }
        let resp = self.agent.get(&link.url).call().map_err(ApiError::network)?;
        if !resp.status().is_success() {
            return Err(ApiError { status: resp.status().as_u16(), code: "download".into(), message: format!("The download failed ({}).", resp.status()) });
        }
        let mut body = resp.into_body();
        let mut reader = body.with_config().limit(crate::package::MAX_PACKAGE_BYTES + 1).reader();
        let mut out = Vec::with_capacity(link.size as usize);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(ApiError::network)?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            progress(out.len() as u64, link.size);
        }
        Ok(out)
    }
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_query_values() {
        assert_eq!(super::encode("kit.multiplayer"), "kit.multiplayer");
        assert_eq!(super::encode("a b&c"), "a%20b%26c");
    }
}
