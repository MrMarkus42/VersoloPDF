//! X.509 certificates (reading the parts a signature needs, creating a
//! self-signed one) and the keys that sign (RSA, ECDSA P-256 / P-384).

use super::der::{self, Tlv};
use crate::{Error, Result};

fn bad(what: &str) -> Error {
    Error::Invalid(format!("Zertifikat: {what}"))
}

pub const SHA1: &str = "1.3.14.3.2.26";
pub const SHA256: &str = "2.16.840.1.101.3.4.2.1";
pub const SHA384: &str = "2.16.840.1.101.3.4.2.2";
pub const SHA512: &str = "2.16.840.1.101.3.4.2.3";
pub const RSA: &str = "1.2.840.113549.1.1.1";
pub const EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
pub const P256: &str = "1.2.840.10045.3.1.7";
pub const P384: &str = "1.3.132.0.34";
pub const ECDSA_SHA256: &str = "1.2.840.10045.4.3.2";
pub const ECDSA_SHA384: &str = "1.2.840.10045.4.3.3";
pub const ECDSA_SHA512: &str = "1.2.840.10045.4.3.4";

/// A hash function by its OID; also accepts the RSA/ECDSA-with-hash OIDs.
pub fn hash_of_oid(oid: &str) -> Option<&'static str> {
    Some(match oid {
        SHA1 | "1.2.840.113549.1.1.5" => SHA1,
        SHA256 | "1.2.840.113549.1.1.11" | ECDSA_SHA256 => SHA256,
        SHA384 | "1.2.840.113549.1.1.12" | ECDSA_SHA384 => SHA384,
        SHA512 | "1.2.840.113549.1.1.13" | ECDSA_SHA512 => SHA512,
        _ => return None,
    })
}

/// Digest of the concatenated parts.
pub fn digest(hash: &str, parts: &[&[u8]]) -> Result<Vec<u8>> {
    use sha2::Digest;
    fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
        let mut d = D::new();
        for p in parts {
            d.update(p);
        }
        d.finalize().to_vec()
    }
    Ok(match hash {
        SHA1 => run::<sha1::Sha1>(parts),
        SHA256 => run::<sha2::Sha256>(parts),
        SHA384 => run::<sha2::Sha384>(parts),
        SHA512 => run::<sha2::Sha512>(parts),
        _ => return Err(bad("unbekanntes Hashverfahren")),
    })
}

/// The parts of a certificate a signature needs.
#[derive(Debug, Clone)]
pub struct Cert {
    pub der: Vec<u8>,
    /// DER of the issuer and subject names (for IssuerAndSerialNumber).
    pub issuer_der: Vec<u8>,
    pub subject_der: Vec<u8>,
    /// Content of the serial number INTEGER.
    pub serial: Vec<u8>,
    pub subject: String,
    pub issuer: String,
    /// "Common name" of the subject (else the whole subject).
    pub name: String,
    pub email: String,
    /// "2026-11-04 12:00:00 UTC".
    pub not_before: String,
    pub not_after: String,
    pub key_alg: String,
    /// Curve OID of an EC key.
    pub curve: String,
    /// The public key bits (PKCS#1 RSAPublicKey or an EC point).
    pub public_key: Vec<u8>,
}

impl Cert {
    pub fn self_signed(&self) -> bool {
        self.issuer_der == self.subject_der
    }
}

/// Readable form of a Name ("CN=…, O=…").
fn name_text(t: &Tlv<'_>) -> (String, String, String) {
    let mut parts = Vec::new();
    let (mut cn, mut email) = (String::new(), String::new());
    for set in der::children(t.content).unwrap_or_default() {
        for atv in der::children(set.content).unwrap_or_default() {
            let kids = der::children(atv.content).unwrap_or_default();
            let (Some(o), Some(v)) = (kids.first(), kids.get(1)) else { continue };
            let value = der::text(v);
            let key = match der::oid_string(o).as_str() {
                "2.5.4.3" => {
                    cn = value.clone();
                    "CN"
                }
                "2.5.4.10" => "O",
                "2.5.4.11" => "OU",
                "2.5.4.6" => "C",
                "2.5.4.7" => "L",
                "2.5.4.8" => "ST",
                "1.2.840.113549.1.9.1" => {
                    email = value.clone();
                    "E"
                }
                _ => continue,
            };
            parts.push(format!("{key}={value}"));
        }
    }
    (parts.join(", "), cn, email)
}

/// "YYMMDDHHMMSSZ" / "YYYYMMDDHHMMSSZ" as "YYYY-MM-DD HH:MM:SS UTC".
fn time_text(t: &Tlv<'_>) -> String {
    let s = String::from_utf8_lossy(t.content).to_string();
    let s = if t.tag == der::UTC_TIME && s.len() >= 12 {
        let yy: u32 = s[..2].parse().unwrap_or(0);
        format!("{}{}", if yy < 50 { 20 } else { 19 }, s)
    } else {
        s
    };
    if s.len() < 14 {
        return s;
    }
    format!("{}-{}-{} {}:{}:{} UTC", &s[..4], &s[4..6], &s[6..8], &s[8..10], &s[10..12], &s[12..14])
}

/// Reads a DER certificate.
pub fn parse(der_bytes: &[u8]) -> Result<Cert> {
    let (cert, _) = der::read(der_bytes)?;
    let top = der::expect(&cert, der::SEQUENCE, "kein Zertifikat")?;
    let tbs = top.first().ok_or_else(|| bad("leer"))?;
    let f = der::expect(tbs, der::SEQUENCE, "TBSCertificate")?;
    // Version [0] is optional.
    let o = usize::from(f.first().is_some_and(|t| t.tag == 0xa0));
    let get = |i: usize| f.get(o + i).ok_or_else(|| bad("unvollständig"));
    let serial = get(0)?;
    let issuer = get(2)?;
    let validity = der::expect(get(3)?, der::SEQUENCE, "Gültigkeit")?;
    let subject = get(4)?;
    let spki = der::expect(get(5)?, der::SEQUENCE, "öffentlicher Schlüssel")?;
    let alg = der::expect(spki.first().ok_or_else(|| bad("Schlüsselart"))?, der::SEQUENCE, "Schlüsselart")?;
    let key_alg = alg.first().map(der::oid_string).unwrap_or_default();
    let curve = alg.get(1).filter(|t| t.tag == der::OID).map(der::oid_string).unwrap_or_default();
    let bits = spki.get(1).filter(|t| t.tag == der::BIT_STRING).ok_or_else(|| bad("Schlüssel"))?;
    let (subject_text, cn, email) = name_text(subject);
    let (issuer_text, _, _) = name_text(issuer);
    Ok(Cert {
        der: cert.raw.to_vec(),
        issuer_der: issuer.raw.to_vec(),
        subject_der: subject.raw.to_vec(),
        serial: serial.content.to_vec(),
        name: if cn.is_empty() { subject_text.clone() } else { cn },
        email,
        subject: subject_text,
        issuer: issuer_text,
        not_before: validity.first().map(time_text).unwrap_or_default(),
        not_after: validity.get(1).map(time_text).unwrap_or_default(),
        key_alg,
        curve,
        public_key: bits.content.get(1..).unwrap_or_default().to_vec(),
    })
}

/// DigestInfo for RSA PKCS#1 v1.5.
fn digest_info(hash: &str, digest: &[u8]) -> Vec<u8> {
    der::seq(&[&der::alg(hash, true), &der::octets(digest)])
}

/// A private key that signs.
pub enum Key {
    Rsa(Box<rsa::RsaPrivateKey>),
    P256(p256::ecdsa::SigningKey),
    P384(p384::ecdsa::SigningKey),
}

impl Key {
    /// From a PKCS#8 PrivateKeyInfo (as found in .pfx files).
    pub fn from_pkcs8(der_bytes: &[u8]) -> Result<Key> {
        use p256::pkcs8::DecodePrivateKey as _;
        if let Ok(k) = p256::SecretKey::from_pkcs8_der(der_bytes) {
            return Ok(Key::P256(k.into()));
        }
        if let Ok(k) = p384::SecretKey::from_pkcs8_der(der_bytes) {
            return Ok(Key::P384(k.into()));
        }
        rsa::RsaPrivateKey::from_pkcs8_der(der_bytes)
            .map(|k| Key::Rsa(Box::new(k)))
            .map_err(|_| bad("Schlüsselart wird nicht unterstützt (RSA, ECDSA P-256 oder P-384)"))
    }

    /// Signs a SHA-256 digest; returns the signature and its algorithm.
    pub fn sign_sha256(&self, digest: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        use p256::ecdsa::signature::hazmat::PrehashSigner;
        let failed = |_| bad("Signieren fehlgeschlagen");
        Ok(match self {
            Key::Rsa(k) => {
                let sig = k
                    .sign(rsa::Pkcs1v15Sign::new_unprefixed(), &digest_info(SHA256, digest))
                    .map_err(failed)?;
                (sig, der::alg(RSA, true))
            }
            Key::P256(k) => {
                let sig: p256::ecdsa::Signature =
                    k.sign_prehash(digest).map_err(|_| bad("Signieren fehlgeschlagen"))?;
                (sig.to_der().as_bytes().to_vec(), der::alg(ECDSA_SHA256, false))
            }
            Key::P384(k) => {
                let sig: p384::ecdsa::Signature =
                    k.sign_prehash(digest).map_err(|_| bad("Signieren fehlgeschlagen"))?;
                (sig.to_der().as_bytes().to_vec(), der::alg(ECDSA_SHA256, false))
            }
        })
    }
}

/// Checks `signature` over `digest` (made with `hash`) with the
/// certificate's public key.
pub fn verify(cert: &Cert, hash: &str, digest: &[u8], signature: &[u8]) -> Result<bool> {
    use p256::ecdsa::signature::hazmat::PrehashVerifier;
    match cert.key_alg.as_str() {
        RSA => {
            use rsa::pkcs1::DecodeRsaPublicKey as _;
            let key =
                rsa::RsaPublicKey::from_pkcs1_der(&cert.public_key).map_err(|_| bad("RSA-Schlüssel"))?;
            Ok(key.verify(rsa::Pkcs1v15Sign::new_unprefixed(), &digest_info(hash, digest), signature).is_ok())
        }
        EC_PUBLIC_KEY if cert.curve == P256 => {
            let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&cert.public_key)
                .map_err(|_| bad("EC-Schlüssel"))?;
            let Ok(sig) = p256::ecdsa::Signature::from_der(signature) else { return Ok(false) };
            Ok(key.verify_prehash(digest, &sig).is_ok())
        }
        EC_PUBLIC_KEY if cert.curve == P384 => {
            let key = p384::ecdsa::VerifyingKey::from_sec1_bytes(&cert.public_key)
                .map_err(|_| bad("EC-Schlüssel"))?;
            let Ok(sig) = p384::ecdsa::Signature::from_der(signature) else { return Ok(false) };
            Ok(key.verify_prehash(digest, &sig).is_ok())
        }
        _ => Err(bad("Schlüsselart wird nicht unterstützt")),
    }
}

/// Fields of a new self-signed certificate.
pub struct NewId<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub organisation: &'a str,
    pub years: u32,
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| bad("kein Zufall verfügbar"))?;
    Ok(b)
}

/// A new ECDSA P-256 key and a self-signed certificate for it (digital
/// signature and non-repudiation, for e-mail and documents). Returns the
/// certificate and the key as PKCS#8.
pub fn create_self_signed(id: &NewId<'_>, now_unix: i64) -> Result<(Vec<u8>, Vec<u8>)> {
    use p256::pkcs8::EncodePrivateKey as _;
    let secret = loop {
        if let Ok(k) = p256::SecretKey::from_slice(&random::<32>()?) {
            break k;
        }
    };
    let pkcs8 = secret.to_pkcs8_der().map_err(|_| bad("Schlüssel speichern"))?.as_bytes().to_vec();
    let signing: p256::ecdsa::SigningKey = secret.clone().into();
    let point = p256::ecdsa::VerifyingKey::from(&signing).to_encoded_point(false);
    let mut name_parts: Vec<Vec<u8>> = Vec::new();
    let mut rdn = |oid: &str, value: Vec<u8>| {
        name_parts.push(der::set_of(&[&der::seq(&[&der::oid(oid), &value])]));
    };
    rdn("2.5.4.3", der::utf8(id.name));
    if !id.organisation.trim().is_empty() {
        rdn("2.5.4.10", der::utf8(id.organisation.trim()));
    }
    if !id.email.trim().is_empty() {
        rdn("1.2.840.113549.1.9.1", der::ia5(id.email.trim()));
    }
    let refs: Vec<&[u8]> = name_parts.iter().map(Vec::as_slice).collect();
    let name = der::seq(&refs);
    let (y, mo, d, h, mi, s) = civil(now_unix);
    let validity = der::seq(&[
        &der::time(y, mo, d, h, mi, s),
        &der::time(y + i64::from(id.years.max(1)), mo, d.min(28), h, mi, s),
    ]);
    let mut serial = random::<16>()?;
    serial[0] &= 0x7f;
    let spki =
        der::seq(&[&der::seq(&[&der::oid(EC_PUBLIC_KEY), &der::oid(P256)]), &der::bits(point.as_bytes())]);
    // keyUsage: digitalSignature + nonRepudiation (critical).
    let key_usage =
        der::seq(&[&der::oid("2.5.29.15"), &[0x01, 0x01, 0xff], &der::octets(&[0x03, 0x02, 0x06, 0xc0])]);
    // extKeyUsage: e-mail protection, document signing.
    let eku_list = der::seq(&[&der::oid("1.3.6.1.5.5.7.3.4"), &der::oid("1.3.6.1.4.1.311.10.3.12")]);
    let ext_key_usage = der::seq(&[&der::oid("2.5.29.37"), &der::octets(&eku_list)]);
    let extensions = der::explicit(3, &der::seq(&[&key_usage, &ext_key_usage]));
    let alg = der::alg(ECDSA_SHA256, false);
    let tbs = der::seq(&[
        &der::explicit(0, &der::small_int(2)),
        &der::uint(&serial),
        &alg,
        &name,
        &validity,
        &name,
        &spki,
        &extensions,
    ]);
    let (sig, _) = Key::P256(signing).sign_sha256(&digest(SHA256, &[&tbs])?)?;
    let cert = der::seq(&[&tbs, &alg, &der::bits(&sig)]);
    Ok((cert, pkcs8))
}

/// (year, month, day, hour, minute, second) in UTC.
pub fn civil(unix: i64) -> (i64, u32, u32, u32, u32, u32) {
    let (days, rest) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, (rest / 3600) as u32, (rest / 60 % 60) as u32, (rest % 60) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_signed_certificate_reads_back_and_verifies() {
        let id =
            NewId { name: "Max Muster", email: "max@example.org", organisation: "Beispiel GmbH", years: 3 };
        let (cert_der, pkcs8) = create_self_signed(&id, 1_793_808_192).unwrap();
        let c = parse(&cert_der).unwrap();
        assert_eq!(c.name, "Max Muster");
        assert_eq!(c.email, "max@example.org");
        assert!(c.subject.contains("O=Beispiel GmbH"));
        assert!(c.self_signed());
        assert_eq!(c.not_before, "2026-11-04 16:03:12 UTC");
        assert!(c.not_after.starts_with("2029-11-04"));
        // The certificate's own signature is valid; the key signs and verifies.
        let key = Key::from_pkcs8(&pkcs8).unwrap();
        let d = digest(SHA256, &[b"hello"]).unwrap();
        let (sig, _) = key.sign_sha256(&d).unwrap();
        assert!(verify(&c, SHA256, &d, &sig).unwrap());
        let other = digest(SHA256, &[b"hellO"]).unwrap();
        assert!(!verify(&c, SHA256, &other, &sig).unwrap());
    }
}
