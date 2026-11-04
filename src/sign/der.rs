//! A small DER (ASN.1) reader and writer: what signatures, certificates
//! and the CMS structure of a PDF signature need, nothing more.

use crate::{Error, Result};

pub const SEQUENCE: u8 = 0x30;
pub const SET: u8 = 0x31;
pub const INTEGER: u8 = 0x02;
pub const BIT_STRING: u8 = 0x03;
pub const OCTET_STRING: u8 = 0x04;
pub const NULL: u8 = 0x05;
pub const OID: u8 = 0x06;
pub const UTF8_STRING: u8 = 0x0c;
pub const PRINTABLE_STRING: u8 = 0x13;
pub const IA5_STRING: u8 = 0x16;
pub const UTC_TIME: u8 = 0x17;
pub const GENERALIZED_TIME: u8 = 0x18;

fn bad(what: &str) -> Error {
    Error::Invalid(format!("Signatur: {what}"))
}

// ---------------------------------------------------------------------
// Writing

/// Tag, length and content.
pub fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = content.len();
    if n < 0x80 {
        out.push(n as u8);
    } else {
        let bytes: Vec<u8> = n.to_be_bytes().into_iter().skip_while(|b| *b == 0).collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(content);
    out
}

pub fn seq(parts: &[&[u8]]) -> Vec<u8> {
    tlv(SEQUENCE, &parts.concat())
}

/// SET OF: the elements sorted by their encoding, as DER requires.
pub fn set_of(parts: &[&[u8]]) -> Vec<u8> {
    let mut v: Vec<&[u8]> = parts.to_vec();
    v.sort();
    tlv(SET, &v.concat())
}

/// [n] EXPLICIT (constructed context tag).
pub fn explicit(n: u8, content: &[u8]) -> Vec<u8> {
    tlv(0xa0 | n, content)
}

/// [n] IMPLICIT constructed (e.g. a SET OF given another tag).
pub fn implicit(n: u8, content: &[u8]) -> Vec<u8> {
    tlv(0xa0 | n, content)
}

/// An unsigned big-endian integer (a leading zero added when needed).
pub fn uint(bytes: &[u8]) -> Vec<u8> {
    let mut b: Vec<u8> = bytes.iter().copied().skip_while(|x| *x == 0).collect();
    if b.is_empty() {
        b.push(0);
    }
    if b[0] & 0x80 != 0 {
        b.insert(0, 0);
    }
    tlv(INTEGER, &b)
}

pub fn small_int(v: u32) -> Vec<u8> {
    uint(&v.to_be_bytes())
}

pub fn octets(b: &[u8]) -> Vec<u8> {
    tlv(OCTET_STRING, b)
}

pub fn bits(b: &[u8]) -> Vec<u8> {
    let mut c = vec![0u8];
    c.extend_from_slice(b);
    tlv(BIT_STRING, &c)
}

pub fn null() -> Vec<u8> {
    vec![NULL, 0]
}

/// An object identifier from its dotted form ("1.2.840.113549.1.7.2").
pub fn oid(dotted: &str) -> Vec<u8> {
    let n: Vec<u64> = dotted.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let mut c = vec![(n[0] * 40 + n.get(1).copied().unwrap_or(0)) as u8];
    for &v in &n[2..] {
        let mut chunk = vec![(v & 0x7f) as u8];
        let mut v = v >> 7;
        while v > 0 {
            chunk.push(0x80 | (v & 0x7f) as u8);
            v >>= 7;
        }
        chunk.reverse();
        c.extend(chunk);
    }
    tlv(OID, &c)
}

pub fn utf8(s: &str) -> Vec<u8> {
    tlv(UTF8_STRING, s.as_bytes())
}

pub fn ia5(s: &str) -> Vec<u8> {
    tlv(IA5_STRING, s.as_bytes())
}

/// UTCTime "YYMMDDHHMMSSZ" (years 1950–2049) else GeneralizedTime.
pub fn time(year: i64, month: u32, day: u32, h: u32, m: u32, s: u32) -> Vec<u8> {
    if (1950..2050).contains(&year) {
        tlv(UTC_TIME, format!("{:02}{month:02}{day:02}{h:02}{m:02}{s:02}Z", year % 100).as_bytes())
    } else {
        tlv(GENERALIZED_TIME, format!("{year:04}{month:02}{day:02}{h:02}{m:02}{s:02}Z").as_bytes())
    }
}

/// AlgorithmIdentifier without parameters, or with NULL.
pub fn alg(dotted: &str, with_null: bool) -> Vec<u8> {
    if with_null { seq(&[&oid(dotted), &null()]) } else { seq(&[&oid(dotted)]) }
}

// ---------------------------------------------------------------------
// Reading

/// One element: tag, content and the whole encoding.
#[derive(Debug, Clone, Copy)]
pub struct Tlv<'a> {
    pub tag: u8,
    pub content: &'a [u8],
    pub raw: &'a [u8],
}

/// Reads one element from the start of `data`; returns it and the rest.
pub fn read(data: &[u8]) -> Result<(Tlv<'_>, &[u8])> {
    if data.len() < 2 {
        return Err(bad("zu kurz"));
    }
    let tag = data[0];
    let (len, head) = if data[1] < 0x80 {
        (data[1] as usize, 2)
    } else {
        let k = (data[1] & 0x7f) as usize;
        if k == 0 || k > 4 || data.len() < 2 + k {
            return Err(bad("ungültige Länge"));
        }
        let mut len = 0usize;
        for &b in &data[2..2 + k] {
            len = (len << 8) | b as usize;
        }
        (len, 2 + k)
    };
    let end = head.checked_add(len).filter(|e| *e <= data.len()).ok_or_else(|| bad("Länge über das Ende"))?;
    Ok((Tlv { tag, content: &data[head..end], raw: &data[..end] }, &data[end..]))
}

/// All elements inside `content`.
pub fn children(content: &[u8]) -> Result<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    let mut rest = content;
    while !rest.is_empty() {
        let (t, r) = read(rest)?;
        out.push(t);
        rest = r;
    }
    Ok(out)
}

/// The elements of a constructed element with the expected tag.
pub fn expect<'a>(t: &Tlv<'a>, tag: u8, what: &str) -> Result<Vec<Tlv<'a>>> {
    if t.tag != tag {
        return Err(bad(what));
    }
    children(t.content)
}

/// Dotted form of an OID element.
pub fn oid_string(t: &Tlv<'_>) -> String {
    if t.tag != OID || t.content.is_empty() {
        return String::new();
    }
    let c = t.content;
    let mut parts = vec![(c[0] / 40).min(2) as u64, (c[0] as u64) - 40 * (c[0] / 40).min(2) as u64];
    let mut v = 0u64;
    for &b in &c[1..] {
        v = (v << 7) | (b & 0x7f) as u64;
        if b & 0x80 == 0 {
            parts.push(v);
            v = 0;
        }
    }
    parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
}

/// Text of a string element (UTF8, Printable, IA5, BMP, Teletex as Latin-1).
pub fn text(t: &Tlv<'_>) -> String {
    match t.tag {
        0x1e => {
            // BMPString: UTF-16BE.
            let u: Vec<u16> =
                t.content.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
            String::from_utf16_lossy(&u)
        }
        0x14 => t.content.iter().map(|&b| b as char).collect(),
        _ => String::from_utf8_lossy(t.content).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oids_round_trip() {
        for o in ["1.2.840.113549.1.7.2", "2.16.840.1.101.3.4.2.1", "1.2.840.10045.4.3.2", "2.5.4.3"] {
            let enc = oid(o);
            let (t, rest) = read(&enc).unwrap();
            assert!(rest.is_empty());
            assert_eq!(oid_string(&t), o);
        }
        // Known encoding of sha256.
        assert_eq!(
            oid("2.16.840.1.101.3.4.2.1"),
            [0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01]
        );
    }

    #[test]
    fn long_lengths_and_sets() {
        let big = vec![7u8; 300];
        let e = octets(&big);
        assert_eq!(&e[..4], &[0x04, 0x82, 0x01, 0x2c]);
        let (t, _) = read(&e).unwrap();
        assert_eq!(t.content.len(), 300);
        // SET OF is sorted.
        let s = set_of(&[&small_int(5), &small_int(1)]);
        assert_eq!(s, [0x31, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x05]);
        assert_eq!(uint(&[0x80]), [0x02, 0x02, 0x00, 0x80]);
    }
}
