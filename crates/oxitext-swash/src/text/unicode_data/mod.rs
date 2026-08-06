// OXITEXT MODIFICATION (oxitext 0.2.2): upstream shipped this as one generated
// 5 491-line `src/text/unicode_data.rs`, over the workspace's 2000-line limit
// (CONTRIBUTING.md). It is split here into seven submodules along the natural
// seams of the generated data, with the tables byte-for-byte unchanged and the
// public paths preserved by the re-exports below -- `super::unicode_data::X`
// still resolves for every `X`, so no consumer changed. See ../../PROVENANCE.md.
//! Generated Unicode character-database tables.
//!
//! Automatically generated from the Unicode 13.0.0 character database (see
//! [`enums::UNICODE_VERSION`]). Split across submodules purely for file size;
//! everything is re-exported here, so callers use `unicode_data::X` and never
//! name a submodule:
//!
//! | Submodule | Contents |
//! |---|---|
//! | [`enums`] | the property enums and `UNICODE_VERSION` |
//! | [`script_tables`] | script tags, names, complexity, brackets, mirrors |
//! | [`record_index`] | the three-level code-point → record-index trie |
//! | [`records`] | [`records::Record`], [`records::Flags`] and `RECORDS` |
//! | [`compose`] | canonical-composition tables and their trie |
//! | [`decompose_index`] | canonical and compatibility decomposition tries |
//! | [`decompose`] | the decomposition data itself |

#![allow(dead_code)]

pub mod compose;
pub mod decompose;
pub mod decompose_index;
pub mod enums;
pub mod record_index;
pub mod records;
pub mod script_tables;

pub use compose::*;
pub use decompose::*;
pub use decompose_index::*;
pub use enums::*;
pub use record_index::*;
pub use records::*;
pub use script_tables::*;
