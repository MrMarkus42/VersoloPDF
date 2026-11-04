//! Digital IDs: a certificate with its private key, read from or written
//! to a password protected .pfx / .p12 file, or newly created.

use std::path::{Path, PathBuf};

use super::cert::{self, Cert, Key, NewId};
use crate::{Error, Result};

pub struct DigitalId {
    pub cert: Cert,
    /// Further certificates of the chain (issuers), DER.
    pub chain: Vec<Vec<u8>>,
    pub key: Key,
    pkcs8: Vec<u8>,
}

impl DigitalId {
    /// Reads a .pfx / .p12 file.
    pub fn from_pfx(data: &[u8], password: &str) -> Result<DigitalId> {
        use p12_keystore::{KeyStore, Pkcs12ImportPolicy};
        let store = KeyStore::from_pkcs12(data, password, Pkcs12ImportPolicy::Relaxed).map_err(|e| {
            let text = e.to_string().to_lowercase();
            if text.contains("mac") || text.contains("password") || text.contains("decrypt") {
                Error::Invalid("Das Kennwort der digitalen ID ist falsch.".into())
            } else {
                Error::Invalid(format!("Die Datei ist keine lesbare digitale ID (.pfx/.p12): {e}"))
            }
        })?;
        let (_, chain) = store
            .private_key_chain()
            .ok_or_else(|| Error::Invalid("Die Datei enthält keinen privaten Schlüssel.".into()))?;
        let pkcs8 = chain.key().as_der().to_vec();
        let key = Key::from_pkcs8(&pkcs8)?;
        let certs: Vec<Vec<u8>> = chain.certs().iter().map(|c| c.as_der().to_vec()).collect();
        // The signer's certificate is the one whose key matches; usually the first.
        let probe = cert::digest(cert::SHA256, &[b"mrpdf"])?;
        let (sig, _) = key.sign_sha256(&probe)?;
        let mut parsed: Vec<Cert> = certs.iter().filter_map(|d| cert::parse(d).ok()).collect();
        let pos = parsed
            .iter()
            .position(|c| cert::verify(c, cert::SHA256, &probe, &sig).unwrap_or(false))
            .ok_or_else(|| {
                Error::Invalid("Zertifikat und Schlüssel der digitalen ID passen nicht zusammen.".into())
            })?;
        let signer = parsed.remove(pos);
        let chain = parsed.into_iter().map(|c| c.der).collect();
        Ok(DigitalId { cert: signer, chain, key, pkcs8 })
    }

    /// Creates a new self-signed ID.
    pub fn create(id: &NewId<'_>) -> Result<DigitalId> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let (cert_der, pkcs8) = cert::create_self_signed(id, now)?;
        Ok(DigitalId {
            cert: cert::parse(&cert_der)?,
            chain: Vec::new(),
            key: Key::from_pkcs8(&pkcs8)?,
            pkcs8,
        })
    }

    /// As .pfx file protected by `password`.
    pub fn to_pfx(&self, password: &str) -> Result<Vec<u8>> {
        use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
        let bad = |e: p12_keystore::error::Error| Error::Invalid(format!("Digitale ID speichern: {e}"));
        let mut certs = vec![Certificate::from_der(&self.cert.der).map_err(bad)?];
        for c in &self.chain {
            certs.push(Certificate::from_der(c).map_err(bad)?);
        }
        let key = PrivateKey::from_der(&self.pkcs8).map_err(bad)?;
        let local_id = cert::digest(cert::SHA1, &[&self.cert.der])?;
        let mut store = KeyStore::new();
        store.add_entry(
            &self.cert.name,
            KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(local_id, key, certs)),
        );
        store.writer(password).write().map_err(bad)
    }
}

/// Folder of the digital IDs MrPDF created.
/// `MRPDF_IDS_DIR` replaces it (tests).
pub fn ids_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MRPDF_IDS_DIR") {
        return Some(PathBuf::from(dir));
    }
    crate::files::data_dir().map(|d| d.join("digitale-ids"))
}

/// The .pfx / .p12 files in `dir`, sorted by name.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("pfx") || x.eq_ignore_ascii_case("p12"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfx_round_trip_and_wrong_password() {
        let id =
            DigitalId::create(&NewId { name: "Erika Test", email: "", organisation: "", years: 2 }).unwrap();
        let pfx = id.to_pfx("geheim 123").unwrap();
        let back = DigitalId::from_pfx(&pfx, "geheim 123").unwrap();
        assert_eq!(back.cert.der, id.cert.der);
        assert_eq!(back.cert.name, "Erika Test");
        let err = DigitalId::from_pfx(&pfx, "falsch").err().unwrap();
        assert!(err.to_string().contains("Kennwort"), "{err}");
    }
}
