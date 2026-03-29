mod parser;
mod storage;
mod types;

/// TTL for error-state and run-metadata KV entries: 4h (2× the 2h cron interval).
/// Feed data uses 24h TTL (see storage::store_feed).
const SHORT_TTL_SECS: u64 = 14400;

// All worker-specific code is wasm32-only.
// cargo test runs on native (x86_64) and only compiles parser/storage pure functions.
#[cfg(target_arch = "wasm32")]
use futures::future::join_all;
#[cfg(target_arch = "wasm32")]
use worker::{event, Env, ScheduleContext, ScheduledEvent};

#[cfg(target_arch = "wasm32")]
use parser::parse_feed;
#[cfg(target_arch = "wasm32")]
use storage::{store_feed, url_key};

/// RSS feed URLs to fetch on each cron run.
/// Hard limit: 45 (CF free tier allows 50 subrequests/invocation; leave 5 slack).
/// Baseline write budget: 45 feed writes + 1 meta write = 552 writes/day
/// (limit: 1000/day, before any transient error-state rewrites).
///
/// Phase 2: load this list from KV key "config:feeds" instead of hardcoding.
#[cfg(target_arch = "wasm32")]
const FEEDS: &[&str] = &[
    // AI & LLM (11)
    "https://simonwillison.net/atom/everything/",
    "https://openai.com/news/rss.xml",
    "https://arxiv.org/rss/cs.AI",
    "https://arxiv.org/rss/cs.LG",
    "https://research.google/blog/rss/",
    "https://deepmind.google/blog/rss.xml",
    "https://lilianweng.github.io/lil-log/feed.xml",
    "https://www.gwern.net/feed",
    "https://garymarcus.substack.com/feed",
    "https://minimaxir.com/index.xml",
    "https://thesequence.substack.com/feed",
    // Tech Blogs (25)
    "https://www.jeffgeerling.com/blog.xml",
    "https://www.seangoedecke.com/rss.xml",
    "https://krebsonsecurity.com/feed/",
    "https://daringfireball.net/feeds/main",
    "https://ericmigi.com/rss.xml",
    "https://idiallo.com/feed.rss",
    "https://pluralistic.net/feed/",
    "https://shkspr.mobi/blog/feed/",
    "https://lcamtuf.substack.com/feed",
    "https://mitchellh.com/feed.xml",
    "https://dynomight.net/feed.xml",
    "https://xeiaso.net/blog.rss",
    "https://devblogs.microsoft.com/oldnewthing/feed",
    "https://www.righto.com/feeds/posts/default",
    "https://rachelbythebay.com/w/atom.xml",
    "https://www.johndcook.com/blog/feed/",
    "https://matklad.github.io/feed.xml",
    "https://eli.thegreenplace.net/feeds/all.atom.xml",
    "https://fabiensanglard.net/rss.xml",
    "https://blog.miguelgrinberg.com/feed",
    "https://www.troyhunt.com/rss/",
    "https://anildash.com/feed.xml",
    "https://computer.rip/rss.xml",
    "https://www.tedunangst.com/flak/rss",
    "https://steveblank.com/feed/",
    // Startup & News (7)
    "https://news.ycombinator.com/rss",
    "https://www.techmeme.com/feed.xml",
    "https://techcrunch.com/feed/",
    "https://techcrunch.com/category/artificial-intelligence/feed/",
    "https://techcrunch.com/tag/funding/feed/",
    "https://venturebeat.com/feed/",
    "https://venturebeat.com/category/ai/feed/",
    // 中文 (2)
    "https://www.ruanyifeng.com/blog/atom.xml",
    "https://www.bestblogs.dev/zh/feeds/rss?category=ai&minScore=90",
];

#[cfg(target_arch = "wasm32")]
async fn fetch_url(url: &str) -> Result<String, String> {
    // MVP: rely on CF Workers 30s platform timeout (no manual timeout).
    // Phase 2: race fetch against gloo-timers::TimeoutFuture for per-feed timeout.
    let req = worker::Request::new(url, worker::Method::Get).map_err(|e| e.to_string())?;
    let mut resp = worker::Fetch::Request(req)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.text().await.map_err(|e| e.to_string())
}

/// Write a {error, fetched} JSON record to KV with a 4-hour TTL.
/// Lets consumers distinguish "broken feed" from "never fetched" between runs.
#[cfg(target_arch = "wasm32")]
async fn write_error_state(kv: &worker::kv::KvStore, url: &str, error: &str, timestamp: &str) {
    let v = serde_json::json!({"error": error, "fetched": timestamp}).to_string();
    match kv.put(&url_key(url), v) {
        Ok(b) => {
            if let Err(e) = b.expiration_ttl(SHORT_TTL_SECS).execute().await {
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
            if let Err(e) = b.expiration_ttl(SHORT_TTL_SECS).execute().await {
                worker::console_error!("meta:last-run write failed: {:?}", e);
            }
        }
        Err(e) => {
            worker::console_error!("meta:last-run put failed: {:?}", e);
        }
    }
}
