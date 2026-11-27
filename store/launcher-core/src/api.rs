use std::{io::Read, time::Duration};

use serde::{Deserialize, de::DeserializeOwned};

pub const DEFAULT_API: &str = "https://publish.kitgameengine.win";

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CatalogResponse {
    #[serde(default)]
    pub games: Vec<CatalogEntry>,
    #[serde(default)]
    pub total: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CatalogEntry {
    pub id: String,
    #[serde(alias = "title")]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "cover")]
    pub icon_url: Option<String>,
    #[serde(default, alias = "release")]
    pub latest: Option<Release>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Release {
    #[serde(alias = "revisionId", default)]
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub files: Vec<ReleaseFile>,
    #[serde(default, alias = "downloadUrl")]
    pub download_url: Option<String>,
    #[serde(default)]
    pub executable: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default, alias = "sizeBytes")]
    pub size: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ReleaseFile {
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub executable: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ApiClient {
    base: String,
    agent: ureq::Agent,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ApiError {}

impl ApiClient {
    pub fn new(base: impl Into<String>) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(600)))
            .user_agent(concat!("Kit/", env!("CARGO_PKG_VERSION"), " kit-launcher-core"))
            .build()
            .into();
        Self { base: base.into().trim_end_matches('/').to_owned(), agent }
    }

    pub fn default() -> Self {
        Self::new(DEFAULT_API)
    }
    pub fn base(&self) -> &str {
        &self.base
    }

    fn get(&self, path: &str) -> Result<ureq::http::Response<ureq::Body>, ApiError> {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_owned()
        } else {
            format!("{}{}", self.base, if path.starts_with('/') { path.to_owned() } else { format!("/{path}") })
        };
        let response = self.agent.get(&url).call().map_err(|e| ApiError { status: 0, message: e.to_string() })?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(ApiError {
                status: response.status().as_u16(),
                message: format!("Store API returned {}", response.status()),
            })
        }
    }

    pub fn catalog(&self) -> Result<CatalogResponse, ApiError> {
        let mut response = self.get("/api/v1/catalog")?;
        response
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .read_json()
            .map_err(|e| ApiError { status: 0, message: e.to_string() })
    }

    pub fn release(&self, id: &str, version: &str) -> Result<Release, ApiError> {
        self.json(&format!("/api/v1/games/{}/releases/{}", encode(id), encode(version)))
    }

    pub fn download(&self, file: &ReleaseFile, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, ApiError> {
        let mut response = self.get(&file.url)?;
        let mut reader = response.body_mut().with_config().limit(file.size.saturating_add(1)).reader();
        let mut bytes = Vec::with_capacity(file.size as usize);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| ApiError { status: 0, message: e.to_string() })?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
            progress(bytes.len() as u64, file.size);
        }
        if bytes.len() as u64 != file.size {
            return Err(ApiError {
                status: 0,
                message: format!("download size mismatch: got {}, expected {}", bytes.len(), file.size),
            });
        }
        Ok(bytes)
    }

    pub fn download_release(&self, release: &Release, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, ApiError> {
        let url = release
            .download_url
            .as_ref()
            .ok_or_else(|| ApiError { status: 0, message: "release has no download URL".into() })?;
        let size =
            release.size.ok_or_else(|| ApiError { status: 0, message: "release has no download size".into() })?;
        let sha256 =
            release.sha256.clone().ok_or_else(|| ApiError { status: 0, message: "release has no SHA-256".into() })?;
        self.download(
            &ReleaseFile {
                platform: "linux-x86_64".into(),
                url: url.clone(),
                sha256,
                size,
                executable: release.executable.clone(),
            },
            |done, total| progress(done, total),
        )
    }

    fn json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let mut response = self.get(path)?;
        response
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .read_json()
            .map_err(|e| ApiError { status: 0, message: e.to_string() })
    }
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
