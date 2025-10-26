# symspellrs

[![Crates.io](https://img.shields.io/crates/v/symspellrs.svg)](https://crates.io/crates/symspellrs)
[![Docs.rs](https://docs.rs/symspellrs/badge.svg)](https://docs.rs/symspellrs)

A compact Rust library implementing a SymSpell-style fuzzy-word suggestion algorithm.
It supports two primary modes:

- Compile-time embedding — use the `include_dictionary!` proc-macro to embed a dictionary
  (and optionally a precomputed deletion index) into the binary.
- Runtime construction — build a `SymSpell` instance at runtime from an iterator of
  `(String, usize)` pairs (word, frequency).

This README contains quick usage commands, example snippets and developer commands.

Quick commands
--------------

- Run the included example (demonstrates compile-time and runtime usage):
  ```bash
  cargo run --example simple_usage
  ```

- Run the test suite:
  ```bash
  cargo test --workspace
  ```

- Format and lint:
  ```bash
  cargo fmt --all
  cargo clippy --all-targets --all-features -- -D warnings
  ```

- Build release artifacts:
  ```bash
  cargo build --workspace --release
  ```

Simple usage examples
---------------------

These small snippets show the most common usage patterns. See `examples/simple_usage.rs`
for a complete runnable example.

1) Compile-time embedding (recommended when your dictionary is static)

The `include_dictionary!` proc-macro reads a dictionary file at compile time (path is evaluated
relative to the crate root) and returns a ready-to-use value (when `precompute = true` you get
an `EmbeddedSymSpell`-like value backed by `phf` statics; otherwise the macro constructs a
runtime `SymSpell`).

```ignore
use symspellrs::include_dictionary;
use symspellrs::Verbosity;

// Read tests/data/words.txt at compile time and build a ready value.
let sym = include_dictionary!("tests/data/words.txt", max_distance = 2, lowercase = true);

// Query the embedded instance:
let maybe_best = sym.find_top("helo");             // Option<Suggestion>
let closest = sym.lookup("helo", 2, Verbosity::Closest);
```

2) Runtime construction (dynamic dictionaries)

If you load dictionaries from the network, a database, or need to modify them at runtime,
use `SymSpell::from_iter` or `SymSpell::load_iter`:

```ignore
use symspellrs::{SymSpell, Verbosity};

let entries = vec![
    ("hello".to_string(), 3usize),
    ("world".to_string(), 5usize),
];

let sym = SymSpell::from_iter(2, entries);
let results = sym.lookup("helo", 2, Verbosity::Top);
```

Where to look
--------------

- Example: `examples/simple_usage.rs` — shows both compile-time embedding and runtime builder.
- Tests: `tests/` — includes tests that exercise the compile-time macro and runtime lookup.
