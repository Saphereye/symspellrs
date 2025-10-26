//! symspellrs - library exports and PHF integration point
//!
//! This crate exposes the `symspell` module which contains the core implementation.
//! It also re-exports a compile-time proc-macro `include_dictionary!` (from the
//! `symspellrs_macros` crate) that can embed a dictionary and a precomputed
//! deletion-index (PHF maps) at compile time and return an `EmbeddedSymSpell`.
//!
//! Examples
//!
//! - Compile-time embedding (recommended when you want the dictionary embedded in
//!   the binary):
//!
//! ```ignore
//! use symspellrs::include_dictionary;
//!
//! // Returns an `EmbeddedSymSpell` constructed from PHF statics emitted by the macro.
//! let embedded = include_dictionary!("path/to/words.txt", max_distance = 2, lowercase = true);
//! let suggestions = embedded.find_top("helo");
//! ```
//!
//! - Runtime construction (useful when loading dictionaries from dynamic sources):
//!
//! ```ignore
//! use symspellrs::{SymSpell, Verbosity};
//! let entries = vec![("hello".to_string(), 1usize), ("world".to_string(), 1usize)];
//! let sym = SymSpell::from_iter(2, entries);
//! let results = sym.lookup("helo", 2, Verbosity::Closest);
//! ```

pub mod symspell;

/// Re-export commonly used types from the `symspell` module.
pub use symspell::{EmbeddedSymSpell, Suggestion, SymSpell, Verbosity};

/// Re-export the compile-time dictionary macro from the proc-macro crate.
///
/// The proc-macro crate is published as the workspace member `symspellrs-macros`
/// and exposes the macro as `symspellrs_macros::include_dictionary`. Re-exporting
/// it here makes it convenient for consumers to call:
///
///   use symspellrs::include_dictionary;
pub use symspellrs_macros::include_dictionary;
