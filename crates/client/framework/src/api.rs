//! HTTP client for the gateway. Problem details become [`Error`]s again, so client code stays on the railway
//! and shows the translated text of the error's text key.

use redhive_contracts::api::ApiError;
use redhive_monads::{Error, ErrorKind, Res};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    base: Arc<RwLock<String>>,
    token: Arc<RwLock<Option<String>>>,
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new("http://localhost:8080")
    }
}

impl ApiClient {
    pub fn new(base: &str) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(60)).connect_timeout(Duration::from_secs(8)).user_agent(concat!("RedHive/", env!("CARGO_PKG_VERSION"))).build().expect("http client");
        ApiClient { http, base: Arc::new(RwLock::new(base.trim_end_matches('/').to_string())), token: Arc::new(RwLock::new(None)) }
    }

    pub fn set_base(&self, base: &str) {
        if let Ok(mut b) = self.base.write() {
            *b = base.trim_end_matches('/').to_string();
        }
    }

    pub fn base(&self) -> String {
        self.base.read().map(|b| b.clone()).unwrap_or_default()
    }

    pub fn set_token(&self, token: Option<String>) {
        if let Ok(mut t) = self.token.write() {
            *t = token;
        }
    }

    pub fn token(&self) -> Option<String> {
        self.token.read().ok().and_then(|t| t.clone())
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn url(&self, path: &str) -> String {
        if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", self.base(), path)
        }
    }

    pub async fn request<T: DeserializeOwned>(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Res<T> {
        let mut req = self.http.request(method, self.url(path));
        if let Some(t) = self.token() {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(transport)?;
        read(resp).await
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Res<T> {
        self.request(reqwest::Method::GET, path, None).await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Res<T> {
        self.request(reqwest::Method::POST, path, Some(serde_json::to_value(body).map_err(Error::from)?)).await
    }

    pub async fn put<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Res<T> {
        self.request(reqwest::Method::PUT, path, Some(serde_json::to_value(body).map_err(Error::from)?)).await
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Res<T> {
        self.request(reqwest::Method::DELETE, path, None).await
    }

    /// Raw bytes (packages, images).
    pub async fn bytes(&self, path: &str) -> Res<Vec<u8>> {
        let mut req = self.http.get(self.url(path));
        if let Some(t) = self.token() {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(transport)?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(transport)?;
        if status.is_success() {
            Ok(bytes.to_vec())
        } else {
            Err(problem(status.as_u16(), &bytes))
        }
    }
}

pub fn transport(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::timeout("client.timeout", "errors:timeout").with_message(e.to_string())
    } else {
        Error::unavailable("client.unavailable", "errors:serverUnavailable").with_message(e.to_string())
    }
}

fn problem(status: u16, bytes: &[u8]) -> Error {
    match serde_json::from_slice::<ApiError>(bytes) {
        Ok(api) => api.into_error(),
        Err(_) => Error::new(ErrorKind::from_status(status), format!("http.{status}"), "errors:unexpected").with_message(String::from_utf8_lossy(bytes).chars().take(200).collect::<String>()),
    }
}

async fn read<T: DeserializeOwned>(resp: reqwest::Response) -> Res<T> {
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(transport)?;
    if status.is_success() {
        let slice: &[u8] = if bytes.is_empty() { b"null" } else { &bytes };
        serde_json::from_slice(slice).map_err(|e| Error::unexpected("client.response", "errors:unexpected").with_message(e.to_string()))
    } else {
        Err(problem(status.as_u16(), &bytes))
    }
}
