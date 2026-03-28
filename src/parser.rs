use quick_xml::{events::Event, Reader};

use crate::types::FeedItem;

#[derive(Debug)]
enum Field {
    Title,
    Link,
    Date,
    Desc,
    Guid,
}

/// Parse RSS 2.0 or Atom 1.0 XML into a list of feed items.
///
/// Items without a link are filtered out. Order matches document order.
/// Returns Err on malformed XML or unresolvable entity references. Never panics.
///
/// Design notes:
///   - Pull parser: no DOM tree, O(1) memory per event
///   - CDATA: handled via Event::CData (raw bytes, no XML unescaping)
///   - Atom <link href="..." rel="alternate"/>: handled for both
///     self-closing tags and explicit <link></link> pairs
///   - depth tracks nesting inside item/entry; field tags only matched at depth 0
///     to prevent nested elements (e.g. <p> inside <description>) from corrupting
///     in_field. Text/CData resets in_field only when back at depth 0 (after End).
pub fn parse_feed(xml: &str) -> Result<Vec<FeedItem>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut items: Vec<FeedItem> = Vec::new();
    let mut current: Option<FeedItem> = None;
    let mut in_field: Option<Field> = None;
    let mut depth: u32 = 0; // nesting depth inside current item/entry

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            // RSS 2.0 <item> or Atom <entry> — start a new item
            Event::Start(ref e) if matches!(e.name().as_ref(), b"item" | b"entry") => {
                current = Some(FeedItem::default());
                in_field = None;
                depth = 0;
            }

            // Field start tags — only at depth 0 (directly inside item/entry).
            // Nested elements (e.g. <p> inside <description>) do not change in_field.
            Event::Start(ref e) => {
                if let Some(ref mut item) = current {
                    if depth == 0 {
                        if e.name().as_ref() == b"link" && !is_rss_link(e) {
                            apply_atom_link(item, e);
                            in_field = None;
                        } else {
                            in_field = match e.name().as_ref() {
                                b"title" => Some(Field::Title),
                                b"link" if is_rss_link(e) => Some(Field::Link),
                                b"pubDate" | b"published" | b"updated" => Some(Field::Date),
                                b"description" | b"summary" | b"content" => Some(Field::Desc),
                                b"guid" | b"id" => Some(Field::Guid),
                                _ => None,
                            };
                        }
                    }
                    depth += 1;
                }
            }

            // Atom <link href="..." rel="alternate"/> — self-closing, handled here
            Event::Empty(ref e) if e.name().as_ref() == b"link" => {
                if let Some(ref mut item) = current {
                    apply_atom_link(item, e);
                }
            }

            // Plain text — accumulate into active field across multiple Text events.
            // in_field resets only when depth returns to 0 (on the matching End tag).
            Event::Text(ref e) => {
                if let (Some(ref mut item), Some(ref field)) = (&mut current, &in_field) {
                    let text = e.unescape().map_err(|e| e.to_string())?;
                    apply_field(item, field, text.into_owned());
                }
                if depth == 0 {
                    in_field = None;
                }
            }

            // CDATA — raw bytes, no unescape (e.g. <description><![CDATA[<p>…</p>]]></description>)
            Event::CData(ref e) => {
                if let (Some(ref mut item), Some(ref field)) = (&mut current, &in_field) {
                    let text = String::from_utf8_lossy(e.as_ref()).into_owned();
                    apply_field(item, field, text);
                }
                if depth == 0 {
                    in_field = None;
                }
            }

            // End of <item> or <entry> — commit item if it has a link
            Event::End(ref e) if matches!(e.name().as_ref(), b"item" | b"entry") => {
                if let Some(item) = current.take() {
                    if !item.link.is_empty() {
                        items.push(item);
                    }
                }
                in_field = None;
                depth = 0;
            }

            // Any other end tag — decrement depth; reset in_field when back at item level
            Event::End(_) => {
                if depth > 0 {
                    depth -= 1;
                }
                if depth == 0 {
                    in_field = None;
                }
            }

            Event::Eof => break,
            _ => {}
        }
    }

    Ok(items)
}

fn apply_field(item: &mut FeedItem, field: &Field, text: String) {
    // push_str accumulates across multiple Text events for the same field
    // (e.g. text nodes split by nested elements like <p>).
    match field {
        Field::Title => item.title.push_str(&text),
        Field::Link => item.link.push_str(&text),
        Field::Date => item.pub_date.push_str(&text),
        Field::Desc => item.description.push_str(&text),
        Field::Guid => item.guid.push_str(&text),
    }
}

fn apply_atom_link(item: &mut FeedItem, e: &quick_xml::events::BytesStart<'_>) {
    let mut href = String::new();
    let mut rel = String::new();

    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"href" => href = String::from_utf8_lossy(&attr.value).into_owned(),
            b"rel" => rel = String::from_utf8_lossy(&attr.value).into_owned(),
            _ => {}
        }
    }

    if !href.is_empty() && (rel.is_empty() || rel == "alternate") {
        item.link = href;
    }
}

/// Returns true if this <link> start tag is RSS 2.0 style (text node follows).
/// Returns false if it has an href attribute (Atom style, handled by apply_atom_link).
pub(crate) fn is_rss_link(e: &quick_xml::events::BytesStart) -> bool {
    !e.attributes()
        .any(|a| a.map(|a| a.key.as_ref() == b"href").unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::BytesStart;

    // --- parse_feed tests ---

    #[test]
    fn test_rss2_happy_path() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item>
    <title>First</title>
    <link>https://example.com/1</link>
    <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
    <description>Desc 1</description>
    <guid>guid-1</guid>
  </item>
  <item>
    <title>Second</title>
    <link>https://example.com/2</link>
    <pubDate>Tue, 02 Jan 2024 00:00:00 +0000</pubDate>
    <description>Desc 2</description>
    <guid>guid-2</guid>
  </item>
</channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "First");
        assert_eq!(items[0].link, "https://example.com/1");
        assert_eq!(items[0].pub_date, "Mon, 01 Jan 2024 00:00:00 +0000");
        assert_eq!(items[0].description, "Desc 1");
        assert_eq!(items[0].guid, "guid-1");
        assert_eq!(items[1].title, "Second");
        assert_eq!(items[1].link, "https://example.com/2");
    }

    #[test]
    fn test_atom_happy_path() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <title>Atom Post</title>
    <link href="https://atom.example.com/post" rel="alternate"/>
    <published>2024-01-01T00:00:00Z</published>
    <summary>Summary text</summary>
    <id>atom-id-1</id>
  </entry>
</feed>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].link, "https://atom.example.com/post");
        assert_eq!(items[0].title, "Atom Post");
    }

    #[test]
    fn test_atom_link_with_explicit_closing_tag() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <title>Explicit Close</title>
    <link href="https://atom.example.com/explicit" rel="alternate"></link>
    <id>atom-id-2</id>
  </entry>
</feed>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].link, "https://atom.example.com/explicit");
        assert_eq!(items[0].title, "Explicit Close");
    }

    #[test]
    fn test_cdata_description() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item>
    <title>CDATA Test</title>
    <link>https://example.com/cdata</link>
    <description><![CDATA[<p>Hello</p>]]></description>
  </item>
</channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "<p>Hello</p>");
    }

    #[test]
    fn test_atom_link_rel_filtering() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <title>Rel Test</title>
    <link rel="self" href="https://self.example.com"/>
    <link rel="alternate" href="https://post.example.com"/>
    <id>rel-id</id>
  </entry>
</feed>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 1);
        // self link should be ignored; alternate link wins
        assert_eq!(items[0].link, "https://post.example.com");
    }

    #[test]
    fn test_item_without_link_filtered() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item>
    <title>No Link</title>
    <description>This item has no link element</description>
  </item>
</channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 0, "items without link should be dropped");
    }

    #[test]
    fn test_empty_feed() {
        let xml = r#"<?xml version="1.0"?><rss version="2.0"><channel></channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_malformed_xml_returns_err() {
        // "<item" hits EOF in the middle of a tag — quick-xml returns UnexpectedEof
        let result = parse_feed("<item");
        assert!(
            result.is_err(),
            "malformed XML should return Err, not panic"
        );
    }

    #[test]
    fn test_order_preserved() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item><title>A</title><link>https://a.com</link></item>
  <item><title>B</title><link>https://b.com</link></item>
  <item><title>C</title><link>https://c.com</link></item>
  <item><title>D</title><link>https://d.com</link></item>
  <item><title>E</title><link>https://e.com</link></item>
</channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].title, "A");
        assert_eq!(items[4].title, "E");
    }

    #[test]
    fn test_nested_html_in_description_does_not_corrupt_guid() {
        // Real-world feeds sometimes put inline HTML in <description> without CDATA.
        // The nested <p> tag must not reset in_field to something wrong.
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item>
    <title>Nested Test</title>
    <link>https://example.com/nested</link>
    <description><p>Some text</p></description>
    <guid>correct-guid</guid>
  </item>
</channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "Some text");
        assert_eq!(
            items[0].guid, "correct-guid",
            "<p> inside description must not corrupt guid"
        );
        assert_eq!(items[0].title, "Nested Test");
    }

    #[test]
    fn test_bad_entity_reference_returns_err() {
        // &nonexistent; is not a predefined XML entity — unescape() should fail.
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item>
    <title>Hello &nonexistent; World</title>
    <link>https://example.com/bad-entity</link>
  </item>
</channel></rss>"#;
        let result = parse_feed(xml);
        assert!(
            result.is_err(),
            "undefined entity reference should return Err, not silently produce empty field"
        );
    }

    // --- is_rss_link tests ---

    #[test]
    fn test_is_rss_link_no_href() {
        // <link> with no attributes — RSS 2.0 style, text node follows
        let e = BytesStart::new("link");
        assert!(is_rss_link(&e), "bare <link> should be RSS2 style");
    }

    #[test]
    fn test_is_rss_link_has_href() {
        // <link href="..."> — Atom style, handled by Event::Empty branch
        let mut e = BytesStart::new("link");
        e.push_attribute(("href", "https://example.com"));
        assert!(!is_rss_link(&e), "<link href=...> should NOT be RSS2 style");
    }
}
