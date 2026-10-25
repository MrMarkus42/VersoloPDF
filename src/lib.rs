//! MrPDF – a free, open-source PDF viewer and editor.
//!
//! The crate is split into a toolkit independent document core
//! ([`document`], [`pdf_edit`], [`appearance`], [`text`], [`search`]) and the
//! egui based user interface in [`ui`].
//!
//! `unsafe` code is denied everywhere except in the small, reviewed Windows
//! printing module (`print::windows_print`), which has to call Win32 APIs.

#![deny(unsafe_code)]

pub mod accessibility;
pub mod appearance;
pub mod compare;
pub mod convert;
pub mod document;
pub mod engine;
pub mod error;
pub mod forms;
pub mod geometry;
pub mod model;
pub mod outline;
pub mod pages;
pub mod pdf_edit;
pub mod print;
pub mod redact;
pub mod report;
pub mod search;
pub mod security;
pub mod text;
pub mod ui;
pub mod xfdf;

pub use document::Document;
pub use error::{Error, Result};
