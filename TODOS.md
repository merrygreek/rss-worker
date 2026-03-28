# TODOS — rss-worker

## TODO-1: KV write cap guard

**What:** Check remaining KV write quota before each cron run. Warn (via `console_error!`) or skip remaining feeds if near the daily limit.

**Why:** Free tier is 1000 KV writes/day. At 45 feeds × 12 runs/day = 552 writes, you have ~45% headroom now. If FEEDS grows past ~83 feeds or run frequency doubles, writes will silently fail — KV returns an error, feeds appear stale, no visible alert.

**Pros:** Prevents silent data loss when quota is exhausted. Turns a silent failure into a logged warning.

**Cons:** Cloudflare KV does not expose remaining quota via the Workers API directly — this would require reading a counter from KV (`meta:write-count`) and incrementing it atomically (which KV doesn't support natively). Approximate only.

**Context:** The simpler approach is a write-count accumulator: increment a counter in KV at the start of each run, check it before writing feeds. Not atomic (race possible if two runs overlap) but overlap is unlikely with 2h cron spacing. Alternatively, compute expected writes from `FEEDS.len()` and refuse if `FEEDS.len() * runs_per_day > 950`.

**Depends on:** None. Standalone addition to the handler in `src/lib.rs`.

---

## TODO-2: latin-1 / non-UTF-8 feed encoding

**What:** Detect `encoding="ISO-8859-1"` (or other non-UTF-8 encodings) in the XML prolog and transcode the bytes to UTF-8 before handing to `quick-xml`.

**Why:** `quick-xml` expects UTF-8 input. Feeds with latin-1 encoding declarations will fail the parse step today — the error is logged and the feed is skipped gracefully, but you silently lose that feed. Some older RSS feeds (government sites, legacy blogs) still ship latin-1.

**Pros:** Broader feed compatibility. Turns silent feed loss into successful parses.

**Cons:** Adds `encoding_rs` crate (~30KB WASM, acceptable). Requires reading the prolog before parsing, then re-encoding. Adds code complexity.

**Context:** Pattern: peek the first 200 bytes, regex-match `encoding="([^"]+)"`, if non-UTF-8 use `encoding_rs::Encoding::for_label()` to get a decoder and transcode. Then pass the UTF-8 string to `parse_feed`. Only needed for feeds that don't self-identify as UTF-8 (which is the majority).

**Depends on:** None. Isolated to the fetch/parse pipeline in `src/lib.rs`.

---

## TODO-3: content:encoded namespace support

**What:** Parse `<content:encoded>` as a fallback for `<description>` when the description field is empty.

**Why:** Some RSS feeds (WordPress-generated, Medium, Substack) use the Content module extension: `<content:encoded><![CDATA[...]]></content:encoded>` for the full post body instead of `<description>`. The parser today only reads `<description>` — these feeds return empty `description` fields.

**Pros:** Richer content extraction from high-value feeds. No new dependencies — just an additional parser state branch.

**Cons:** Namespaced elements in quick-xml require checking the local name after stripping the prefix (`content:encoded` → local name `encoded`, namespace URI `http://purl.org/rss/1.0/modules/content/`). Adds a `ContentEncoded` variant to the `Field` enum and a fallback assignment in the `End` handler.

**Context:** quick-xml exposes the raw qualified name. Check for `b"content:encoded"` as a byte match on `BytesStart::name()`. If `item.description.is_empty()` after parsing, substitute from `content_encoded_buf`. This is safe — `content:encoded` is always a superset of `description`.

**Depends on:** None. Isolated to `src/parser.rs`.
