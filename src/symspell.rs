/*!
symspell module

This module provides a SymSpell implementation suitable for use by the
crate's public API. It implements:

- `Suggestion` struct for suggestion results
- `SymSpell` struct which stores a dictionary and a deletion index
- `generate_deletes` to produce deletion variants for SymSpell indexing
- `damerau_levenshtein` to compute edit distances with transposition

How to populate a SymSpell dictionary
- Compile-time: use the `include_dictionary!` proc-macro (provided by the
  `symspellrs_macros` crate). The macro reads a dictionary file at compile
  time and emits a `phf` map and code that constructs a ready `SymSpell`.
- Runtime: use `SymSpell::from_iter` or `SymSpell::load_iter` to build from
  an iterator of `(String, usize)` pairs.

Note: the previous `embedded_dictionary` feature / build-script approach was
removed in favor of the compile-time macro above (which embeds a PHF map in
the expansion) or runtime construction using `from_iter`.
*/

use std::collections::{BTreeSet, HashMap, HashSet};

// Compile-time embedding is now provided by the `include_dictionary!` proc-macro
// (in the `symspellrs_macros` crate) which emits a `phf::Map` in the macro expansion.
// The prior `embedded_dictionary` build-script approach has been removed.

/// A candidate suggestion returned by `lookup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub term: String,
    /// Frequency of the candidate in the underlying dictionary.
    pub frequency: usize,
    /// Edit distance from the queried term to the candidate.
    pub distance: u8,
}

/// Controls which suggestions are returned by lookup functions.
///
/// - `Top`: return a single best suggestion (closest distance, then highest frequency)
/// - `Closest`: return all suggestions with the minimal edit distance (sorted by frequency)
/// - `All`: return all suggestions within max_distance (sorted by distance then frequency)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Top,
    Closest,
    All,
}

/// SymSpell core structure.
///
/// It stores:
/// - `dictionary`: a map from word -> frequency
/// - `deletes`: a map from deletion-variant -> set of original words that produce this deletion
///
/// This follows the classical SymSpell approach where the deletion index maps
/// from generated deletions (strings with one or more characters removed) to
/// the possible original words. At lookup time, the algorithm enumerates deletions
/// of the misspelled term and finds candidate words quickly.
pub struct SymSpell {
    max_distance: u8,
    dictionary: HashMap<String, usize>,
    deletes: HashMap<String, HashSet<String>>,
}

impl SymSpell {
    /// Create an empty `SymSpell` with a configured `max_distance`.
    pub fn new(max_distance: u8) -> Self {
        Self {
            max_distance,
            dictionary: HashMap::new(),
            deletes: HashMap::new(),
        }
    }

    /// Build a `SymSpell` instance from an iterator of `(word, frequency)`.
    /// Frequencies should be >= 0; higher means more common.
    pub fn from_iter<I, S>(max_distance: u8, iter: I) -> Self
    where
        I: IntoIterator<Item = (S, usize)>,
        S: Into<String>,
    {
        let mut sym = SymSpell::new(max_distance);
        sym.load_iter(iter);
        sym
    }

    /// Load dictionary entries from an iterator, inserting or updating entries.
    /// Existing entries for the same word will be replaced by the provided frequency.
    pub fn load_iter<I, S>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (S, usize)>,
        S: Into<String>,
    {
        for (word_s, freq) in iter {
            let word = word_s.into();
            if word.is_empty() {
                continue;
            }
            // Insert/replace dictionary frequency
            self.dictionary.insert(word.clone(), freq);
            // Generate deletes and update delete-index
            let dels = generate_deletes(&word, self.max_distance);
            for d in dels {
                self.deletes
                    .entry(d)
                    .or_insert_with(HashSet::new)
                    .insert(word.clone());
            }
        }
    }

    /// Look up suggestions for `term`.
    ///
    /// `max_distance` is capped by the instance `max_distance`.
    /// `verbosity` controls which suggestions are returned:
    /// - `Verbosity::Top` returns a single best suggestion (closest distance, then highest frequency).
    /// - `Verbosity::Closest` returns all suggestions with the minimal edit distance (sorted by frequency desc).
    /// - `Verbosity::All` returns all suggestions with distance <= max_distance, sorted by distance asc then frequency desc.
    pub fn lookup(&self, term: &str, max_distance: u8, verbosity: Verbosity) -> Vec<Suggestion> {
        if term.is_empty() {
            return Vec::new();
        }
        let max_distance = std::cmp::min(max_distance, self.max_distance);

        // Candidate words found from the deletion index
        let mut candidates: HashSet<String> = HashSet::new();
        // Track terms we've already verified
        let mut considered: HashSet<String> = HashSet::new();

        // SymSpell approach: generate deletions from the query term and find mapped words.
        let mut queue: Vec<String> = vec![term.to_string()];
        // To avoid unbounded growth we cap the queue size heuristically:
        // (this keeps queries reasonable; users may tune logic as needed)
        let queue_limit = 10000usize;

        for idx in 0..queue.len() {
            if idx >= queue_limit {
                break;
            }
            // Clone the current element so we don't hold an immutable borrow of `queue`
            // while also mutating it (e.g. `queue.push(...)`). This resolves the borrow
            // checker error by avoiding simultaneous mutable and immutable borrows.
            let current = queue[idx].clone();

            if let Some(set) = self.deletes.get(&current) {
                for w in set {
                    candidates.insert(w.clone());
                }
            }

            // If we can go deeper generate further deletions
            // We generate 1-deletions of `current` and push into queue if not already queued.
            if (current.len() > 1) && (max_distance as usize) > 0 {
                for i in 0..current.len() {
                    let mut s = current.clone();
                    s.remove(i);
                    if !queue.contains(&s) {
                        queue.push(s);
                    }
                }
            }
        }

        // Collect results with computed distances
        let mut results: Vec<Suggestion> = Vec::new();

        // If the exact term exists in the dictionary, include it among candidates (distance 0)
        if let Some(&freq) = self.dictionary.get(term) {
            results.push(Suggestion {
                term: term.to_string(),
                frequency: freq,
                distance: 0,
            });
            // For Top/Closest, an exact match is already optimal; but we'll still run the general selection below.
        }

        for cand in candidates {
            if considered.contains(&cand) {
                continue;
            }
            considered.insert(cand.clone());
            let distance = damerau_levenshtein(term, &cand);
            if distance <= max_distance {
                let freq = *self.dictionary.get(&cand).unwrap_or(&0);
                results.push(Suggestion {
                    term: cand.clone(),
                    frequency: freq,
                    distance,
                });
            }
        }

        if results.is_empty() {
            return Vec::new();
        }

        // Determine minimal distance among results
        let min_distance = results.iter().map(|r| r.distance).min().unwrap_or(u8::MAX);

        match verbosity {
            Verbosity::Top => {
                // Choose suggestions with minimal distance, then pick the one with highest frequency.
                let mut best: Option<Suggestion> = None;
                for r in results.into_iter().filter(|r| r.distance == min_distance) {
                    match &best {
                        None => best = Some(r),
                        Some(b) => {
                            if r.frequency > b.frequency {
                                best = Some(r);
                            }
                        }
                    }
                }
                best.into_iter().collect()
            }
            Verbosity::Closest => {
                // Return all with minimal distance, sorted by frequency desc
                let mut filtered: Vec<Suggestion> = results
                    .into_iter()
                    .filter(|r| r.distance == min_distance)
                    .collect();
                filtered.sort_by(|a, b| b.frequency.cmp(&a.frequency));
                filtered
            }
            Verbosity::All => {
                // Return all within max_distance sorted by distance asc then frequency desc
                results.sort_by(|a, b| {
                    a.distance
                        .cmp(&b.distance)
                        .then_with(|| b.frequency.cmp(&a.frequency))
                });
                results
            }
        }
    }

    /// Small helper to query raw frequency
    pub fn frequency(&self, word: &str) -> Option<usize> {
        self.dictionary.get(word).copied()
    }
}

/// EmbeddedSymSpell: fully precomputed PHF-backed SymSpell.
///
/// This struct is intended for the macro variant that precomputes the deletion-index
/// at compile time and emits two PHF maps:
/// - a dictionary `::phf::Map<&'static str, usize>` mapping words to frequencies
/// - a deletes map `::phf::Map<&'static str, &'static [&'static str]>` mapping each deletion
///   variant to a static slice of original words that produce that deletion
///
/// Advantages:
/// - No runtime cost to build the delete-index; lookups read the precomputed PHF maps.
/// - Very fast lookup because the deletes -> words map is a direct PHF lookup.
///
/// Tradeoffs:
/// - The number of deletion variants may be large; precomputing and embedding them
///   increases generated code size and compile time. The consuming macro should be
///   used with care for very large dictionaries or large `max_distance` values.
pub struct EmbeddedSymSpell {
    /// maximum edit distance the index was built for
    pub max_distance: u8,
    /// dictionary map: word -> frequency
    pub dict: &'static ::phf::Map<&'static str, usize>,
    /// delete-index map: deletion_variant -> slice of originating words
    pub deletes: &'static ::phf::Map<&'static str, &'static [&'static str]>,
}

impl EmbeddedSymSpell {
    /// Construct an `EmbeddedSymSpell` from generated PHF maps.
    ///
    /// Typical usage: the `include_dictionary!` proc-macro when asked to
    /// precompute deletes will emit two statics `DICT_PHF` and `DELETES_PHF`
    /// and then call this constructor in the macro expansion.
    pub fn from_phf(
        max_distance: u8,
        dict: &'static ::phf::Map<&'static str, usize>,
        deletes: &'static ::phf::Map<&'static str, &'static [&'static str]>,
    ) -> Self {
        Self {
            max_distance,
            dict,
            deletes,
        }
    }

    /// Get frequency from the embedded dict
    pub fn frequency(&self, word: &str) -> Option<usize> {
        self.dict.get(word).copied()
    }

    /// Lookup suggestions using the precomputed deletes PHF map.
    ///
    /// Behavior mirrors `SymSpell::lookup`: enumerate deletion-variants of the query
    /// (up to `max_distance`), use the deletes PHF to find candidate original words,
    /// then verify candidates with Damerau-Levenshtein and return suggestions according
    /// to `verbosity`.
    pub fn lookup(&self, term: &str, max_distance: u8, verbosity: Verbosity) -> Vec<Suggestion> {
        if term.is_empty() {
            return Vec::new();
        }
        let max_distance = std::cmp::min(max_distance, self.max_distance);

        // If exact match
        if let Some(&freq) = self.dict.get(term) {
            return vec![Suggestion {
                term: term.to_string(),
                frequency: freq,
                distance: 0,
            }];
        }

        // Candidate words found from the PHF deletion-index
        let mut candidates: HashSet<String> = HashSet::new();
        // Track visited deletion variants to avoid duplicate PHF lookups
        let mut visited_deletions: HashSet<String> = HashSet::new();

        // Generate deletions up to max_distance (BFS by deletion-levels)
        let mut queue: Vec<String> = vec![term.to_string()];
        let queue_limit = 10000usize;

        for idx in 0..queue.len() {
            if idx >= queue_limit {
                break;
            }
            let current = queue[idx].clone();

            // Avoid repeated PHF lookups for the same deletion variant
            if visited_deletions.insert(current.clone()) {
                if let Some(slice) = self.deletes.get(&current as &str) {
                    for &w in *slice {
                        candidates.insert(w.to_string());
                    }
                }
            }

            // Generate next-level deletions (1-deletions of current)
            if (current.len() > 1) && (max_distance as usize) > 0 {
                for i in 0..current.len() {
                    let mut s = current.clone();
                    s.remove(i);
                    if !queue.contains(&s) {
                        queue.push(s);
                    }
                }
            }
        }

        // Compute Damerau-Levenshtein distances for candidates and collect results
        let mut results: Vec<Suggestion> = Vec::new();

        for cand in candidates {
            let distance = damerau_levenshtein(term, &cand);
            if distance <= max_distance {
                let freq = *self.dict.get(&cand as &str).unwrap_or(&0);
                results.push(Suggestion {
                    term: cand.clone(),
                    frequency: freq,
                    distance,
                });
            }
        }

        if results.is_empty() {
            return Vec::new();
        }

        // Determine minimal distance among results
        let min_distance = results.iter().map(|r| r.distance).min().unwrap_or(u8::MAX);

        match verbosity {
            Verbosity::Top => {
                let mut best: Option<Suggestion> = None;
                for r in results.into_iter().filter(|r| r.distance == min_distance) {
                    match &best {
                        None => best = Some(r),
                        Some(b) => {
                            if r.frequency > b.frequency {
                                best = Some(r);
                            }
                        }
                    }
                }
                best.into_iter().collect()
            }
            Verbosity::Closest => {
                let mut filtered: Vec<Suggestion> = results
                    .into_iter()
                    .filter(|r| r.distance == min_distance)
                    .collect();
                filtered.sort_by(|a, b| b.frequency.cmp(&a.frequency));
                filtered
            }
            Verbosity::All => {
                // Return all within max_distance sorted by distance asc then frequency desc
                results.sort_by(|a, b| {
                    a.distance
                        .cmp(&b.distance)
                        .then_with(|| b.frequency.cmp(&a.frequency))
                });
                results
            }
        }
    }

    // Convenience helpers added for easier user-facing API:

    /// Return the single best suggestion (if any) for `term`. This is a shorthand
    /// for `lookup(term, self.max_distance, Verbosity::Top)` returning Option.
    pub fn find_top(&self, term: &str) -> Option<Suggestion> {
        self.lookup(term, self.max_distance, Verbosity::Top)
            .into_iter()
            .next()
    }

    /// Return all suggestions with minimal distance (shorthand for Closest).
    pub fn find_closest(&self, term: &str) -> Vec<Suggestion> {
        self.lookup(term, self.max_distance, Verbosity::Closest)
    }

    /// Return all suggestions within the configured max distance (shorthand for All).
    pub fn find_all(&self, term: &str) -> Vec<Suggestion> {
        self.lookup(term, self.max_distance, Verbosity::All)
    }

    /// Returns true if a word is present in the embedded dictionary.
    pub fn contains(&self, word: &str) -> bool {
        self.dict.contains_key(word)
    }

    /// Return a reference to the underlying PHF dictionary map (word -> frequency).
    pub fn dict_map(&self) -> &'static ::phf::Map<&'static str, usize> {
        self.dict
    }

    /// Return a reference to the underlying PHF deletes map (deletion -> list of words).
    pub fn deletes_map(&self) -> &'static ::phf::Map<&'static str, &'static [&'static str]> {
        self.deletes
    }

    /// Return a list of candidate words that map from the provided deletion variant,
    /// or None if the deletion variant is not present.
    pub fn candidates_for_deletion(&self, deletion: &str) -> Option<&'static [&'static str]> {
        self.deletes.get(deletion).copied()
    }

    /// Convenience: return frequency of a word or 0 if absent.
    pub fn frequency_or_zero(&self, word: &str) -> usize {
        *self.dict.get(word).unwrap_or(&0usize)
    }
}

/// Generate all deletion variants for `word` up to `max_distance`.
///
/// For example, for `word = "hello"` and `max_distance = 2` this will include
/// deletions with 1 and 2 characters removed. The returned set includes the empty
/// string only if deletions produce it (rare for short words).
fn generate_deletes(word: &str, max_distance: u8) -> HashSet<String> {
    let mut deletes: HashSet<String> = HashSet::new();
    let mut queue: BTreeSet<String> = BTreeSet::new();
    queue.insert(word.to_string());

    for _d in 0..max_distance {
        let mut next: BTreeSet<String> = BTreeSet::new();
        for s in &queue {
            if s.len() == 0 {
                continue;
            }
            for i in 0..s.len() {
                let mut t = s.clone();
                t.remove(i);
                if deletes.insert(t.clone()) {
                    next.insert(t);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        queue = next;
    }
    deletes
}

/// Damerau-Levenshtein distance with transposition, returns distance as u8.
///
/// The implementation is a standard dynamic programming approach. It is not
/// optimized for speed but is simple and correct. Distances larger than 255
/// will be capped at 255.
fn damerau_levenshtein(a: &str, b: &str) -> u8 {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let (alen, blen) = (a_chars.len(), b_chars.len());

    if alen == 0 {
        return blen.min(255) as u8;
    }
    if blen == 0 {
        return alen.min(255) as u8;
    }

    let mut dp: Vec<Vec<usize>> = vec![vec![0; blen + 1]; alen + 1];

    for i in 0..=alen {
        dp[i][0] = i;
    }
    for j in 0..=blen {
        dp[0][j] = j;
    }

    for i in 1..=alen {
        for j in 1..=blen {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            dp[i][j] = std::cmp::min(
                std::cmp::min(dp[i - 1][j] + 1, dp[i][j - 1] + 1),
                dp[i - 1][j - 1] + cost,
            );
            // transposition
            if i > 1
                && j > 1
                && a_chars[i - 1] == b_chars[j - 2]
                && a_chars[i - 2] == b_chars[j - 1]
            {
                dp[i][j] = std::cmp::min(dp[i][j], dp[i - 2][j - 2] + 1);
            }
        }
    }

    dp[alen][blen].min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_damerau_basic() {
        assert_eq!(damerau_levenshtein("abc", "abc"), 0);
        assert_eq!(damerau_levenshtein("abc", "ab"), 1);
        assert_eq!(damerau_levenshtein("ab", "ba"), 1); // transposition
    }

    #[test]
    fn test_symspell_lookup() {
        let entries = vec![
            ("hello".to_string(), 100usize),
            ("hell".to_string(), 50usize),
            ("help".to_string(), 10usize),
            ("world".to_string(), 200usize),
        ];
        let sym = SymSpell::from_iter(2, entries);
        let suggestions = sym.lookup("helo", 2, Verbosity::Closest);
        // Expect "hello" to be a top suggestion
        assert!(suggestions.iter().any(|s| s.term == "hello"));
    }
}
