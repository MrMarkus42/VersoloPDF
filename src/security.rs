//! Password protection (PDF standard security handler, AES-256).

use std::collections::BTreeMap;
use std::sync::Arc;

use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{Document, EncryptionState, EncryptionVersion, LoadOptions, Permissions};

use crate::error::{Error, Result};
use crate::pdf_edit::guarded;

/// What a reader who only knows the user password may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectOptions {
    /// Required to open the document (may be empty: anyone can open it).
    pub user_password: String,
    /// Required to change permissions. Generated randomly if empty.
    pub owner_password: String,
    pub allow_print: bool,
    pub allow_copy: bool,
    pub allow_modify: bool,
    pub allow_annotate: bool,
    pub allow_fill_forms: bool,
    pub allow_assemble: bool,
}

impl Default for ProtectOptions {
    fn default() -> Self {
        Self {
            user_password: String::new(),
            owner_password: String::new(),
            allow_print: true,
            allow_copy: true,
            allow_modify: true,
            allow_annotate: true,
            allow_fill_forms: true,
            allow_assemble: true,
        }
    }
}

impl ProtectOptions {
    fn permissions(&self) -> Permissions {
        let mut p = Permissions::COPYABLE_FOR_ACCESSIBILITY;
        if self.allow_print {
            p |= Permissions::PRINTABLE | Permissions::PRINTABLE_IN_HIGH_QUALITY;
        }
        if self.allow_copy {
            p |= Permissions::COPYABLE;
        }
        if self.allow_modify {
            p |= Permissions::MODIFIABLE;
        }
        if self.allow_annotate {
            p |= Permissions::ANNOTABLE;
        }
        if self.allow_fill_forms {
            p |= Permissions::FILLABLE;
        }
        if self.allow_assemble {
            p |= Permissions::ASSEMBLABLE;
        }
        p
    }
}

/// Facts about the standard security handler of an encrypted document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityInfo {
    pub revision: i64,
    /// The `/P` permission bits.
    pub bits: u32,
    /// Whether the password used is the owner password (`None`: unknown,
    /// only determined for revisions 5 and 6).
    pub owner: Option<bool>,
}

impl SecurityInfo {
    fn bit(&self, n: u32) -> bool {
        self.bits & (1 << (n - 1)) != 0
    }

    pub fn can_print(&self) -> bool {
        self.bit(3)
    }

    pub fn can_modify(&self) -> bool {
        self.bit(4)
    }

    pub fn can_copy(&self) -> bool {
        self.bit(5)
    }

    pub fn can_annotate(&self) -> bool {
        self.bit(6)
    }

    pub fn can_fill_forms(&self) -> bool {
        self.bit(6) || (self.revision >= 3 && self.bit(9))
    }

    pub fn can_assemble(&self) -> bool {
        if self.revision >= 3 { self.bit(11) } else { self.bit(4) }
    }
}

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

/// Hash algorithm 2.B of ISO 32000-2 (revision 6) and its revision 5 variant.
fn hash_2b(password: &[u8], salt: &[u8], user_key: Option<&[u8]>, revision: i64) -> Vec<u8> {
    use aes::cipher::{BlockModeEncrypt as _, KeyIvInit as _};
    use sha2::{Digest as _, Sha256, Sha384, Sha512};

    let mut hasher = Sha256::new();
    hasher.update(password);
    hasher.update(salt);
    if let Some(u) = user_key {
        hasher.update(u);
    }
    let mut k = hasher.finalize().to_vec();
    if revision == 5 {
        return k;
    }
    let mut round: u32 = 0;
    loop {
        round += 1;
        let mut k1 = Vec::with_capacity(64 * (password.len() + k.len() + user_key.map_or(0, <[u8]>::len)));
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            if let Some(u) = user_key {
                k1.extend_from_slice(u);
            }
        }
        let key: [u8; 16] = k[..16].try_into().expect("hash key is at least 32 bytes");
        let iv: [u8; 16] = k[16..32].try_into().expect("hash key is at least 32 bytes");
        let mut encryptor = Aes128CbcEnc::new(&key.into(), &iv.into());
        for block in k1.as_chunks_mut::<16>().0 {
            let block: &mut aes::cipher::Block<aes::Aes128> =
                (&mut block[..]).try_into().expect("AES block is 16 bytes");
            encryptor.encrypt_block(block);
        }
        let e = k1;
        k = match e[..16].iter().map(|v| u32::from(*v)).sum::<u32>() % 3 {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        if round >= 64 && u32::from(e.last().copied().unwrap_or(0)) <= round - 32 {
            break;
        }
    }
    k.truncate(32);
    k
}

/// Reads the security handler of an encrypted document. Returns `None` for
/// unencrypted documents or if the structure cannot be read.
pub fn security_info(bytes: &[u8], password: Option<&str>) -> Option<SecurityInfo> {
    guarded(|| {
        let mut options = match password {
            Some(pw) => LoadOptions::with_password(pw),
            None => LoadOptions::default(),
        };
        options.max_decompressed_size = Some(MAX_DECOMPRESSED_STREAM);
        let doc = Document::load_mem_with_options(bytes, options)?;
        let state = doc.encryption_state.as_ref().ok_or(Error::InvalidFormat)?;
        let revision = state.revision();
        let owner = if revision >= 5 {
            let o = state.owner_value();
            let u = state.user_value();
            if o.len() >= 40 && u.len() >= 48 {
                let pw = password.unwrap_or("");
                let prepared =
                    stringprep::saslprep(pw).map(|p| p.into_owned()).unwrap_or_else(|_| pw.to_string());
                let bytes = prepared.as_bytes();
                let bytes = &bytes[..bytes.len().min(127)];
                Some(hash_2b(bytes, &o[32..40], Some(&u[..48]), revision) == o[..32])
            } else {
                None
            }
        } else {
            None
        };
        Ok(SecurityInfo { revision, bits: state.permissions().bits() as u32, owner })
    })
    .ok()
}

/// Upper limit for a single decompressed stream (see `pdf_edit`).
const MAX_DECOMPRESSED_STREAM: usize = 256 * 1024 * 1024;

/// Loads a document and removes any existing encryption in memory.
fn load_decrypted(bytes: &[u8], password: Option<&str>) -> Result<Document> {
    let mut options = match password {
        Some(pw) => LoadOptions::with_password(pw),
        None => LoadOptions::default(),
    };
    options.max_decompressed_size = Some(MAX_DECOMPRESSED_STREAM);
    let mut doc = Document::load_mem_with_options(bytes, options)?;
    if doc.encryption_state.is_some() {
        doc.encryption_state = None;
        doc.trailer.remove(b"Encrypt");
    } else if doc.is_encrypted() {
        return Err(Error::WrongPassword);
    }
    Ok(doc)
}

/// A random password of 32 hex digits.
pub fn random_password() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| Error::Invalid(format!("random generator: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Encrypts the document with AES-256. Returns the new bytes and the owner
/// password that was used (generated if none was given).
pub fn protect(
    bytes: &[u8],
    current_password: Option<&str>,
    opts: &ProtectOptions,
) -> Result<(Vec<u8>, String)> {
    guarded(|| {
        if opts.user_password.chars().count() > 127 || opts.owner_password.chars().count() > 127 {
            return Err(Error::Invalid("password too long".into()));
        }
        let owner =
            if opts.owner_password.is_empty() { random_password()? } else { opts.owner_password.clone() };
        let mut doc = load_decrypted(bytes, current_password)?;
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|e| Error::Invalid(format!("random generator: {e}")))?;
        let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
        let version = EncryptionVersion::V5 {
            encrypt_metadata: true,
            crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
            file_encryption_key: &key,
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: &owner,
            user_password: &opts.user_password,
            permissions: opts.permissions(),
        };
        let state = EncryptionState::try_from(version).map_err(|e| Error::Structure(format!("{e:?}")))?;
        doc.encrypt(&state)?;
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok((out, owner))
    })
}

/// Removes the password protection.
pub fn remove_protection(bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>> {
    guarded(|| {
        let mut doc = load_decrypted(bytes, password)?;
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| Error::Structure(e.to_string()))?;
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_passwords_differ() {
        let a = random_password().unwrap();
        let b = random_password().unwrap();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn permission_bits() {
        let none = ProtectOptions {
            allow_print: false,
            allow_copy: false,
            allow_modify: false,
            allow_annotate: false,
            allow_fill_forms: false,
            allow_assemble: false,
            ..ProtectOptions::default()
        };
        assert_eq!(none.permissions(), Permissions::COPYABLE_FOR_ACCESSIBILITY);
        assert!(ProtectOptions::default().permissions().contains(Permissions::ANNOTABLE));
    }
}
