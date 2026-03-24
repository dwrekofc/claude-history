use crate::history::Conversation;
use chrono::{DateTime, Duration, Local};
use rayon::prelude::*;

/// Precomputed search data for a conversation
#[derive(Clone)]
pub struct SearchableConversation {
    /// Lowercased full text for searching
    pub text_lower: String,
    /// Original conversation index
    pub index: usize,
}

/// Check if a query looks like a UUID (e.g., e7d318b1-4274-4ee2-a341-e94893b5df49)
pub fn is_uuid(query: &str) -> bool {
    let q = query.trim();
    if q.len() != 36 {
        return false;
    }
    let parts: Vec<&str> = q.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected_lens.iter())
        .all(|(part, &len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Check if a character is CJK punctuation or symbol.
/// Only includes actual punctuation — excludes iteration marks (々), shorthand (〆),
/// and CJK zero (〇) which appear inside words/names.
fn is_cjk_punctuation(c: char) -> bool {
    matches!(
        c,
        '\u{3000}' | // ideographic space
        '\u{3001}' | // ideographic comma
        '\u{3002}' | // ideographic full stop
        '\u{3008}' | // left angle bracket
        '\u{3009}' | // right angle bracket
        '\u{300A}' | // left double angle bracket
        '\u{300B}' | // right double angle bracket
        '\u{300C}' | // left corner bracket
        '\u{300D}' | // right corner bracket
        '\u{300E}' | // left white corner bracket
        '\u{300F}' | // right white corner bracket
        '\u{3010}' | // left black lenticular bracket
        '\u{3011}' | // right black lenticular bracket
        '\u{3014}' | // left tortoise shell bracket
        '\u{3015}' | // right tortoise shell bracket
        '\u{3016}' | // left white lenticular bracket
        '\u{3017}' | // right white lenticular bracket
        '\u{FF01}' | // fullwidth exclamation
        '\u{FF08}' | // fullwidth left parenthesis
        '\u{FF09}' | // fullwidth right parenthesis
        '\u{FF0C}' | // fullwidth comma
        '\u{FF1A}' | // fullwidth colon
        '\u{FF1B}' | // fullwidth semicolon
        '\u{FF1F}' | // fullwidth question mark
        '\u{201C}' | // left double quotation mark
        '\u{201D}' | // right double quotation mark
        '\u{2018}' | // left single quotation mark
        '\u{2019}' | // right single quotation mark
        '\u{2014}' | // em dash
        '\u{2026}' | // horizontal ellipsis
        '\u{00B7}' // middle dot
    )
}

/// Normalize text for search: lowercase, replace separators with spaces,
/// and handle CJK punctuation as word boundaries
pub fn normalize_for_search(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '_' || ch == '-' || ch == '/' || is_cjk_punctuation(ch) {
            out.push(' ');
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Check if a character is a word separator for search purposes
pub fn is_word_separator(c: char) -> bool {
    c.is_whitespace() || c == '_' || c == '-' || c == '/' || is_cjk_punctuation(c)
}

/// Precompute lowercased search text for all conversations
pub fn precompute_search_text(conversations: &[Conversation]) -> Vec<SearchableConversation> {
    conversations
        .par_iter()
        .enumerate()
        .map(|(idx, conv)| {
            let mut text = conv.full_text.clone();
            if let Some(ref name) = conv.project_name {
                text.push(' ');
                text.push_str(name);
            }
            SearchableConversation {
                text_lower: normalize_for_search(&text),
                index: idx,
            }
        })
        .collect()
}

/// Filter and score conversations based on query
/// Returns indices into the original conversations vec, sorted by score descending
pub fn search(
    conversations: &[Conversation],
    searchable: &[SearchableConversation],
    query: &str,
    now: DateTime<Local>,
) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        // Return all indices sorted by timestamp (already sorted in history.rs)
        return (0..conversations.len()).collect();
    }

    let query_lower = normalize_for_search(query);
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    if query_words.is_empty() {
        return (0..conversations.len()).collect();
    }

    // Score all conversations in parallel
    let mut scored: Vec<(usize, f64, DateTime<Local>)> = searchable
        .par_iter()
        .filter_map(|s| {
            let score = score_text(
                &s.text_lower,
                &query_words,
                conversations[s.index].timestamp,
                now,
            );
            if score > 0.0 {
                Some((s.index, score, conversations[s.index].timestamp))
            } else {
                None
            }
        })
        .collect();

    // Sort by score descending, then by timestamp descending for stability
    scored.sort_unstable_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.2.cmp(&a.2))
    });

    scored.into_iter().map(|(idx, _, _)| idx).collect()
}

/// Score a conversation based on word prefix matching and recency.
/// Each query word must be a prefix of at least one word in the text (AND logic).
/// Falls back to substring matching when prefix matching fails (e.g. for CJK text).
///
/// Uses `find()` + word boundary check instead of `split_whitespace()` iteration
/// so that search time scales with the number of matches, not the number of words
/// in the text. This keeps interactive search fast even on multi-MB conversations.
fn score_text(
    text_lower: &str,
    query_words: &[&str],
    timestamp: DateTime<Local>,
    now: DateTime<Local>,
) -> f64 {
    if query_words.is_empty() {
        return 0.0;
    }

    // Fast rejection: if a query word isn't present as substring, skip expensive checking
    for &qw in query_words {
        if !text_lower.contains(qw) {
            return 0.0;
        }
    }

    // For each query word, find it at a word boundary (prefix match).
    // Uses find() which is backed by SIMD-accelerated memchr in Rust's std.
    let mut all_prefix_matched = true;

    for &qw in query_words {
        let mut start = 0;
        let mut found = false;

        while let Some(pos) = text_lower[start..].find(qw) {
            let actual_pos = start + pos;

            // Check word boundary: start of string or preceded by whitespace.
            // Uses char-level check to handle Unicode whitespace correctly.
            let at_boundary = actual_pos == 0
                || text_lower[..actual_pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_whitespace());

            if at_boundary {
                found = true;
                break;
            }
            // Advance past the matched substring (actual_pos + qw.len() is always
            // a valid char boundary since find() matched valid UTF-8 at actual_pos)
            start = actual_pos + qw.len().max(1);
        }

        if !found {
            all_prefix_matched = false;
            break;
        }
    }

    if all_prefix_matched {
        return (query_words.len() as f64) * recency_multiplier(timestamp, now);
    }

    // Fallback: substring matching for CJK text.
    // Only apply when at least one query word contains CJK ideographs, since CJK text
    // lacks whitespace word boundaries and substring matching is the appropriate strategy.
    // Without this guard, Latin queries like "ime" would incorrectly match "runtime".
    let has_cjk = query_words
        .iter()
        .any(|w| w.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)));
    if has_cjk {
        return (query_words.len() as f64) * 0.5 * recency_multiplier(timestamp, now);
    }

    0.0
}

/// Calculate recency multiplier based on age
fn recency_multiplier(timestamp: DateTime<Local>, now: DateTime<Local>) -> f64 {
    let age = now.signed_duration_since(timestamp);

    // Handle future timestamps (shouldn't happen, but be safe)
    if age < Duration::zero() {
        return 3.0;
    }

    if age < Duration::days(1) {
        3.0
    } else if age < Duration::days(7) {
        2.0
    } else if age < Duration::days(30) {
        1.5
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Conversation;
    use std::path::PathBuf;

    fn make_conv(text: &str, timestamp: DateTime<Local>) -> Conversation {
        Conversation {
            path: PathBuf::new(),
            index: 0,
            timestamp,
            preview: text.to_string(),
            full_text: text.to_string(),
            project_name: None,
            project_path: None,
            cwd: None,
            message_count: 1,
            parse_errors: vec![],
            summary: None,
            custom_title: None,
            model: None,
            total_tokens: 0,
            duration_minutes: None,
        }
    }

    #[test]
    fn search_matches_underscore_separated() {
        let now = Local::now();
        let convs = vec![make_conv("HARDENED_RUNTIME config", now)];
        let searchable = precompute_search_text(&convs);
        let results = search(&convs, &searchable, "harden runtime", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_matches_different_case() {
        let now = Local::now();
        let convs = vec![make_conv("Hardened Runtime enabled", now)];
        let searchable = precompute_search_text(&convs);
        let results = search(&convs, &searchable, "harden runtime", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_prefix_matches_words() {
        let now = Local::now();
        let convs = vec![make_conv("hardened security", now)];
        let searchable = precompute_search_text(&convs);
        let results = search(&convs, &searchable, "harden", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_requires_all_words() {
        let now = Local::now();
        let convs = vec![make_conv("hardened security", now)];
        let searchable = precompute_search_text(&convs);
        let results = search(&convs, &searchable, "harden runtime", now);
        assert_eq!(results.len(), 0); // "runtime" not present
    }

    #[test]
    fn search_with_underscore_in_query() {
        let now = Local::now();
        let convs = vec![make_conv("hardened runtime enabled", now)];
        let searchable = precompute_search_text(&convs);
        // Query with underscore should still match space-separated text
        let results = search(&convs, &searchable, "hardened_runtime", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn recency_today_gets_highest_multiplier() {
        let now = Local::now();
        let timestamp = now - Duration::hours(1);
        assert_eq!(recency_multiplier(timestamp, now), 3.0);
    }

    #[test]
    fn recency_this_week_gets_medium_multiplier() {
        let now = Local::now();
        let timestamp = now - Duration::days(3);
        assert_eq!(recency_multiplier(timestamp, now), 2.0);
    }

    #[test]
    fn recency_this_month_gets_low_multiplier() {
        let now = Local::now();
        let timestamp = now - Duration::days(15);
        assert_eq!(recency_multiplier(timestamp, now), 1.5);
    }

    #[test]
    fn recency_older_gets_base_multiplier() {
        let now = Local::now();
        let timestamp = now - Duration::days(60);
        assert_eq!(recency_multiplier(timestamp, now), 1.0);
    }

    #[test]
    fn future_timestamp_gets_highest_multiplier() {
        let now = Local::now();
        let timestamp = now + Duration::hours(1);
        assert_eq!(recency_multiplier(timestamp, now), 3.0);
    }

    fn make_conv_with_project(
        text: &str,
        project: &str,
        timestamp: DateTime<Local>,
    ) -> Conversation {
        let mut conv = make_conv(text, timestamp);
        conv.project_name = Some(project.to_string());
        conv
    }

    #[test]
    fn search_matches_project_name() {
        let now = Local::now();
        let convs = vec![make_conv_with_project(
            "some conversation",
            "workmux/main-worktree-fix",
            now,
        )];
        let searchable = precompute_search_text(&convs);

        // Match worktree name
        let results = search(&convs, &searchable, "main-worktree-fix", now);
        assert_eq!(results.len(), 1);

        // Match with project prefix
        let results = search(&convs, &searchable, "workmux", now);
        assert_eq!(results.len(), 1);

        // Match project/worktree combined
        let results = search(&convs, &searchable, "workmux main worktree", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_matches_hyphenated_words() {
        let now = Local::now();
        let convs = vec![make_conv("main-worktree-fix discussion", now)];
        let searchable = precompute_search_text(&convs);
        let results = search(&convs, &searchable, "worktree fix", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn is_uuid_valid() {
        assert!(is_uuid("e7d318b1-4274-4ee2-a341-e94893b5df49"));
        assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(is_uuid("ABCDEF01-2345-6789-abcd-ef0123456789"));
    }

    #[test]
    fn is_uuid_invalid() {
        assert!(!is_uuid(""));
        assert!(!is_uuid("not-a-uuid"));
        assert!(!is_uuid("e7d318b1-4274-4ee2-a341")); // too short
        assert!(!is_uuid("e7d318b1-4274-4ee2-a341-e94893b5df49x")); // too long
        assert!(!is_uuid("e7d318b14274-4ee2-a341-e94893b5df49-")); // wrong grouping
        assert!(!is_uuid("g7d318b1-4274-4ee2-a341-e94893b5df49")); // non-hex char
    }

    #[test]
    fn is_uuid_with_whitespace() {
        assert!(is_uuid("  e7d318b1-4274-4ee2-a341-e94893b5df49  "));
    }

    #[test]
    fn search_matches_chinese_text_with_punctuation() {
        let now = Local::now();
        let convs = vec![make_conv(
            "\u{9000}\u{51FA}\u{7801} 143 \u{5C31}\u{662F} SIGTERM\u{FF0C}\u{5C5E}\u{4E8E}\u{9884}\u{671F}\u{884C}\u{4E3A}\u{3002}\u{5F53}\u{524D}\u{65B0}\u{8FDB}",
            now,
        )];
        let searchable = precompute_search_text(&convs);

        // Should match Chinese text across CJK punctuation boundaries
        let results = search(&convs, &searchable, "\u{5C5E}\u{4E8E}\u{9884}\u{671F}", now);
        assert_eq!(results.len(), 1);

        // Should match text before punctuation
        let results = search(&convs, &searchable, "\u{9000}\u{51FA}\u{7801}", now);
        assert_eq!(results.len(), 1);

        // Should match mixed Chinese and English
        let results = search(&convs, &searchable, "SIGTERM \u{9884}\u{671F}", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_matches_chinese_substring_within_token() {
        let now = Local::now();
        let convs = vec![make_conv(
            "\u{8FD9}\u{662F}\u{4E00}\u{4E2A}\u{6D4B}\u{8BD5}\u{4F1A}\u{8BDD}\u{5185}\u{5BB9}",
            now,
        )];
        let searchable = precompute_search_text(&convs);

        // Should find substring even without word boundaries
        let results = search(&convs, &searchable, "\u{6D4B}\u{8BD5}\u{4F1A}\u{8BDD}", now);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn cjk_punctuation_treated_as_separator() {
        assert_eq!(
            normalize_for_search("SIGTERM\u{FF0C}\u{5C5E}\u{4E8E}\u{9884}\u{671F}"),
            "sigterm \u{5C5E}\u{4E8E}\u{9884}\u{671F}"
        );
        assert_eq!(
            normalize_for_search("\u{884C}\u{4E3A}\u{3002}\u{5F53}\u{524D}"),
            "\u{884C}\u{4E3A} \u{5F53}\u{524D}"
        );
    }
}
