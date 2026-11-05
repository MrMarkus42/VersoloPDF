//! Time stamps (RFC 3161): a time stamp authority ("Zeitstempeldienst")
//! confirms that a hash value existed at a certain time. VersoloPDF sends
//! only the SHA-256 hash, never the document, with the system's `curl`.
//!
//! Used for signatures with a time stamp (unsigned attribute of the CMS)
//! and for document time stamps (/DocTimeStamp, ETSI.RFC3161).

use super::cert;
use super::cms::{self, Check};
use super::der;
use crate::{Error, Result};

/// The time stamp authority used unless another one is set.
pub const DEFAULT_URL: &str = "http://timestamp.digicert.com";
/// Largest accepted answer.
const MAX_ANSWER: u64 = 64 * 1024;

fn bad(what: impl Into<String>) -> Error {
    Error::Invalid(format!("Zeitstempel: {}", what.into()))
}

/// TimeStampReq for a SHA-256 `hash` (with a nonce, asking for the
/// authority's certificate).
pub fn request(hash: &[u8], nonce: &[u8]) -> Vec<u8> {
    let imprint = der::seq(&[&der::alg(cert::SHA256, true), &der::octets(hash)]);
    let cert_req = der::tlv(0x01, &[0xff]);
    der::seq(&[&der::small_int(1), &imprint, &der::uint(nonce), &cert_req])
}

/// The token (CMS ContentInfo) of a TimeStampResp.
pub fn token_of_response(resp: &[u8]) -> Result<Vec<u8>> {
    let (top, _) = der::read(resp)?;
    let parts = der::expect(&top, der::SEQUENCE, "keine Antwort")?;
    let status = parts.first().map(|s| der::children(s.content)).transpose()?.unwrap_or_default();
    let code =
        status.first().filter(|t| t.tag == der::INTEGER).map(|t| t.content.to_vec()).unwrap_or_default();
    // 0 granted, 1 granted with modifications.
    if !(code == [0] || code == [1]) {
        let text = status.get(1).map(|t| der::children(t.content).unwrap_or_default());
        let text = text.and_then(|t| t.first().map(der::text)).unwrap_or_default();
        return Err(bad(format!("vom Dienst abgelehnt {text}")));
    }
    parts.get(1).map(|t| t.raw.to_vec()).ok_or_else(|| bad("Antwort ohne Zeitstempel"))
}

/// The hash and the time ("20261105112233Z") inside a TSTInfo.
pub fn tst_info(info: &[u8]) -> Result<(Vec<u8>, String)> {
    let (top, _) = der::read(info)?;
    let parts = der::expect(&top, der::SEQUENCE, "TSTInfo")?;
    // version, policy, messageImprint, serialNumber, genTime, ...
    let imprint = parts.get(2).ok_or_else(|| bad("TSTInfo unvollständig"))?;
    let imprint = der::expect(imprint, der::SEQUENCE, "messageImprint")?;
    let hash = imprint.get(1).filter(|t| t.tag == der::OCTET_STRING).ok_or_else(|| bad("Hash fehlt"))?;
    let time = parts
        .get(4)
        .filter(|t| t.tag == der::GENERALIZED_TIME)
        .map(|t| String::from_utf8_lossy(t.content).into_owned())
        .ok_or_else(|| bad("Zeit fehlt"))?;
    Ok((hash.content.to_vec(), time))
}

/// "20261105112233.5Z" as "05.11.2026 11:22:33 UTC".
pub fn readable_time(t: &str) -> String {
    let n = |a: usize, b: usize| t.get(a..b).unwrap_or("00");
    if t.len() < 14 {
        return t.to_string();
    }
    format!("{}.{}.{} {}:{}:{} UTC", n(6, 8), n(4, 6), n(0, 4), n(8, 10), n(10, 12), n(12, 14))
}

/// A checked time stamp: its time (readable) and the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub time: String,
    pub authority: String,
    pub check: Check,
}

/// Checks a token against the SHA-256 `hash` it should confirm.
pub fn check_token(token: &[u8], hash: &[u8]) -> Stamp {
    let mut stamp =
        Stamp { time: String::new(), authority: String::new(), check: Check::Unsupported(String::new()) };
    let parsed = match cms::parse(token) {
        Ok(p) => p,
        Err(e) => {
            stamp.check = Check::Unsupported(e.to_string());
            return stamp;
        }
    };
    if let Some(c) = parsed.signer.and_then(|i| parsed.certs.get(i)) {
        stamp.authority = c.name.clone();
    }
    let Some(info) = parsed.content.as_deref().filter(|_| parsed.content_type == cms::TST_INFO) else {
        stamp.check = Check::Unsupported("kein Zeitstempel".into());
        return stamp;
    };
    match tst_info(info) {
        Ok((stamped, time)) => {
            stamp.time = readable_time(&time);
            stamp.check = if stamped != hash { Check::DigestMismatch } else { cms::check(&parsed, &[info]) };
        }
        Err(e) => stamp.check = Check::Unsupported(e.to_string()),
    }
    stamp
}

/// Asks the authority at `url` for a time stamp of the SHA-256 `hash` and
/// returns the token. The answer is checked before it is used.
pub fn fetch(url: &str, hash: &[u8]) -> Result<Vec<u8>> {
    if crate::mail::no_external() {
        return Err(bad("im Testbetrieb keine Netzverbindung"));
    }
    if !crate::download::valid_url(url) {
        return Err(bad("ungültige Adresse des Zeitstempeldienstes"));
    }
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| bad(e.to_string()))?;
    let dir = std::env::temp_dir();
    let tag = format!(
        "versolopdf-tsa-{}-{}",
        std::process::id(),
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let (req_path, resp_path) = (dir.join(format!("{tag}.tsq")), dir.join(format!("{tag}.tsr")));
    std::fs::write(&req_path, request(hash, &nonce)).map_err(|e| Error::io(&req_path, e))?;
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--max-redirs", "5", "--connect-timeout", "10", "--max-time", "30"])
        .args(["--max-filesize", &MAX_ANSWER.to_string()])
        .args(["--header", "Content-Type: application/timestamp-query"])
        .arg("--data-binary")
        .arg(format!("@{}", req_path.display()))
        .arg("--output")
        .arg(&resp_path)
        .arg(url);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd.output();
    std::fs::remove_file(&req_path).ok();
    let output = output.map_err(|e| bad(format!("curl: {e}")))?;
    let answer = std::fs::read(&resp_path);
    std::fs::remove_file(&resp_path).ok();
    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().chars().take(300).collect::<String>();
        return Err(bad(if msg.is_empty() { "Dienst nicht erreichbar".into() } else { msg }));
    }
    let token = token_of_response(&answer.map_err(|e| Error::io(&resp_path, e))?)?;
    let stamp = check_token(&token, hash);
    if stamp.check != Check::Valid {
        return Err(bad(format!("Antwort ungültig ({:?})", stamp.check)));
    }
    Ok(token)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sign::cert::NewId;
    use crate::sign::id::DigitalId;

    /// A time stamp authority made with a self-made ID (for tests).
    pub(crate) fn fake_token(hash: &[u8], time: &str) -> Vec<u8> {
        let tsa = DigitalId::create(&NewId {
            name: "Test-Zeitstempeldienst",
            email: "",
            organisation: "",
            years: 1,
        })
        .unwrap();
        let imprint = der::seq(&[&der::alg(cert::SHA256, true), &der::octets(hash)]);
        let info = der::seq(&[
            &der::small_int(1),
            &der::oid("1.2.3.4.1"),
            &imprint,
            &der::small_int(42),
            &der::tlv(der::GENERALIZED_TIME, time.as_bytes()),
        ]);
        cms::encapsulated(&tsa, cms::TST_INFO, &info).unwrap()
    }

    #[test]
    fn request_and_response_are_read() {
        let hash = [7u8; 32];
        let req = request(&hash, &[1, 2, 3]);
        let (t, rest) = der::read(&req).unwrap();
        assert!(rest.is_empty());
        assert_eq!(der::children(t.content).unwrap().len(), 4);
        let token = fake_token(&hash, "20261105112233Z");
        let resp = der::seq(&[&der::seq(&[&der::small_int(0)]), &token]);
        assert_eq!(token_of_response(&resp).unwrap(), token);
        let refused = der::seq(&[&der::seq(&[&der::small_int(2)])]);
        assert!(token_of_response(&refused).is_err());
    }

    #[test]
    fn tokens_are_checked() {
        let hash = [9u8; 32];
        let token = fake_token(&hash, "20261105112233Z");
        let stamp = check_token(&token, &hash);
        assert_eq!(stamp.check, Check::Valid);
        assert_eq!(stamp.time, "05.11.2026 11:22:33 UTC");
        assert_eq!(stamp.authority, "Test-Zeitstempeldienst");
        assert_eq!(check_token(&token, &[1u8; 32]).check, Check::DigestMismatch);
        // A changed time breaks the authority's signature.
        let mut forged = token.clone();
        let at = forged.windows(4).position(|w| w == b"2026").unwrap();
        forged[at + 3] = b'7';
        assert_eq!(check_token(&forged, &hash).check, Check::DigestMismatch);
    }
}
