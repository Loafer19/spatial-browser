// Omnibox suggestions from visit history and typed omnibox input.
//
// Ranking: entries whose host (without `www.`) or scheme-less URL starts
// with the query come first, then entries that merely contain it (URL,
// title or typed text). Within a tier: more visits first, visits before
// typed text, then the
// most recent. Each URL / typed text appears once.

use super::history::HistoryEntry;
use serde::Serialize;
use std::collections::HashMap;

/// Suggestions shown under the omnibox.
pub const MAX_SUGGESTIONS: usize = 8;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Suggestion {
    /// What submitting the row sends to `go()`: a URL for visits, the raw
    /// text for typed entries.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// From typed_history (raw input) rather than a visit.
    pub typed: bool,
}

/// `https://www.a.test/x` → `a.test/x`.
fn bare(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.strip_prefix("www.").unwrap_or(rest)
}

struct Candidate {
    text: String,
    title: Option<String>,
    typed: bool,
    tier: u8,
    visits: usize,
    /// Lower = more recent.
    age_rank: usize,
}

fn tier(haystack_bare: &str, extra: &str, q: &str) -> Option<u8> {
    let bare = haystack_bare.to_lowercase();
    if bare.starts_with(q) {
        Some(0)
    } else if bare.contains(q) || extra.to_lowercase().contains(q) {
        Some(1)
    } else {
        None
    }
}

pub fn suggest(history: &[HistoryEntry], typed: &[String], query: &str) -> Vec<Suggestion> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut by_text: HashMap<String, Candidate> = HashMap::new();
    for (age_rank, entry) in history.iter().enumerate() {
        if let Some(c) = by_text.get_mut(&entry.url) {
            c.visits += 1;
            continue;
        }
        let title = entry.title.as_deref().unwrap_or("");
        let Some(tier) = tier(bare(&entry.url), title, &q) else {
            continue;
        };
        by_text.insert(
            entry.url.clone(),
            Candidate {
                text: entry.url.clone(),
                title: entry.title.clone().filter(|t| !t.is_empty()),
                typed: false,
                tier,
                visits: 1,
                age_rank,
            },
        );
    }
    for (age_rank, raw) in typed.iter().enumerate() {
        if by_text.contains_key(raw) {
            continue;
        }
        let Some(tier) = tier(bare(raw), "", &q) else {
            continue;
        };
        by_text.insert(
            raw.clone(),
            Candidate {
                text: raw.clone(),
                title: None,
                typed: true,
                tier,
                visits: 1,
                age_rank,
            },
        );
    }
    let mut all: Vec<Candidate> = by_text.into_values().collect();
    all.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then(b.visits.cmp(&a.visits))
            .then(a.typed.cmp(&b.typed))
            .then(a.age_rank.cmp(&b.age_rank))
            .then(a.text.cmp(&b.text))
    });
    all.into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|c| Suggestion {
            text: c.text,
            title: c.title,
            typed: c.typed,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(url: &str, title: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            url: url.into(),
            visited_at: 0,
            title: title.map(Into::into),
        }
    }

    fn texts(s: &[Suggestion]) -> Vec<&str> {
        s.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn host_prefix_ranks_before_substring() {
        let history = vec![
            visit("https://docs.rs/github", None),
            visit("https://www.github.com/", Some("GitHub")),
            visit("https://example.test/", Some("all about GitHub")),
        ];
        let s = suggest(&history, &[], "git");
        assert_eq!(texts(&s)[0], "https://www.github.com/");
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn dedupes_and_prefers_frequent_then_recent() {
        let history = vec![
            visit("https://a.test/one", None),
            visit("https://a.test/two", None),
            visit("https://a.test/two", None),
        ];
        let s = suggest(
            &history,
            &["a.test/one".into(), "https://a.test/two".into()],
            "a.t",
        );
        assert_eq!(
            texts(&s),
            vec!["https://a.test/two", "https://a.test/one", "a.test/one"]
        );
        assert!(s[2].typed);
    }

    #[test]
    fn empty_query_and_limit() {
        let history: Vec<_> = (0..50)
            .map(|i| visit(&format!("https://s{i}.test/"), None))
            .collect();
        assert!(suggest(&history, &[], "  ").is_empty());
        assert_eq!(suggest(&history, &[], "s").len(), MAX_SUGGESTIONS);
        // Most recent first within a tier.
        assert_eq!(suggest(&history, &[], "s")[0].text, "https://s0.test/");
    }

    #[test]
    fn case_insensitive_title_match() {
        let history = vec![visit("https://x.test/", Some("Rust Book"))];
        assert_eq!(suggest(&history, &[], "BOOK").len(), 1);
    }
}
