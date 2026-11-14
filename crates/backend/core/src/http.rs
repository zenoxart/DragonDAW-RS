//! `Result` → HTTP and service-to-service calls.

use crate::hosting::{RedHiveOptions, CORRELATION_HEADER};
use crate::security::INTERNAL_KEY_HEADER;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use redhive_contracts::api::ApiError;
use redhive_monads::{Error, ErrorKind, Res};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// An [`Error`] as an HTTP response (problem details, `application/problem+json`).
#[derive(Debug)]
pub struct AppError(pub Error);

impl From<Error> for AppError {
    fn from(e: Error) -> Self {
        AppError(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        problem(&self.0)
    }
}

/// Builds the problem-details response of an error and logs it by severity.
pub fn problem(error: &Error) -> Response {
    match error.kind {
        ErrorKind::Unexpected => tracing::error!(code = %error.code, message = ?error.message, "unexpected error"),
        ErrorKind::Unavailable | ErrorKind::Timeout => tracing::warn!(code = %error.code, message = ?error.message, "dependency failed"),
        _ => tracing::debug!(code = %error.code, "request failed"),
    }
    let body = ApiError::from_error(error, None);
    let status = StatusCode::from_u16(body.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static("application/problem+json"));
    response
}

/// Handler return type: `Ok` → 200 + JSON, `Err` → problem details.
pub type ApiResult<T> = Result<Json<T>, AppError>;

/// Extension to turn a use-case result into a handler result.
pub trait ToHttp<T> {
    fn http(self) -> ApiResult<T>;
}

impl<T> ToHttp<T> for Res<T> {
    fn http(self) -> ApiResult<T> {
        self.map(Json).map_err(AppError)
    }
}

/// 201 Created with a JSON body.
pub struct Created<T>(pub T);

impl<T: Serialize> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        (StatusCode::CREATED, Json(self.0)).into_response()
    }
}

/// 204 No Content.
pub struct NoContent;

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        StatusCode::NO_CONTENT.into_response()
    }
}

/// How a service call authenticates.
#[derive(Debug, Clone)]
pub enum Auth {
    /// The internal key (for `/internal/*` endpoints).
    Internal,
    /// A user's (or delegated) bearer token.
    Bearer(String),
    None,
}

/// HTTP client for calls between Red Hive services. Maps transport failures and problem details back
/// into [`Error`]s, so callers keep working on the railway.
#[derive(Clone)]
pub struct ServiceClient {
    http: reqwest::Client,
    options: Arc<RedHiveOptions>,
}

impl ServiceClient {
    pub fn new(options: Arc<RedHiveOptions>) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).connect_timeout(Duration::from_secs(5)).build().expect("http client");
        ServiceClient { http, options }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Absolute URL of `path` on `service`. `path` may already be absolute (`http://…`).
    pub fn url(&self, service: &str, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.options.service_url(service), path)
        }
    }

    pub async fn send<T: DeserializeOwned>(&self, method: reqwest::Method, url: &str, body: Option<&serde_json::Value>, auth: &Auth, correlation: Option<&str>) -> Res<T> {
        let mut req = self.http.request(method, url);
        req = match auth {
            Auth::Internal => req.header(INTERNAL_KEY_HEADER, &self.options.internal_key),
            Auth::Bearer(t) => req.bearer_auth(t),
            Auth::None => req,
        };
        if let Some(c) = correlation.filter(|c| !c.is_empty()) {
            req = req.header(CORRELATION_HEADER, c);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| transport_error(url, e))?;
        read_response(url, resp).await
    }

    pub async fn get<T: DeserializeOwned>(&self, service: &str, path: &str, auth: &Auth) -> Res<T> {
        self.send(reqwest::Method::GET, &self.url(service, path), None, auth, None).await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, service: &str, path: &str, body: &B, auth: &Auth) -> Res<T> {
        let body = serde_json::to_value(body).map_err(Error::from)?;
        self.send(reqwest::Method::POST, &self.url(service, path), Some(&body), auth, None).await
    }

    pub async fn put<T: DeserializeOwned, B: Serialize>(&self, service: &str, path: &str, body: &B, auth: &Auth) -> Res<T> {
        let body = serde_json::to_value(body).map_err(Error::from)?;
        self.send(reqwest::Method::PUT, &self.url(service, path), Some(&body), auth, None).await
    }

    /// GET with the internal key.
    pub async fn get_internal<T: DeserializeOwned>(&self, service: &str, path: &str) -> Res<T> {
        self.get(service, path, &Auth::Internal).await
    }

    /// POST with the internal key.
    pub async fn post_internal<T: DeserializeOwned, B: Serialize>(&self, service: &str, path: &str, body: &B) -> Res<T> {
        self.post(service, path, body, &Auth::Internal).await
    }
}

pub fn transport_error(url: &str, e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::timeout("service.timeout", "errors:timeout").with_message(format!("{url}: {e}"))
    } else {
        Error::unavailable("service.unavailable", "errors:unavailable").with_message(format!("{url}: {e}"))
    }
}

/// Reads a JSON response; non-2xx bodies are parsed as problem details.
pub async fn read_response<T: DeserializeOwned>(url: &str, resp: reqwest::Response) -> Res<T> {
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| transport_error(url, e))?;
    if status.is_success() {
        let slice: &[u8] = if bytes.is_empty() { b"null" } else { &bytes };
        return serde_json::from_slice(slice).map_err(|e| Error::unexpected("service.response.invalid", "errors:unexpected").with_message(format!("{url}: {e}")));
    }
    match serde_json::from_slice::<ApiError>(&bytes) {
        Ok(api) => Err(api.into_error()),
        Err(_) => Err(Error::new(ErrorKind::from_status(status.as_u16()), format!("http.{}", status.as_u16()), "errors:unexpected")
            .with_message(format!("{url}: {} {}", status, String::from_utf8_lossy(&bytes).chars().take(300).collect::<String>()))),
    }
}

/// A JSON request body; parse errors become problem details (`json.invalid`) instead of plain-text rejections.
pub struct ReqJson<T>(pub T);

impl<T, S> axum::extract::FromRequest<S> for ReqJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = axum::body::Bytes::from_request(req, state)
            .await
            .map_err(|e| AppError(Error::validation("request.body", "errors:invalidJson").with_message(e.to_string())))?;
        serde_json::from_slice(&bytes)
            .map(ReqJson)
            .map_err(|e| AppError(Error::validation("json.invalid", "errors:invalidJson").with_message(e.to_string())))
    }
}

/// An optional JSON body: an empty body gives `T::default()`.
pub struct OptJson<T>(pub T);

impl<T, S> axum::extract::FromRequest<S> for OptJson<T>
where
    T: DeserializeOwned + Default,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = axum::body::Bytes::from_request(req, state)
            .await
            .map_err(|e| AppError(Error::validation("request.body", "errors:invalidJson").with_message(e.to_string())))?;
        if bytes.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(OptJson(T::default()));
        }
        serde_json::from_slice(&bytes)
            .map(OptJson)
            .map_err(|e| AppError(Error::validation("json.invalid", "errors:invalidJson").with_message(e.to_string())))
    }
}
