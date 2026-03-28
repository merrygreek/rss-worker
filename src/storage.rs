use fnv::FnvHasher;
use std::hash::{Hash, Hasher};

use crate::types::FeedItem;

/// Deterministic KV key for a feed URL.
/// Format: "feed:{16 lowercase hex chars}" (21 chars total).
/// Uses FNV-1a hash (saves ~50-80KB vs SHA-256 on wasm32).
pub fn url_key(url: &str) -> String {
    let mut h = FnvHasher::default();
    url.hash(&mut h);
    format!("feed:{:016x}", h.finish())
}

/// Write feed items to KV with a 24-hour TTL.
/// Only compiled for wasm32 — uses worker::Env which is WASM-only.
#[cfg(target_arch = "wasm32")]
pub async fn store_feed(
    env: &worker::Env,
    url: &str,
    items: &[FeedItem],
) -> Result<(), worker::Error> {
    use crate::types::StoredFeed;

    let kv = env.kv("RSS_STORE")?;
    let value = serde_json::to_string(&StoredFeed {
        url,
        fetched: js_sys::Date::new_0()
            .to_iso_string()
            .as_string()
            .unwrap_or_default(),
        items,
    })
    .map_err(|e| worker::Error::RustError(e.to_string()))?;

    kv.put(&url_key(url), value)?
        .expiration_ttl(86400) // 24h
        .execute()
        .await
}

#[cfg(test)]
mod tests {
    use super::url_key;

    #[test]
    fn test_url_key_deterministic() {
        let url = "https://example.com/feed.xml";
        assert_eq!(url_key(url), url_key(url));
    }

    #[test]
    fn test_url_key_different_urls_differ() {
        let a = url_key("https://example.com/feed.xml");
        let b = url_key("https://other.org/rss");
        assert_ne!(a, b, "different URLs must produce different keys");
    }

    #[test]
    fn test_url_key_format() {
        let key = url_key("https://example.com");
        assert!(key.starts_with("feed:"), "key must start with 'feed:'");
        assert_eq!(key.len(), 21, "key must be 21 chars: 'feed:' + 16 hex");
        let hex_part = &key[5..];
        assert!(
            hex_part.chars().all(|c| c.is_ascii_hexdigit()),
            "hex part must be lowercase hex"
        );
    }
}
