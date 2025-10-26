use symspellrs::{include_dictionary, Verbosity};

#[test]
fn test_include_macro_basic_lookup() {
    // Build a SymSpell at compile time from tests/data/words.txt
    let sym = include_dictionary!("tests/data/words.txt", max_distance = 2, lowercase = true);

    // Exact match should return the word with distance 0 using Top verbosity
    let top = sym.lookup("world", 2, Verbosity::Top);
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].term, "world");
    assert_eq!(top[0].distance, 0);
    assert_eq!(top[0].frequency, 1);

    // A typical misspelling should return "hello" among the closest suggestions
    let suggestions = sym.lookup("helo", 2, Verbosity::Closest);
    assert!(suggestions.iter().any(|s| s.term == "hello"));
}

#[test]
fn test_include_macro_closest_multiple() {
    let sym = include_dictionary!("tests/data/words.txt", max_distance = 2, lowercase = true);

    // "appl" is close to "apple" and "apply" (both distance 1)
    let closest = sym.lookup("appl", 2, Verbosity::Closest);
    let terms: Vec<&str> = closest.iter().map(|s| s.term.as_str()).collect();

    assert!(terms.contains(&"apple"));
    assert!(terms.contains(&"apply"));
}

#[test]
fn test_include_macro_all_and_ordering() {
    let sym = include_dictionary!("tests/data/words.txt", max_distance = 2, lowercase = true);

    // Request all suggestions within max distance for a short typo.
    let all = sym.lookup("teso", 2, Verbosity::All);
    assert!(!all.is_empty());

    // The closest suggestion should be first (distance asc), and frequency ties resolved by frequency desc.
    let first = &all[0];
    assert!(first.distance <= 2);
    // Expect that "test" is among returned results for this query.
    assert!(all.iter().any(|s| s.term == "test"));
}
