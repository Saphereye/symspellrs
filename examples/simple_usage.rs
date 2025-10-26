/*!
simple_usage.rs

Example demonstrating:
- compile-time dictionary embedding using `include_dictionary!` proc-macro (returns a ready `SymSpell`)
- runtime construction of `SymSpell` from an iterator of `(String, usize)` pairs

Place this file under `examples/` and run with:
    cargo run --example simple_usage

Note: the `include_dictionary!` macro expects the path you pass to be relative to the crate root
(evaluated using `CARGO_MANIFEST_DIR`). This example uses `tests/data/words.txt` which is included
in the repository for the tests.
*/

use symspellrs::{include_dictionary, SymSpell, Verbosity};

fn print_suggestions(title: &str, suggestions: &[symspellrs::Suggestion]) {
    println!("-- {} ({} suggestions) --", title, suggestions.len());
    for s in suggestions {
        println!(
            "  term: {:<12} distance: {:>2} frequency: {}",
            s.term, s.distance, s.frequency
        );
    }
}

fn example_compile_time() {
    // Build a SymSpell instance at compile time from the provided dictionary file.
    // The macro returns a ready-to-use `SymSpell` value.
    //
    // Options:
    // - max_distance = 2
    // - lowercase = true (normalize the dictionary to lower-case)
    //
    // The path is evaluated relative to the crate root at compile time.
    let sym = include_dictionary!("tests/data/words.txt", max_distance = 2, lowercase = true);

    println!("=== Compile-time built SymSpell ===");

    // Exact lookup (Top verbosity)
    let exact = sym.lookup("worl", 2, Verbosity::Top);
    print_suggestions("Exact lookup for 'worl'", &exact);

    // Misspelling: 'helo' -> expect 'hello'
    let suggestions = sym.lookup("helo", 2, Verbosity::Closest);
    print_suggestions("Closest suggestions for 'helo'", &suggestions);

    // All suggestions within distance 2 for 'teso'
    let all = sym.lookup("teso", 2, Verbosity::All);
    print_suggestions("All suggestions for 'teso'", &all);
}

fn example_runtime_build() {
    println!("\n=== Runtime-built SymSpell ===");

    // Build a SymSpell from an iterator at runtime.
    // You might prefer this approach when you load dictionaries from a dynamic source.
    let entries = vec![
        ("hello".to_string(), 3usize),
        ("hell".to_string(), 1usize),
        ("help".to_string(), 1usize),
        ("world".to_string(), 5usize),
        ("test".to_string(), 2usize),
        ("tost".to_string(), 4usize),
        ("applied".to_string(), 1usize),
        ("apple".to_string(), 2usize),
        ("apply".to_string(), 1usize),
    ];

    let sym = SymSpell::from_iter(2, entries);

    // Example lookups
    let s1 = sym.lookup("helo", 2, Verbosity::Top);
    print_suggestions("Top suggestion for 'helo' (runtime)", &s1);

    let s2 = sym.lookup("appl", 2, Verbosity::Closest);
    print_suggestions("Closest suggestions for 'appl' (runtime)", &s2);

    let s3 = sym.lookup("testo", 2, Verbosity::All);
    print_suggestions("All suggestions for 'testo' (runtime)", &s3);
}

fn main() {
    println!("symspellrs example: compile-time macro and runtime builder\n");

    example_compile_time();
    example_runtime_build();

    println!("\nDone.");
}
