#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod document;
mod error;
mod parse;

#[cfg(feature = "serde")]
mod de;
#[cfg(feature = "serde")]
mod ser;

pub use document::{Document, FileKind, Section, SectionId, SectionKind, SectionMut};
#[cfg(feature = "serde")]
pub use error::SerdeError;
pub use error::{EditError, ParseError, ParseErrorKind};
pub use parse::is_valid_identifier;

/// The format specification, and how this crate reads, edits and
/// (de)serializes files.
#[doc = include_str!("../SPEC.md")]
pub mod spec {}
