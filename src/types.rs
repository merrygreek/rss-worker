use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeedItem {
    pub title: String,
    pub link: String,
    pub pub_date: String,
    pub description: String,
    pub guid: String,
}

/// Write-only — never read back from KV, so no Deserialize.
#[derive(Debug, Serialize)]
pub struct StoredFeed<'a> {
    pub url: &'a str,
    pub fetched: String, // ISO 8601 from js_sys::Date
    pub items: &'a [FeedItem],
}
