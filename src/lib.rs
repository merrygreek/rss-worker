mod parser;
mod storage;
mod types;

// All worker-specific code is wasm32-only.
// cargo test runs on native (x86_64) and only compiles parser/storage pure functions.
#[cfg(target_arch = "wasm32")]
use futures::future::join_all;
#[cfg(target_arch = "wasm32")]
use worker::{event, Env, ScheduledEvent, ScheduleContext};

#[cfg(target_arch = "wasm32")]
use parser::parse_feed;
#[cfg(target_arch = "wasm32")]
use storage::{store_feed, url_key};

/// RSS feed URLs to fetch on each cron run.
/// Hard limit: 45 (CF free tier allows 50 subrequests/invocation; leave 5 slack).
/// KV budget: 45 feeds × 12 runs/day = 540 writes/day (limit: 1000/day).
///
/// Phase 2: load this list from KV key "config:feeds" instead of hardcoding.
#[cfg(target_arch = "wasm32")]
const FEEDS: &[&str] = &[
    // Add your RSS feed URLs here, one per line, max 45 total.
    // "https://example.com/feed.xml",
    // "https://blog.example.org/rss",
];

#[cfg(target_arch = "wasm32")]
async fn fetch_url(url: &str) -> Result<String, String> {
    // MVP: rely on CF Workers 30s platform timeout (no manual timeout).
    // Phase 2: race fetch against gloo-timers::TimeoutFuture for per-feed timeout.
    let req =
        worker::Request::new(url, worker::Method::Get).map_err(|e| e.to_string())?;
    let mut resp = worker::Fetch::Request(req)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.text().await.map_err(|e| e.to_string())
}

/// Write a {error, fetched} JSON record to KV with a 1-hour TTL.
/// Lets consumers distinguish "broken feed" from "never fetched."
#[cfg(target_arch = "wasm32")]
async fn write_error_state(
    kv: &worker::kv::KvStore,
    url: &str,
    error: &str,
    timestamp: &str,
) {
    let v = serde_json::json!({"error": error, "fetched": timestamp}).to_string();
    match kv.put(&url_key(url), v) {
        Ok(b) => {
            if let Err(e) = b.expiration_ttl(3600).execute().await {
                worker::console_error!("write_error_state execute failed for {}: {:?}", url, e);
            }
        }
        Err(e) => {
            worker::console_error!("write_error_state put failed for {}: {:?}", url, e);
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let kv = match env.kv("RSS_STORE") {
        Ok(k) => k,
        Err(e) => {
            worker::console_error!("kv binding error: {:?}", e);
            return;
        }
    };

    let urls: Vec<&str> = FEEDS.iter().take(45).copied().collect();
    let results = join_all(urls.iter().map(|u| fetch_url(u))).await;

    let timestamp = js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default();

    let mut success_count: usize = 0;
    let mut failed_count: usize = 0;

    for (url, result) in urls.iter().zip(results) {
        match result {
            Ok(xml) => match parse_feed(&xml) {
                Ok(items) => {
                    if let Err(e) = store_feed(&env, url, &items).await {
                        worker::console_error!("store failed for {}: {:?}", url, e);
                        failed_count += 1;
                        write_error_state(&kv, url, &e.to_string(), &timestamp).await;
                    } else {
                        success_count += 1;
                    }
                }
                Err(e) => {
                    worker::console_error!("parse failed for {}: {}", url, e);
                    failed_count += 1;
                    write_error_state(&kv, url, &e, &timestamp).await;
                }
            },
            Err(e) => {
                worker::console_error!("fetch failed for {}: {}", url, e);
                failed_count += 1;
                write_error_state(&kv, url, &e, &timestamp).await;
            }
        }
    }

    // Write run metadata so consumers can detect partial runs.
    let meta = serde_json::json!({
        "ts": &timestamp,
        "success": success_count,
        "failed": failed_count,
        "total": urls.len(),
    })
    .to_string();

    // TTL = 4h (2× the cron interval). If the worker stops running, the key
    // expires and consumers can distinguish "stale/stopped" from "never run".
    match kv.put("meta:last-run", meta) {
        Ok(b) => {
            if let Err(e) = b.expiration_ttl(14400).execute().await {
                worker::console_error!("meta:last-run write failed: {:?}", e);
            }
        }
        Err(e) => {
            worker::console_error!("meta:last-run put failed: {:?}", e);
        }
    }
}
