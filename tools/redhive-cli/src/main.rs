//! `redhive-cli` – packaging and publishing tool for Red Hive modules.
//!
//! ```text
//! redhive-cli pack     <module-dir> [--out artifacts]          build <id>-<version>.zip (manifest.json + views.json)
//! redhive-cli keygen   [--out signing-key.pem]                 create a P-256 signing key (PKCS#8 PEM)
//! redhive-cli sign     <package.zip> --key signing-key.pem     write <package>.sig (base64 DER over the SHA-256 hex digest)
//! redhive-cli verify   <package.zip> --pub public.pem          check a .sig file
//! redhive-cli login    --server URL --user U --password P      print a bearer token
//! redhive-cli upload   <package.zip> --server URL --token T    upload the package to module-delivery (server re-signs it)
//! redhive-cli publish  <module-dir> --server URL --token T [--immediate]
//!                                                              pack + upload + publish the manifest (starts a rollout)
//! ```
//! `REDHIVE_SERVER` and `REDHIVE_TOKEN` can replace `--server` / `--token`.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use p256::ecdsa::signature::{Signer as _, Verifier as _};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use redhive_contracts::modules::{check_manifest, ModuleManifest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

struct Args {
    positional: Vec<String>,
    flags: std::collections::HashMap<String, String>,
}

impl Args {
    fn parse() -> Args {
        let mut positional = Vec::new();
        let mut flags = std::collections::HashMap::new();
        let mut it = std::env::args().skip(1).peekable();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let value = match it.peek() {
                    Some(v) if !v.starts_with("--") => it.next().unwrap(),
                    _ => "true".to_string(),
                };
                flags.insert(name.to_string(), value);
            } else {
                positional.push(a);
            }
        }
        Args { positional, flags }
    }
    fn pos(&self, i: usize, what: &str) -> Result<&str> {
        self.positional.get(i).map(String::as_str).ok_or_else(|| anyhow!("missing argument: {what}"))
    }
    fn flag(&self, name: &str) -> Option<String> {
        self.flags.get(name).cloned()
    }
    fn need(&self, name: &str, env: &str) -> Result<String> {
        self.flag(name).or_else(|| std::env::var(env).ok()).ok_or_else(|| anyhow!("missing --{name} (or {env})"))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn read_module(dir: &Path) -> Result<(ModuleManifest, String, String)> {
    let manifest_text = std::fs::read_to_string(dir.join("manifest.json")).with_context(|| format!("{}/manifest.json", dir.display()))?;
    let manifest: ModuleManifest = serde_json::from_str(&manifest_text).context("manifest.json is not a valid module manifest")?;
    check_manifest(&manifest, "1.0.0").map_err(|e| anyhow!("manifest check failed: {} {:?}", e.code, e.field_errors))?;
    // module folder: client/views.json; a bare update folder (demo-updates/1.1.0): views.json next to the manifest
    let views_path = [dir.join("client").join(&manifest.client.view_pack), dir.join(&manifest.client.view_pack)].into_iter().find(|p| p.exists()).ok_or_else(|| anyhow!("{}: client/{} not found", dir.display(), manifest.client.view_pack))?;
    let views = std::fs::read_to_string(&views_path)?;
    serde_json::from_str::<redhive_contracts::viewpack::ViewPack>(&views).context("client/views.json is not a valid view pack")?;
    Ok((manifest, manifest_text, views))
}

fn pack(dir: &Path, out: &Path) -> Result<(ModuleManifest, PathBuf)> {
    let (manifest, manifest_text, views) = read_module(dir)?;
    std::fs::create_dir_all(out)?;
    let file = out.join(format!("{}-{}.zip", manifest.module_id, manifest.version));
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", opts)?;
        zip.write_all(manifest_text.as_bytes())?;
        zip.start_file(manifest.client.view_pack.clone(), opts)?;
        zip.write_all(views.as_bytes())?;
        zip.finish()?;
    }
    std::fs::write(&file, buf.into_inner())?;
    Ok((manifest, file))
}

fn client(token: Option<&str>) -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(t) = token {
        headers.insert(reqwest::header::AUTHORIZATION, format!("Bearer {t}").parse()?);
    }
    Ok(reqwest::Client::builder().default_headers(headers).build()?)
}

async fn check(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        bail!("{status}: {text}");
    }
    Ok(if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) })
}

async fn upload(server: &str, token: &str, module: &str, version: &str, file: &Path) -> Result<Value> {
    let bytes = std::fs::read(file)?;
    let url = format!("{}/api/packages/{module}/{version}", server.trim_end_matches('/'));
    check(client(Some(token))?.put(url).header("content-type", "application/zip").body(bytes).send().await?).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let a = Args::parse();
    let cmd = a.positional.first().cloned().unwrap_or_default();
    match cmd.as_str() {
        "pack" => {
            let out = PathBuf::from(a.flag("out").unwrap_or_else(|| "artifacts".into()));
            let (m, file) = pack(Path::new(a.pos(1, "module-dir")?), &out)?;
            let sha = sha256_hex(&std::fs::read(&file)?);
            println!("{} {} -> {} (sha256 {sha})", m.module_id, m.version, file.display());
        }
        "keygen" => {
            let out = PathBuf::from(a.flag("out").unwrap_or_else(|| "signing-key.pem".into()));
            let key = SigningKey::random(&mut rand::rngs::OsRng);
            std::fs::write(&out, key.to_pkcs8_pem(LineEnding::LF)?.as_bytes())?;
            let public = VerifyingKey::from(&key).to_public_key_pem(LineEnding::LF)?;
            let pub_file = out.with_extension("pub.pem");
            std::fs::write(&pub_file, &public)?;
            println!("private key: {}\npublic key:  {}\nSet REDHIVE__SIGNING_KEY on module-delivery to the private key to sign with it.", out.display(), pub_file.display());
        }
        "sign" => {
            let file = PathBuf::from(a.pos(1, "package.zip")?);
            let key = SigningKey::from_pkcs8_pem(&std::fs::read_to_string(a.need("key", "REDHIVE_SIGNING_KEY_FILE")?)?)?;
            let digest = sha256_hex(&std::fs::read(&file)?);
            let sig: Signature = key.sign(digest.as_bytes());
            let b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_der().as_bytes());
            let sig_file = PathBuf::from(format!("{}.sig", file.display()));
            std::fs::write(&sig_file, &b64)?;
            println!("sha256 {digest}\nsignature -> {}", sig_file.display());
        }
        "verify" => {
            let file = PathBuf::from(a.pos(1, "package.zip")?);
            let public = VerifyingKey::from_public_key_pem(&std::fs::read_to_string(a.need("pub", "REDHIVE_PUBLIC_KEY_FILE")?)?)?;
            let digest = sha256_hex(&std::fs::read(&file)?);
            let sig_text = std::fs::read_to_string(format!("{}.sig", file.display()))?;
            let sig = Signature::from_der(&base64::engine::general_purpose::STANDARD.decode(sig_text.trim())?)?;
            public.verify(digest.as_bytes(), &sig).map_err(|_| anyhow!("signature does NOT match"))?;
            println!("signature ok ({digest})");
        }
        "login" => {
            let server = a.need("server", "REDHIVE_SERVER")?;
            let body = json!({ "userName": a.need("user", "REDHIVE_USER")?, "password": a.need("password", "REDHIVE_PASSWORD")? });
            let r = check(client(None)?.post(format!("{}/api/identity/login", server.trim_end_matches('/'))).json(&body).send().await?).await?;
            println!("{}", r.get("accessToken").and_then(Value::as_str).unwrap_or_default());
        }
        "upload" => {
            let file = PathBuf::from(a.pos(1, "package.zip")?);
            let name = file.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            let (module, version) = name.rsplit_once('-').ok_or_else(|| anyhow!("package name must be <module>-<version>.zip"))?;
            let r = upload(&a.need("server", "REDHIVE_SERVER")?, &a.need("token", "REDHIVE_TOKEN")?, module, version, &file).await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        "publish" => {
            let server = a.need("server", "REDHIVE_SERVER")?;
            let token = a.need("token", "REDHIVE_TOKEN")?;
            let out = PathBuf::from(a.flag("out").unwrap_or_else(|| "artifacts".into()));
            let (m, file) = pack(Path::new(a.pos(1, "module-dir")?), &out)?;
            upload(&server, &token, &m.module_id, &m.version, &file).await?;
            println!("uploaded {}", file.display());
            let body = json!({ "manifest": m, "channel": a.flag("channel").unwrap_or_else(|| "stable".into()), "immediate": a.flag("immediate").is_some() });
            let r = check(client(Some(&token))?.post(format!("{}/api/modules", server.trim_end_matches('/'))).json(&body).send().await?).await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        _ => {
            eprintln!("usage: redhive-cli <pack|keygen|sign|verify|login|upload|publish> …  (see the source header for options)");
            std::process::exit(2);
        }
    }
    Ok(())
}
