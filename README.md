# rss-worker

High-performance RSS feed aggregator running on Cloudflare Workers free tier.
Written in Rust, compiled to WASM. Fetches up to 45 feeds in parallel every 2 hours
and stores structured JSON to KV.

## Features

- Full Rust (`worker-rs 0.4.0`) — no JS parsing, no DOM tree
- `quick-xml` pull parser: RSS 2.0 + Atom 1.0, CDATA, Atom link attributes
- Parallel fetch via `futures::join_all`
- FNV-1a KV keys (saves ~50-80KB vs SHA-256)
- Error state in KV on failure — consumers can distinguish broken from never-fetched
- Run metadata at `meta:last-run` — detect partial runs
- WASM bundle gzip <800KB (CF 1MB limit, with margin)
- `cargo test` on native x86_64 for unit tests

## Setup

```bash
# Prerequisites
rustup target add wasm32-unknown-unknown
npm install -g wrangler
cargo install worker-build
brew install binaryen  # for wasm-opt

# Create KV namespace
wrangler kv:namespace create RSS_STORE

# Edit wrangler.toml — replace <your-kv-namespace-id> with the namespace ID above
# Edit src/lib.rs — add your feed URLs to the FEEDS constant

# Build
bash build.sh

# Deploy
wrangler deploy

# Test locally
wrangler dev --test-scheduled
# then in another terminal:
curl "http://localhost:8787/__scheduled?cron=0+*/2+*+*+*"
```

## Configuration

Edit `src/lib.rs` and add your feed URLs to the `FEEDS` constant (max 45):

```rust
const FEEDS: &[&str] = &[
    "https://example.com/feed.xml",
    "https://blog.example.org/rss",
];
```

## KV Schema

| Key | Value | TTL |
|-----|-------|-----|
| `feed:{16 hex chars}` | `{"url":"...","fetched":"ISO8601","items":[...]}` | 24h |
| `feed:{16 hex chars}` (error) | `{"error":"...","fetched":"ISO8601"}` | 4h |
| `meta:last-run` | `{"ts":"ISO8601","success":N,"failed":N,"total":N}` | 4h |

## Running Tests

```bash
# Unit tests (native x86_64, no WASM required)
cargo test

# Manual scheduled-event smoke test (requires wrangler installed)
wrangler dev --test-scheduled
```

## CI/CD

GitHub Actions workflow at `.github/workflows/deploy.yml`.
Required secrets: `CF_API_TOKEN`, `CF_ACCOUNT_ID`, `KV_NAMESPACE_ID`.

## TODOS

See `TODOS.md` for deferred work:
- KV write cap guard (quota check before each run)
- latin-1 / non-UTF-8 feed encoding support via `encoding_rs`
- `<content:encoded>` namespace support
