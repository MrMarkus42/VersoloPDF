//! Digital signatures ("Dokument signieren", "Signaturen überprüfen"):
//! digital IDs (.pfx/.p12 or self-made), PAdES signatures appended as
//! incremental updates, and checking the signatures of a document.

pub mod cert;
pub mod cms;
pub mod der;
pub mod id;
pub mod pdf;
pub mod tsa;
