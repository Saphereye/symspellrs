use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Token};

/// Macro input representation:
/// include_dictionary!("path/to/file.txt", max_distance = 2, lowercase = true, has_freq = false, precompute = true, max_deletes = 100000)
struct IncludeDictionaryArgs {
    path: LitStr,
    assignments: Vec<(Ident, Expr)>,
}

impl Parse for IncludeDictionaryArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Parse first argument: a string literal path
        let path: LitStr = input.parse()?;

        let mut assignments = Vec::new();

        // Parse optional comma separated assignments
        while input.parse::<Token![,]>().is_ok() {
            // If there's nothing left after comma, break
            if input.is_empty() {
                break;
            }

            // Expect identifier
            let ident: Ident = input.parse()?;
            // Expect '='
            let _eq: Token![=] = input.parse()?;
            // Parse an expression as the value
            let value: Expr = input.parse()?;

            assignments.push((ident, value));
        }

        Ok(IncludeDictionaryArgs { path, assignments })
    }
}

/// include_dictionary!("path/to/file.txt", max_distance = 2, lowercase = true, has_freq = false, precompute = true, max_deletes = 100000)
/// This proc-macro reads the dictionary file at compile time. By default it precomputes
/// the deletion-index and emits two PHF maps:
///  - DICT_PHF: ::phf::Map<&'static str, usize> (word -> freq)
///  - DELETES_PHF: ::phf::Map<&'static str, &'static [&'static str]> (deletion -> [words])
///
/// If `precompute = false` the macro will only emit DICT_PHF and will construct a
/// runtime `SymSpell` by loading the PHF entries into `SymSpell::load_iter(...)`.
///
/// There is a guard `max_deletes` that prevents emitting enormous deletion indexes; if the
/// estimated total number of deletion entries exceeds `max_deletes` the macro will abort
/// with a helpful message (suggest increasing `max_deletes` or setting `precompute = false`).
#[proc_macro]
pub fn include_dictionary(input: TokenStream) -> TokenStream {
    // Parse macro arguments
    let args = syn::parse_macro_input!(input as IncludeDictionaryArgs);

    // Defaults
    let mut max_distance: u8 = 2;
    let mut lowercase: bool = false;
    let mut has_freq: bool = false;
    let mut precompute: bool = true;
    let mut max_deletes: usize = 100_000;

    // Interpret assignments
    for (ident, expr) in args.assignments.iter() {
        let name = ident.to_string();
        match name.as_str() {
            "max_distance" => match expr {
                Expr::Lit(el) => match &el.lit {
                    syn::Lit::Int(li) => {
                        max_distance = li
                            .base10_parse::<u8>()
                            .expect("max_distance must be a u8 integer literal");
                    }
                    _ => panic!("max_distance must be an integer literal"),
                },
                _ => panic!("max_distance must be an integer literal expression"),
            },
            "lowercase" => match expr {
                Expr::Lit(el) => match &el.lit {
                    syn::Lit::Bool(lb) => {
                        lowercase = lb.value;
                    }
                    _ => panic!("lowercase must be a boolean literal"),
                },
                _ => panic!("lowercase must be a boolean literal expression"),
            },
            "has_freq" => match expr {
                Expr::Lit(el) => match &el.lit {
                    syn::Lit::Bool(lb) => {
                        has_freq = lb.value;
                    }
                    _ => panic!("has_freq must be a boolean literal"),
                },
                _ => panic!("has_freq must be a boolean literal expression"),
            },
            "precompute" => match expr {
                Expr::Lit(el) => match &el.lit {
                    syn::Lit::Bool(lb) => {
                        precompute = lb.value;
                    }
                    _ => panic!("precompute must be a boolean literal"),
                },
                _ => panic!("precompute must be a boolean literal expression"),
            },
            "max_deletes" => match expr {
                Expr::Lit(el) => match &el.lit {
                    syn::Lit::Int(li) => {
                        max_deletes = li
                            .base10_parse::<usize>()
                            .expect("max_deletes must be a usize integer literal");
                    }
                    _ => panic!("max_deletes must be an integer literal"),
                },
                _ => panic!("max_deletes must be an integer literal expression"),
            },
            _ => panic!("Unknown argument to include_dictionary: {}", name),
        }
    }

    // Resolve the dictionary file path relative to the crate using the macro.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable not set");
    let relative_path = args.path.value();
    let file_path = Path::new(&manifest_dir).join(relative_path);

    // Read the dictionary file at compile time
    let file = File::open(&file_path).unwrap_or_else(|e| {
        panic!(
            "include_dictionary!: failed to open dictionary file '{}': {}",
            file_path.display(),
            e
        )
    });

    let reader = io::BufReader::new(file);

    // Build dict: word -> freq (BTreeMap for deterministic order)
    let mut dict: BTreeMap<String, usize> = BTreeMap::new();

    for (lineno, line_res) in reader.lines().enumerate() {
        let line = match line_res {
            Ok(l) => l,
            Err(e) => {
                panic!(
                    "include_dictionary!: error reading line {} of {}: {}",
                    lineno + 1,
                    file_path.display(),
                    e
                );
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Determine word and frequency
        let (word_raw, freq) = if has_freq {
            let mut parts = trimmed.split_whitespace();
            let w = parts.next().expect("expected word");
            let freq_str = parts.next().expect("expected frequency");
            let f = freq_str.parse::<usize>().unwrap_or_else(|_| {
                panic!(
                    "include_dictionary!: invalid frequency on line {}: {}",
                    lineno + 1,
                    trimmed
                )
            });
            (w.to_string(), f)
        } else {
            (trimmed.to_string(), 1usize)
        };

        let word = if lowercase {
            word_raw.to_lowercase()
        } else {
            word_raw
        };

        // Insert into dict (sum frequencies for duplicates)
        *dict.entry(word.clone()).or_insert(0) += freq;
    }

    if precompute {
        // Precompute deletion variants for each word and populate deletes_map.
        // Use the same deletion generation rules as SymSpell implementation.
        let mut deletes_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut total_deletes: usize = 0;

        for (word, _freq) in dict.iter() {
            // generate deletions up to max_distance (BFS-like)
            let mut queue: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            queue.insert(word.clone());
            let mut generated: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();

            for _d in 0..max_distance {
                let mut next: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for s in queue.iter() {
                    if s.is_empty() {
                        continue;
                    }
                    for i in 0..s.len() {
                        let mut t = s.clone();
                        t.remove(i);
                        if generated.insert(t.clone()) {
                            next.insert(t);
                        }
                    }
                }
                if next.is_empty() {
                    break;
                }
                for d in next.iter() {
                    deletes_map.entry(d.clone()).or_default().push(word.clone());
                }
                total_deletes += next.len();
                queue = next;
            }
        }

        // Guard against creating very large compiled maps
        if total_deletes > max_deletes {
            panic!("include_dictionary!: precomputing deletion-index would generate {} deletion entries which exceeds max_deletes = {}. Consider setting `precompute = false` or increasing `max_deletes`.", total_deletes, max_deletes);
        }

        // Now produce PHF entries. Keep deterministic order (BTreeMap iteration order).
        let mut dict_entries_tokens = Vec::new();
        for (k, v) in dict.iter() {
            let key = syn::LitStr::new(k, Span::call_site());
            let val = syn::LitInt::new(&v.to_string(), Span::call_site());
            dict_entries_tokens.push((key, val));
        }

        // For deletes_map produce entries where value is a slice literal: &["w1", "w2"]
        let mut deletes_entries_tokens: Vec<(syn::LitStr, Vec<syn::LitStr>)> = Vec::new();
        for (del, words) in deletes_map.iter() {
            let del_lit = syn::LitStr::new(del, Span::call_site());
            let mut word_lits = Vec::new();
            for w in words.iter() {
                word_lits.push(syn::LitStr::new(w, Span::call_site()));
            }
            deletes_entries_tokens.push((del_lit, word_lits));
        }

        // max_distance literal
        let max_distance_lit = syn::LitInt::new(&max_distance.to_string(), Span::call_site());

        // Build quoted entries for dict and deletes
        let dict_quote_iter = dict_entries_tokens.iter().map(|(k, v)| {
            quote! {
                #k => #v
            }
        });

        let deletes_quote_iter = deletes_entries_tokens.iter().map(|(del, word_lits)| {
            // produce: "del" => &["w1", "w2"]
            let wl = word_lits.iter();
            quote! {
                #del => &[#( #wl ),*]
            }
        });

        // Emit expansion: two PHF maps and construct EmbeddedSymSpell from them.
        let expanded = quote! {
            {
                static DICT_PHF: ::phf::Map<&'static str, usize> = ::phf::phf_map! {
                    #(#dict_quote_iter, )*
                };

                static DELETES_PHF: ::phf::Map<&'static str, &'static [&'static str]> = ::phf::phf_map! {
                    #(#deletes_quote_iter, )*
                };

                // Construct and return an EmbeddedSymSpell referencing the statics
                ::symspellrs::EmbeddedSymSpell::from_phf(#max_distance_lit, &DICT_PHF, &DELETES_PHF)
            }
        };

        TokenStream::from(expanded)
    } else {
        // When precompute is false, emit only DICT_PHF and construct a runtime SymSpell
        let mut dict_entries_tokens = Vec::new();
        for (k, v) in dict.iter() {
            let key = syn::LitStr::new(k, Span::call_site());
            let val = syn::LitInt::new(&v.to_string(), Span::call_site());
            dict_entries_tokens.push((key, val));
        }

        let dict_quote_iter = dict_entries_tokens.iter().map(|(k, v)| {
            quote! {
                #k => #v
            }
        });

        let max_distance_lit = syn::LitInt::new(&max_distance.to_string(), Span::call_site());

        let expanded = quote! {
            {
                static DICT_PHF: ::phf::Map<&'static str, usize> = ::phf::phf_map! {
                    #(#dict_quote_iter, )*
                };

                // Build SymSpell at runtime by loading PHF entries
                let mut sym = ::symspellrs::SymSpell::new(#max_distance_lit);
                sym.load_iter(DICT_PHF.entries().map(|(k, v)| (k.to_string(), *v)));
                sym
            }
        };

        TokenStream::from(expanded)
    }
}
