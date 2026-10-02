//! Small text helpers shared by agents and tools.

use sha2::{Digest, Sha256};
use url::Url;

/// Truncates to at most `max_chars` characters on a char boundary.
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Lowercases and collapses all whitespace runs into single spaces.
pub fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether `excerpt` appears verbatim (modulo case and whitespace) in `content`.
pub fn excerpt_in_content(excerpt: &str, content: &str) -> bool {
    let needle = normalize_ws(excerpt);
    !needle.is_empty() && normalize_ws(content).contains(&needle)
}

pub fn content_hash(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

/// Canonical form of a URL used for source deduplication: lowercase host,
/// no `www.`, no fragment, no tracking parameters, no trailing slash.
pub fn normalize_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_fragment(None);
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            let k = k.to_lowercase();
            !(k.starts_with("utm_") || matches!(k.as_str(), "fbclid" | "gclid" | "ref" | "ref_src"))
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    let host = url.host_str()?.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let path = url.path().trim_end_matches('/');
    let mut out = format!("{}://{}{}", url.scheme(), host, path);
    if let Some(q) = url.query() {
        out.push('?');
        out.push_str(q);
    }
    Some(out)
}

/// Host without `www.`, used as a fallback publisher name.
pub fn host_of(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?.to_lowercase();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

/// Returns the excerpt with up to `radius` characters of surrounding source
/// text, falling back to the excerpt alone if it cannot be located exactly.
pub fn context_around(content: &str, excerpt: &str, radius: usize) -> String {
    let (lc, le) = (content.to_lowercase(), excerpt.trim().to_lowercase());
    // Lowercasing can change byte lengths for some scripts; only trust offsets
    // when they line up with the original string.
    if lc.len() == content.len()
        && !le.is_empty()
        && let Some(pos) = lc.find(&le)
    {
        let start = (0..=pos.saturating_sub(radius)).rev().find(|i| content.is_char_boundary(*i)).unwrap_or(0);
        let want_end = (pos + le.len() + radius).min(content.len());
        let end = (want_end..=content.len()).find(|i| content.is_char_boundary(*i)).unwrap_or(content.len());
        return content[start..end].to_string();
    }
    excerpt.trim().to_string()
}

/// Extracts the outermost JSON object/array from text that may be wrapped in
/// prose or markdown code fences.
pub fn extract_json(text: &str) -> Option<&str> {
    let start = text.find(['{', '['])?;
    let open = text.as_bytes()[start];
    let close = if open == b'{' { '}' } else { ']' };
    let end = text.rfind(close)?;
    (end > start).then(|| &text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_urls_for_dedup() {
        let a = normalize_url("https://www.Example.com/post/?utm_source=x&id=7#top").unwrap();
        let b = normalize_url("http://example.com/post?id=7").unwrap();
        assert_eq!(a, "https://example.com/post?id=7");
        // scheme differs so these are intentionally distinct
        assert_eq!(b, "http://example.com/post?id=7");
        assert_eq!(normalize_url("https://example.com/a/").unwrap(), normalize_url("https://EXAMPLE.com/a").unwrap());
        assert!(normalize_url("ftp://example.com").is_none());
        assert!(normalize_url("not a url").is_none());
    }

    #[test]
    fn excerpt_matching_ignores_case_and_whitespace() {
        let content = "The study\n found a 12%   reduction in time.";
        assert!(excerpt_in_content("found a 12% reduction", content));
        assert!(!excerpt_in_content("found a 40% reduction", content));
        assert!(!excerpt_in_content("   ", content));
    }

    #[test]
    fn extracts_json_from_fenced_output() {
        let raw = "Here you go:\n```json\n{\"a\": [1,2]}\n```\nthanks";
        assert_eq!(extract_json(raw), Some("{\"a\": [1,2]}"));
        assert_eq!(extract_json("no json"), None);
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate_chars("héllo", 2), "hé");
        assert_eq!(truncate_chars("hi", 10), "hi");
    }
}
