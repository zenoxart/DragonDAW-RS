//! Red Hive module delivery service: signed client packages.
//!
//! A package is a ZIP with `manifest.json` and the view pack (`views.json`). On upload the service checks
//! it, computes the SHA-256 digest and signs the digest with its ECDSA P-256 key. Clients pin the public
//! key on first contact and refuse packages whose hash or signature does not match.

mod signing;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, InternalCall, RequestContext, ToHttp};
use redhive_contracts::modules::{ModuleManifest, PackageInfo, SigningKeyInfo};
use redhive_contracts::viewpack::ViewPack;
use redhive_monads::{Error, OptionExt, Res};
use sha2::{Digest, Sha256};
use signing::Signer;
use sqlx::Row;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

const SERVICE: &str = "module-delivery";
const MAX_PACKAGE: usize = 20 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    core: Core,
    signer: Arc<Signer>,
    audit: AuditLog,
}

impl axum::extract::FromRef<AppState> for Core {
    fn from_ref(s: &AppState) -> Core {
        s.core.clone()
    }
}

fn info_from_row(r: &sqlx::postgres::PgRow) -> PackageInfo {
    let module_id: String = r.get("module_id");
    let version: String = r.get("version");
    PackageInfo {
        download_url: format!("/api/packages/{module_id}/{version}/download"),
        module_id,
        version,
        file_name: r.get("file_name"),
        sha256: r.get("sha256"),
        size_bytes: r.get("size_bytes"),
        signature: r.get("signature"),
        uploaded_at: r.get::<DateTime<Utc>, _>("uploaded_at"),
    }
}

/// Reads and checks a package: manifest present and matching, view pack parses and belongs to the module.
pub fn inspect_package(bytes: &[u8], module_id: &str, version: &str) -> Res<(ModuleManifest, ViewPack)> {
    let invalid = |m: String| Error::validation("delivery.package.invalid", "delivery:package.invalid").with_message(m);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| invalid(format!("not a ZIP file: {e}")))?;
    let mut read = |name: &str| -> Res<String> {
        let mut f = zip.by_name(name).map_err(|_| invalid(format!("{name} is missing")))?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(|e| invalid(format!("{name}: {e}")))?;
        Ok(s)
    };
    let manifest: ModuleManifest = serde_json::from_str(&read("manifest.json")?).map_err(|e| invalid(format!("manifest.json: {e}")))?;
    if manifest.module_id != module_id || manifest.version != version {
        return Err(invalid(format!("package contains {} {}, expected {module_id} {version}", manifest.module_id, manifest.version)));
    }
    let pack: ViewPack = serde_json::from_str(&read(&manifest.client.view_pack)?).map_err(|e| invalid(format!("{}: {e}", manifest.client.view_pack)))?;
    if pack.module_id != module_id {
        return Err(invalid(format!("view pack belongs to '{}'", pack.module_id)));
    }
    let missing = pack.missing_texts(&manifest.prefix);
    if !missing.is_empty() {
        return Err(invalid(format!("missing texts: {}", missing.iter().map(|(l, k)| format!("{l}:{k}")).collect::<Vec<_>>().join(", "))));
    }
    Ok((manifest, pack))
}

/// Builds a package ZIP from a manifest and a view pack (used for development modules and by the CLI).
pub fn build_package(manifest_json: &str, views_json: &str, view_pack_name: &str) -> Res<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let fail = |e: String| Error::unexpected("delivery.package.build", "errors:unexpected").with_message(e);
        zip.start_file("manifest.json", opts).map_err(|e| fail(e.to_string()))?;
        zip.write_all(manifest_json.as_bytes()).map_err(|e| fail(e.to_string()))?;
        zip.start_file(view_pack_name, opts).map_err(|e| fail(e.to_string()))?;
        zip.write_all(views_json.as_bytes()).map_err(|e| fail(e.to_string()))?;
        zip.finish().map_err(|e| fail(e.to_string()))?;
    }
    Ok(buf.into_inner())
}

async fn store(s: &AppState, module_id: &str, version: &str, bytes: &[u8]) -> Res<PackageInfo> {
    let digest = hex(&Sha256::digest(bytes));
    let signature = s.signer.sign(&digest);
    let file_name = format!("{module_id}-{version}.zip");
    let row = sqlx::query(
        "insert into delivery.packages (module_id, version, file_name, sha256, size_bytes, signature, content) values ($1, $2, $3, $4, $5, $6, $7)
         on conflict (module_id, version) do update set file_name = excluded.file_name, sha256 = excluded.sha256, size_bytes = excluded.size_bytes,
             signature = excluded.signature, content = excluded.content, uploaded_at = now()
         returning module_id, version, file_name, sha256, size_bytes, signature, uploaded_at",
    )
    .bind(module_id)
    .bind(version)
    .bind(&file_name)
    .bind(&digest)
    .bind(bytes.len() as i64)
    .bind(&signature)
    .bind(bytes)
    .fetch_one(s.core.db.pool())
    .await
    .db_err("delivery.store")?;
    Ok(info_from_row(&row))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Upload: authorize → size → inspect → hash + sign → store. A published version's package cannot be replaced.
async fn upload(State(s): State<AppState>, ctx: RequestContext, Path((module_id, version)): Path<(String, String)>, body: Bytes) -> ApiResult<PackageInfo> {
    async {
        ctx.user.require("modules.publish")?;
        if body.len() > MAX_PACKAGE {
            return Err(Error::validation("delivery.package.tooLarge", "delivery:package.tooLarge").with_metadata("maxMb", 20));
        }
        inspect_package(&body, &module_id, &version)?;
        let published: Option<String> = sqlx::query_scalar("select version from registry.modules where module_id = $1 and version = $2")
            .bind(&module_id)
            .bind(&version)
            .fetch_optional(s.core.db.pool())
            .await
            .db_err("delivery.published")?;
        let existing: Option<String> = sqlx::query_scalar("select sha256 from delivery.packages where module_id = $1 and version = $2")
            .bind(&module_id)
            .bind(&version)
            .fetch_optional(s.core.db.pool())
            .await
            .db_err("delivery.existing")?;
        if published.is_some() && existing.is_some_and(|h| h != hex(&Sha256::digest(&body))) {
            return Err(Error::conflict("delivery.package.immutable", "delivery:package.immutable"));
        }
        let info = store(&s, &module_id, &version, &body).await?;
        let mut tx = s.core.db.begin().await?;
        s.audit.write(&mut tx, &ctx.user, "package.uploaded", "package", &format!("{module_id}@{version}"), serde_json::json!({ "sha256": info.sha256, "size": info.size_bytes })).await?;
        tx.commit().await.db_err("delivery.audit")?;
        Ok(info)
    }
    .await
    .http()
}

async fn load_info(core: &Core, module_id: &str, version: &str) -> Res<PackageInfo> {
    let row = sqlx::query("select module_id, version, file_name, sha256, size_bytes, signature, uploaded_at from delivery.packages where module_id = $1 and version = $2")
        .bind(module_id)
        .bind(version)
        .fetch_optional(core.db.pool())
        .await
        .db_err("delivery.info")?;
    row.as_ref().map(info_from_row).ok_or_not_found("delivery.package", "delivery:package.notFound")
}

async fn info(State(s): State<AppState>, ctx: RequestContext, Path((module_id, version)): Path<(String, String)>) -> ApiResult<PackageInfo> {
    let _ = ctx;
    load_info(&s.core, &module_id, &version).await.http()
}

async fn download(State(s): State<AppState>, _ctx: RequestContext, Path((module_id, version)): Path<(String, String)>) -> Result<Response, redhive_backend_core::AppError> {
    let row = sqlx::query("select file_name, sha256, content from delivery.packages where module_id = $1 and version = $2")
        .bind(&module_id)
        .bind(&version)
        .fetch_optional(s.core.db.pool())
        .await
        .db_err("delivery.download")?
        .ok_or_not_found("delivery.package", "delivery:package.notFound")?;
    let file: String = row.get("file_name");
    let sha: String = row.get("sha256");
    let content: Vec<u8> = row.get("content");
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file}\"")),
            (header::HeaderName::from_static("x-redhive-sha256"), sha),
        ],
        content,
    )
        .into_response())
}

async fn signing_key(State(s): State<AppState>) -> Json<SigningKeyInfo> {
    Json(SigningKeyInfo { public_key_pem: s.signer.public_pem.clone(), algorithm: "ECDSA-P256-SHA256".into() })
}

async fn internal_info(State(s): State<AppState>, _call: InternalCall, Path((module_id, version)): Path<(String, String)>) -> ApiResult<PackageInfo> {
    load_info(&s.core, &module_id, &version).await.http()
}

/// Packages the development modules (`<modules dir>/<module>/manifest.json` + `client/views.json`) that
/// have no package yet, so a fresh installation is usable immediately.
async fn seed_development_modules(s: &AppState) {
    let dir = s.core.options.var("MODULES_DIR").unwrap_or_else(|| "modules".into());
    let Ok(entries) = std::fs::read_dir(&dir) else {
        tracing::info!(dir, "no development modules directory");
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let (Ok(manifest_json), Ok(views_json)) = (std::fs::read_to_string(path.join("manifest.json")), std::fs::read_to_string(path.join("client").join("views.json"))) else { continue };
        let Ok(manifest) = serde_json::from_str::<ModuleManifest>(&manifest_json) else {
            tracing::warn!(path = %path.display(), "manifest.json does not parse");
            continue;
        };
        if load_info(&s.core, &manifest.module_id, &manifest.version).await.is_ok() {
            continue;
        }
        let result = build_package(&manifest_json, &views_json, &manifest.client.view_pack)
            .and_then(|bytes| inspect_package(&bytes, &manifest.module_id, &manifest.version).map(|_| bytes));
        match result {
            Ok(bytes) => match store(s, &manifest.module_id, &manifest.version, &bytes).await {
                Ok(i) => tracing::info!(module = %i.module_id, version = %i.version, sha256 = %i.sha256, "development module packaged"),
                Err(e) => tracing::warn!(error = %e, "development module could not be stored"),
            },
            Err(e) => tracing::warn!(module = %manifest.module_id, error = %e, "development module is invalid"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let signer = Arc::new(Signer::load_or_create(&core).await?);
    let state = AppState { core: core.clone(), signer, audit: AuditLog::new(SERVICE) };
    seed_development_modules(&state).await;
    let router = Router::new()
        .route("/api/packages/signing-key", get(signing_key))
        .route("/api/packages/{module}/{version}", get(info).put(upload).post(upload))
        .route("/api/packages/{module}/{version}/download", get(download))
        .route("/internal/packages/{module}/{version}", get(internal_info));
    serve(core, router, state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_round_trip() {
        let manifest = r#"{"moduleId":"demo","version":"1.0.0","displayName":"demo:module.name","prefix":"demo","client":{}}"#;
        let views = r#"{"moduleId":"demo","version":"1.0.0","texts":{"en":{"module.name":"Demo"}}}"#;
        let zip = build_package(manifest, views, "views.json").unwrap();
        let (m, p) = inspect_package(&zip, "demo", "1.0.0").unwrap();
        assert_eq!(m.module_id, "demo");
        assert_eq!(p.texts["en"]["module.name"], "Demo");
        assert!(inspect_package(&zip, "demo", "2.0.0").is_err());
        assert!(inspect_package(b"not a zip", "demo", "1.0.0").is_err());
    }
}
