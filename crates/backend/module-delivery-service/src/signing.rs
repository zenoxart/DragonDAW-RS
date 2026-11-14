//! ECDSA P-256 package signing.

use base64::Engine;
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use redhive_backend_core::hosting::Core;
use sqlx::Row;

pub struct Signer {
    key: SigningKey,
    pub public_pem: String,
}

impl Signer {
    /// Uses `REDHIVE__SIGNING_KEY` (PKCS#8 PEM) when set, otherwise the key stored in the database,
    /// otherwise generates and stores a new one.
    pub async fn load_or_create(core: &Core) -> Result<Signer, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(pem) = core.options.var("SIGNING_KEY") {
            let key = SigningKey::from_pkcs8_pem(&pem.replace("\\n", "\n"))?;
            return Ok(Signer::from_key(key));
        }
        if let Some(row) = sqlx::query("select private_pem from delivery.signing_keys where id = 'default'").fetch_optional(core.db.pool()).await? {
            let pem: String = row.get("private_pem");
            return Ok(Signer::from_key(SigningKey::from_pkcs8_pem(&pem)?));
        }
        let key = SigningKey::random(&mut rand::rngs::OsRng);
        let signer = Signer::from_key(key);
        let private = signer.key.to_pkcs8_pem(LineEnding::LF)?;
        sqlx::query("insert into delivery.signing_keys (id, private_pem, public_pem) values ('default', $1, $2) on conflict (id) do nothing")
            .bind(private.as_str())
            .bind(&signer.public_pem)
            .execute(core.db.pool())
            .await?;
        // Another instance may have won the race: reload.
        let pem: String = sqlx::query_scalar("select private_pem from delivery.signing_keys where id = 'default'").fetch_one(core.db.pool()).await?;
        tracing::info!("package signing key created");
        Ok(Signer::from_key(SigningKey::from_pkcs8_pem(&pem)?))
    }

    pub fn from_key(key: SigningKey) -> Signer {
        let public_pem = VerifyingKey::from(&key).to_public_key_pem(LineEnding::LF).unwrap_or_default();
        Signer { key, public_pem }
    }

    /// Base64 DER signature over the UTF-8 bytes of the hex digest.
    pub fn sign(&self, digest_hex: &str) -> String {
        let sig: Signature = self.key.sign(digest_hex.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(sig.to_der().as_bytes())
    }
}

/// Verification as done by the client (kept here for tests).
#[cfg(test)]
pub fn verify(public_pem: &str, digest_hex: &str, signature_b64: &str) -> bool {
    use p256::ecdsa::signature::Verifier as _;
    use p256::pkcs8::DecodePublicKey;
    let Ok(key) = VerifyingKey::from_public_key_pem(public_pem) else { return false };
    let Ok(der) = base64::engine::general_purpose::STANDARD.decode(signature_b64) else { return false };
    let Ok(sig) = Signature::from_der(&der) else { return false };
    key.verify(digest_hex.as_bytes(), &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let s = Signer::from_key(SigningKey::random(&mut rand::rngs::OsRng));
        let sig = s.sign("abc123");
        assert!(verify(&s.public_pem, "abc123", &sig));
        assert!(!verify(&s.public_pem, "abc124", &sig));
    }
}
