# Changelog

All notable changes to rss-worker are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)

## [0.1.0.0] - 2026-03-28

### Added
- Full Rust Cloudflare Worker using `worker-rs 0.4.0`
- RSS 2.0 and Atom 1.0 pull parser via `quick-xml 0.36` (no DOM tree, streaming)
- CDATA description handling (`Event::CData` — no double-unescaping)
- Atom `<link href="..." rel="alternate"/>` parsing via `Event::Empty`
- Parallel fetch of up to 45 RSS feeds using `futures::join_all`
- KV storage with FNV-1a hash keys (`feed:{16 hex chars}`, 24h TTL)
- Error state written to KV on fetch/parse failure (1h TTL, lets consumers distinguish "broken" from "never fetched")
- Run metadata written to `meta:last-run` KV key after each cron run
- Cron trigger every 2 hours (45 feeds × 12 runs/day = 540 KV writes/day, well under 1000/day free tier limit)
- GitHub Actions CI/CD: build, unit tests, deploy, smoke test
- `build.sh`: `wasm-opt -Oz` with 800KB gzip size gate
- Binaryen v117 pinned in CI to avoid miscompile risk
- Unit tests for parser (10 cases) and storage (3 cases), run on native x86_64 via `cargo test`
