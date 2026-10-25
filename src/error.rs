//! Error type shared by the document core.

use std::path::PathBuf;

use pdfium_render::prelude::PdfiumError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("PDFium library not found (searched: {0})")]
    EngineUnavailable(String),

    #[error("the document is password protected")]
    PasswordRequired,

    #[error("the password is not correct")]
    WrongPassword,

    #[error("the file is not a valid PDF document")]
    InvalidFormat,

    #[error("the file is too large ({size} bytes, at most {limit} bytes are supported)")]
    FileTooLarge { size: u64, limit: u64 },

    #[error("could not access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("page {0} does not exist")]
    PageOutOfRange(usize),

    #[error("annotation {0} does not exist on this page")]
    AnnotationOutOfRange(usize),

    #[error("this operation is not available for encrypted documents")]
    EncryptedNotSupported,

    #[error("the document does not allow this operation")]
    NotPermitted,

    #[error("{0}")]
    Invalid(String),

    #[error("PDF engine error: {0}")]
    Pdfium(String),

    #[error("PDF structure error: {0}")]
    Structure(String),
}

impl From<PdfiumError> for Error {
    fn from(err: PdfiumError) -> Self {
        Error::Pdfium(format!("{err:?}"))
    }
}

impl From<lopdf::Error> for Error {
    fn from(err: lopdf::Error) -> Self {
        Error::Structure(err.to_string())
    }
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
