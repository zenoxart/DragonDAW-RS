//! The module loader.
//!
//! 1. Ask the registry which modules (and versions) the signed-in user gets.
//! 2. Download missing packages (cached per version), verify the SHA-256 digest and the ECDSA signature
//!    against the server's signing key, which is pinned on first contact.
//! 3. Check the manifest (host version, contracts) and resolve dependencies (load order, rejections).
//! 4. Register the view pack: texts, icons, menu groups, views, commands. Everything is recorded per module so
//!    unloading (or hot-swapping) removes it in one call.
//!
//! Development modules: when `REDHIVE_DEV_MODULES` points at a modules directory, `<id>/client/views.json`
//! from there is used for modules without a package.

use crate::api::ApiClient;
use crate::commands::CommandHandler;
use crate::context::{AppContext, Paths};
use base64::Engine;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use redhive_contracts::commands::{placements, CommandDescriptor, CommandPlacement};
use redhive_contracts::modules::{check_manifest, resolve_load_order, AssignedModule, ModuleManifest, SigningKeyInfo};
use redhive_contracts::viewpack::{CommandRun, ViewBody, ViewPack};
use redhive_contracts::{IconRef, TextKey};
use redhive_monads::{Error, Res};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub manifest: ModuleManifest,
    pub pack: ViewPack,
    pub sha256: Option<String>,
    pub signature_verified: bool,
    pub development: bool,
    pub newer_version: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LoadResult {
    pub modules: Vec<LoadedModule>,
    pub rejected: Vec<(String, Error)>,
    /// The server's key differs from the pinned one.
    pub key_changed: Option<String>,
    pub server_key: Option<String>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Verifies digest and signature of a package.
pub fn verify_package(bytes: &[u8], sha256: &str, signature_b64: &str, public_pem: &str) -> Res<()> {
    let digest = hex(&Sha256::digest(bytes));
    if !digest.eq_ignore_ascii_case(sha256) {
        return Err(Error::conflict("modules.hash.invalid", "modules:hash.invalid"));
    }
    let key = VerifyingKey::from_public_key_pem(public_pem).map_err(|_| Error::conflict("modules.signature.invalid", "modules:signature.invalid"))?;
    let der = base64::engine::general_purpose::STANDARD.decode(signature_b64).map_err(|_| Error::conflict("modules.signature.invalid", "modules:signature.invalid"))?;
    let sig = Signature::from_der(&der).map_err(|_| Error::conflict("modules.signature.invalid", "modules:signature.invalid"))?;
    key.verify(digest.as_bytes(), &sig).map_err(|_| Error::conflict("modules.signature.invalid", "modules:signature.invalid"))
}

/// Reads manifest and view pack from a package.
pub fn read_package(bytes: &[u8]) -> Res<(ModuleManifest, ViewPack)> {
    let bad = |m: String| Error::validation("modules.package.invalid", "delivery:package.invalid").with_message(m);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| bad(e.to_string()))?;
    let mut read = |name: &str| -> Res<String> {
        let mut f = zip.by_name(name).map_err(|_| bad(format!("{name} missing")))?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(|e| bad(e.to_string()))?;
        Ok(s)
    };
    let manifest: ModuleManifest = serde_json::from_str(&read("manifest.json")?).map_err(|e| bad(e.to_string()))?;
    let pack: ViewPack = serde_json::from_str(&read(&manifest.client.view_pack)?).map_err(|e| bad(e.to_string()))?;
    Ok((manifest, pack))
}

fn dev_pack(module_id: &str) -> Option<ViewPack> {
    let dir = std::env::var("REDHIVE_DEV_MODULES").ok()?;
    let text = std::fs::read_to_string(std::path::Path::new(&dir).join(module_id).join("client").join("views.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Downloads, verifies and orders the user's modules. `pinned`: the pinned signing key of this server.
pub async fn load_assigned(api: &ApiClient, paths: &Paths, pinned: Option<String>, only: Option<&str>) -> Res<LoadResult> {
    let assigned: Vec<AssignedModule> = api.get("/api/modules/assigned").await?;
    let key: Option<SigningKeyInfo> = api.get("/api/packages/signing-key").await.ok();
    let mut result = LoadResult { server_key: key.as_ref().map(|k| k.public_key_pem.clone()), ..Default::default() };
    let trusted = match (&pinned, &result.server_key) {
        (Some(p), Some(s)) if p.trim() != s.trim() => {
            result.key_changed = Some(s.clone());
            None
        }
        (Some(p), _) => Some(p.clone()),
        (None, s) => s.clone(),
    };
    let mut candidates = Vec::new();
    for a in assigned {
        if only.is_some_and(|o| o != a.manifest.module_id) {
            continue;
        }
        let id = a.manifest.module_id.clone();
        if let Err(e) = check_manifest(&a.manifest, crate::HOST_VERSION) {
            result.rejected.push((id, e));
            continue;
        }
        let loaded: Res<LoadedModule> = async {
            match &a.package {
                Some(p) => {
                    let key = trusted.clone().ok_or_else(|| Error::conflict("modules.signingKey.changed", "modules:signingKey.changed"))?;
                    let dir = paths.modules.join(&id);
                    let _ = std::fs::create_dir_all(&dir);
                    let file = dir.join(format!("{}.zip", p.version));
                    let bytes = match std::fs::read(&file) {
                        Ok(b) if hex(&Sha256::digest(&b)).eq_ignore_ascii_case(&p.sha256) => b,
                        _ => {
                            let b = api.bytes(&p.download_url).await?;
                            verify_package(&b, &p.sha256, &p.signature, &key)?;
                            let _ = std::fs::write(&file, &b);
                            b
                        }
                    };
                    verify_package(&bytes, &p.sha256, &p.signature, &key)?;
                    let (manifest, pack) = read_package(&bytes)?;
                    if manifest.module_id != id || manifest.version != a.manifest.version {
                        return Err(Error::conflict("modules.package.mismatch", "delivery:package.invalid"));
                    }
                    Ok(LoadedModule { manifest, pack, sha256: Some(p.sha256.clone()), signature_verified: true, development: false, newer_version: a.newer_version.clone() })
                }
                None => match dev_pack(&id) {
                    Some(pack) => Ok(LoadedModule { manifest: a.manifest.clone(), pack, sha256: None, signature_verified: false, development: true, newer_version: a.newer_version.clone() }),
                    None => Err(Error::not_found("modules.package.missing", "modules:package.missing")),
                },
            }
        }
        .await;
        match loaded {
            Ok(m) => candidates.push(m),
            Err(e) => result.rejected.push((id, e)),
        }
    }
    if only.is_some() {
        result.modules = candidates;
        return Ok(result);
    }
    let manifests: Vec<ModuleManifest> = candidates.iter().map(|m| m.manifest.clone()).collect();
    let (order, rejected) = resolve_load_order(&manifests);
    result.rejected.extend(rejected);
    let mut by_id: BTreeMap<String, LoadedModule> = candidates.into_iter().map(|m| (m.manifest.module_id.clone(), m)).collect();
    result.modules = order.iter().filter_map(|m| by_id.remove(&m.module_id)).collect();
    Ok(result)
}

/// Registers a module's view pack with the context.
pub fn register(ctx: &mut AppContext, m: &LoadedModule) {
    let id = &m.manifest.module_id;
    let prefix = &m.manifest.prefix;
    for (lang, texts) in &m.pack.texts {
        ctx.lang.register_module(id, prefix, lang, texts);
    }
    ctx.icons.register_module(id, prefix, &m.pack.icons);
    for g in &m.pack.groups {
        ctx.commands.register_group(id, g.clone());
    }
    for v in &m.pack.views {
        ctx.navigation.register_module_view(id, v);
        // Every view is reachable as a navigation command (navigation tree, wheel, palette, deep links).
        let mut placements = vec![CommandPlacement::new(placements::NAVIGATION, prefix, 0, v.order), CommandPlacement::new(placements::MAIN_MENU, prefix, 0, v.order)];
        if v.home {
            placements.push(CommandPlacement::new(placements::WHEEL, "", 0, 0));
        }
        ctx.commands.register(
            id,
            CommandDescriptor {
                id: format!("navigate.{}", v.key),
                label: TextKey::new(&v.title),
                icon: IconRef::new(&v.icon),
                permission: v.permission.clone(),
                placements,
                shortcuts: vec![],
                workflow_capable: false,
                ai_allowed: false,
                module_id: id.clone(),
            },
            CommandHandler::Navigate(v.key.clone()),
        );
        // Actions with a command id become commands too (palette, shortcuts, recorder).
        if let ViewBody::List(l) = &v.body {
            for a in l.toolbar_actions.iter().chain(l.row_actions.iter()).filter(|a| a.command.is_some()) {
                let surface = &m.manifest.contract_surface;
                let cid = a.command.clone().unwrap_or_default();
                let contract = surface.commands.iter().find(|c| c.id == cid);
                let row = l.row_actions.iter().any(|r| r.id == a.id);
                ctx.commands.register(
                    id,
                    CommandDescriptor {
                        id: cid.clone(),
                        label: TextKey::new(&a.label),
                        icon: IconRef::new(&a.icon),
                        permission: a.permission.clone(),
                        placements: if row { vec![] } else { vec![CommandPlacement::new(placements::MAIN_MENU, prefix, 1, 10)] },
                        shortcuts: vec![],
                        workflow_capable: contract.is_some_and(|c| c.workflow_capable),
                        ai_allowed: contract.is_some_and(|c| c.ai_allowed),
                        module_id: id.clone(),
                    },
                    CommandHandler::Action { module_id: id.clone(), view: Some(v.key.clone()), action: a.clone() },
                );
            }
        }
    }
    for c in &m.pack.commands {
        let handler = match &c.run {
            CommandRun::Navigate { view } => CommandHandler::Navigate(view.clone()),
            CommandRun::Action { action } => CommandHandler::Action { module_id: id.clone(), view: None, action: action.clone() },
        };
        ctx.commands.register(id, c.descriptor.clone(), handler);
    }
    ctx.log.info("modules", format!("{} {} registered{}", id, m.manifest.version, if m.development { " (development)" } else { "" }));
}

/// Removes everything a module registered.
pub fn unregister(ctx: &mut AppContext, module_id: &str) {
    let n = ctx.commands.unregister_owner(module_id);
    ctx.navigation.unregister_owner(module_id);
    ctx.lang.unregister_module(module_id);
    ctx.icons.unregister_module(module_id);
    ctx.log.info("modules", format!("{module_id} unregistered ({n} commands)"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::SigningKey;
    use p256::pkcs8::EncodePublicKey;

    #[test]
    fn verifies_hash_and_signature() {
        let key = SigningKey::random(&mut rand::rngs::OsRng);
        let pem = VerifyingKey::from(&key).to_public_key_pem(p256::pkcs8::LineEnding::LF).unwrap();
        let bytes = b"package bytes";
        let digest = hex(&Sha256::digest(bytes));
        let sig: Signature = key.sign(digest.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_der().as_bytes());
        verify_package(bytes, &digest, &b64, &pem).unwrap();
        assert_eq!(verify_package(b"tampered", &digest, &b64, &pem).unwrap_err().code, "modules.hash.invalid");
        let other = SigningKey::random(&mut rand::rngs::OsRng);
        let other_pem = VerifyingKey::from(&other).to_public_key_pem(p256::pkcs8::LineEnding::LF).unwrap();
        assert_eq!(verify_package(bytes, &digest, &b64, &other_pem).unwrap_err().code, "modules.signature.invalid");
    }

}
