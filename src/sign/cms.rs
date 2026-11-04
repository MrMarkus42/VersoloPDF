//! The CMS (PKCS#7) part of a PDF signature: a detached SignedData with
//! the PAdES attributes (content type, message digest, signing
//! certificate), and reading one back for checking.

use super::cert::{self, Cert};
use super::der::{self, Tlv};
use super::id::DigitalId;
use crate::{Error, Result};

const SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
const DATA: &str = "1.2.840.113549.1.7.1";
const CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
const SIGNING_TIME: &str = "1.2.840.113549.1.9.5";
const SIGNING_CERT_V2: &str = "1.2.840.113549.1.9.16.2.47";

fn bad(what: &str) -> Error {
    Error::Invalid(format!("Signatur: {what}"))
}

fn attribute(oid: &str, value: &[u8]) -> Vec<u8> {
    der::seq(&[&der::oid(oid), &der::set_of(&[value])])
}

/// SignedData over a SHA-256 `digest` of the signed byte ranges.
pub fn signed_data(id: &DigitalId, digest: &[u8]) -> Result<Vec<u8>> {
    let cert_hash = cert::digest(cert::SHA256, &[&id.cert.der])?;
    // ESS signing-certificate-v2 (hash algorithm SHA-256 is the default).
    let ess = der::seq(&[&der::seq(&[&der::seq(&[&der::octets(&cert_hash)])])]);
    let attrs = [
        attribute(CONTENT_TYPE, &der::oid(DATA)),
        attribute(MESSAGE_DIGEST, &der::octets(digest)),
        attribute(SIGNING_CERT_V2, &ess),
    ];
    let refs: Vec<&[u8]> = attrs.iter().map(Vec::as_slice).collect();
    // Signed as a SET OF, stored with the tag [0].
    let as_set = der::set_of(&refs);
    let (signature, sig_alg) = id.key.sign_sha256(&cert::digest(cert::SHA256, &[&as_set])?)?;
    let mut signed_attrs = as_set.clone();
    signed_attrs[0] = 0xa0;
    let sid = der::seq(&[&id.cert.issuer_der, &der::uint(&id.cert.serial)]);
    let sha256 = der::alg(cert::SHA256, true);
    let signer_info =
        der::seq(&[&der::small_int(1), &sid, &sha256, &signed_attrs, &sig_alg, &der::octets(&signature)]);
    let mut certs: Vec<&[u8]> = vec![&id.cert.der];
    certs.extend(id.chain.iter().map(Vec::as_slice));
    let signed = der::seq(&[
        &der::small_int(1),
        &der::set_of(&[&sha256]),
        &der::seq(&[&der::oid(DATA)]),
        &der::implicit(0, &certs.concat()),
        &der::set_of(&[&signer_info]),
    ]);
    Ok(der::seq(&[&der::oid(SIGNED_DATA), &der::explicit(0, &signed)]))
}

/// A SignedData read back.
pub struct Parsed {
    pub certs: Vec<Cert>,
    /// The signer's certificate (index into `certs`).
    pub signer: Option<usize>,
    pub digest_alg: String,
    /// The signed attributes encoded as SET OF (what was signed).
    pub signed_attrs: Option<Vec<u8>>,
    pub message_digest: Option<Vec<u8>>,
    /// Signing time from the attributes, if any.
    pub signing_time: Option<String>,
    pub sig_alg: String,
    pub signature: Vec<u8>,
}

/// Reads a CMS ContentInfo with SignedData (trailing zero padding of the
/// /Contents string is allowed).
pub fn parse(data: &[u8]) -> Result<Parsed> {
    let (ci, _) = der::read(data)?;
    let ci = der::expect(&ci, der::SEQUENCE, "kein CMS")?;
    if ci.first().map(der::oid_string).as_deref() != Some(SIGNED_DATA) {
        return Err(bad("kein SignedData"));
    }
    let wrapped = ci.get(1).ok_or_else(|| bad("leer"))?;
    let inner = der::children(wrapped.content)?;
    let sd = der::expect(inner.first().ok_or_else(|| bad("leer"))?, der::SEQUENCE, "SignedData")?;
    let mut certs = Vec::new();
    let mut signer_infos: Option<Tlv<'_>> = None;
    for t in &sd {
        match t.tag {
            0xa0 => {
                for c in der::children(t.content)? {
                    if let Ok(p) = cert::parse(c.raw) {
                        certs.push(p);
                    }
                }
            }
            der::SET => signer_infos = Some(*t),
            _ => {}
        }
    }
    // The last SET is the signer infos (the first is the digest algorithms).
    let si_set = signer_infos.ok_or_else(|| bad("keine Unterzeichnerangaben"))?;
    let si_first = der::children(si_set.content)?;
    let si =
        der::expect(si_first.first().ok_or_else(|| bad("kein Unterzeichner"))?, der::SEQUENCE, "SignerInfo")?;
    // version, sid, digestAlgorithm, [0] signedAttrs?, signatureAlgorithm, signature
    let sid = si.get(1).ok_or_else(|| bad("Unterzeichner"))?;
    let digest_alg = si
        .get(2)
        .and_then(|a| der::children(a.content).ok())
        .and_then(|a| a.first().map(der::oid_string))
        .unwrap_or_default();
    let mut i = 3;
    let mut signed_attrs = None;
    let mut message_digest = None;
    let mut signing_time = None;
    if let Some(a) = si.get(3).filter(|t| t.tag == 0xa0) {
        let mut as_set = a.raw.to_vec();
        as_set[0] = der::SET;
        signed_attrs = Some(as_set);
        for attr in der::children(a.content)? {
            let parts = der::children(attr.content)?;
            let (Some(o), Some(vals)) = (parts.first(), parts.get(1)) else { continue };
            let first = der::children(vals.content)?.into_iter().next();
            match der::oid_string(o).as_str() {
                MESSAGE_DIGEST => message_digest = first.map(|v| v.content.to_vec()),
                SIGNING_TIME => signing_time = first.map(|v| String::from_utf8_lossy(v.content).into_owned()),
                _ => {}
            }
        }
        i = 4;
    }
    let sig_alg = si
        .get(i)
        .and_then(|a| der::children(a.content).ok())
        .and_then(|a| a.first().map(der::oid_string))
        .unwrap_or_default();
    let signature = si
        .get(i + 1)
        .filter(|t| t.tag == der::OCTET_STRING)
        .map(|t| t.content.to_vec())
        .ok_or_else(|| bad("keine Signatur"))?;
    // Find the signer's certificate: IssuerAndSerialNumber.
    let signer = der::children(sid.content).ok().and_then(|p| {
        let (issuer, serial) = (p.first()?, p.get(1)?);
        certs.iter().position(|c| c.issuer_der == issuer.raw && c.serial == serial.content)
    });
    Ok(Parsed { certs, signer, digest_alg, signed_attrs, message_digest, signing_time, sig_alg, signature })
}

/// Result of checking a SignedData against the digest of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Signature and digest are right.
    Valid,
    /// The document bytes do not match the signed digest.
    DigestMismatch,
    /// The signature value is wrong.
    BadSignature,
    /// Not checkable (unknown algorithm, no certificate …).
    Unsupported(String),
}

/// Checks the SignedData against the signed bytes `parts`.
pub fn check(p: &Parsed, parts: &[&[u8]]) -> Check {
    let Some(signer) = p.signer.and_then(|i| p.certs.get(i)) else {
        return Check::Unsupported("Zertifikat des Unterzeichners fehlt".into());
    };
    let Some(hash) = cert::hash_of_oid(&p.digest_alg) else {
        return Check::Unsupported(format!("Hashverfahren {}", p.digest_alg));
    };
    let Ok(doc_digest) = cert::digest(hash, parts) else { return Check::Unsupported("Hash".into()) };
    let (signed_digest, what) = match (&p.signed_attrs, &p.message_digest) {
        (Some(attrs), Some(md)) => {
            if *md != doc_digest {
                return Check::DigestMismatch;
            }
            (cert::digest(hash, &[attrs]).unwrap_or_default(), true)
        }
        (None, _) => (doc_digest, false),
        (Some(_), None) => return Check::Unsupported("Nachrichten-Hash fehlt".into()),
    };
    let _ = what;
    match cert::verify(signer, hash, &signed_digest, &p.signature) {
        Ok(true) => Check::Valid,
        Ok(false) => Check::BadSignature,
        Err(e) => Check::Unsupported(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::cert::NewId;

    #[test]
    fn signed_data_checks_out() {
        let id =
            DigitalId::create(&NewId { name: "CMS Test", email: "", organisation: "", years: 1 }).unwrap();
        let content: &[u8] = b"Inhalt der Signatur";
        let d = cert::digest(cert::SHA256, &[content]).unwrap();
        let cms = signed_data(&id, &d).unwrap();
        let p = parse(&cms).unwrap();
        assert_eq!(p.certs.len(), 1);
        assert_eq!(p.signer, Some(0));
        assert_eq!(check(&p, &[content]), Check::Valid);
        assert_eq!(check(&p, &[b"anderer Inhalt"]), Check::DigestMismatch);
        // Zero padding after the structure (as in a PDF) is fine.
        let mut padded = cms.clone();
        padded.extend([0u8; 64]);
        assert_eq!(check(&parse(&padded).unwrap(), &[content]), Check::Valid);
    }
}
