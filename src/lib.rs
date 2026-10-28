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
pub mod calculator;
pub mod compare;
pub mod compose;
pub mod convert;
pub mod doc_props;
pub mod document;
pub mod download;
pub mod engine;
pub mod error;
pub mod export_image;
pub mod files;
pub mod font_replace;
pub mod form_fill;
pub mod forms;
pub mod geometry;
pub mod import;
pub mod marks;
pub mod media;
pub mod model;
pub mod ocr;
pub mod office;
pub mod optimize;
pub mod outline;
pub mod outline_edit;
pub mod outlines;
pub mod pages;
pub mod pdf_edit;
pub mod portfolio;
pub mod print;
pub mod print_layout;
pub mod redact;
pub mod report;
pub mod search;
pub mod security;
pub mod speech;
pub mod spell;
pub mod text;
pub mod transitions;
pub mod ui;
pub mod xfdf;

pub use document::Document;
pub use error::{Error, Result};
