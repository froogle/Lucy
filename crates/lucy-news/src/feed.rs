//! Reads RSS and Atom feeds into plain-text items. Feed text is untrusted:
//! markup is stripped, lengths are capped and only web links survive.

use roxmltree::{Document, Node, ParsingOptions};

const ATOM: &str = "http://www.w3.org/2005/Atom";
const RSS_1: &str = "http://purl.org/rss/1.0/";
const DUBLIN_CORE: &str = "http://purl.org/dc/elements/1.1/";

/// The longest headline and description kept, in characters.
const HEADLINE_CHARS: usize = 200;
const DESCRIPTION_CHARS: usize = 600;
const DATE_CHARS: usize = 64;

/// One story, as its feed tells it.
#[derive(Debug, PartialEq)]
pub struct Item {
    pub headline: String,
    pub description: String,
    /// Only `http` and `https` links are kept.
    pub link: Option<String>,
    /// When it was published, as the feed writes it; empty if it doesn't
    /// say. Some feeds go stale, so this lets old stories be left out.
    pub published: String,
}

/// The items of an RSS 2.0, RSS 1.0 or Atom feed, in feed order. Items
/// without a headline are skipped.
pub fn parse(xml: &str) -> Result<Vec<Item>, String> {
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    let document =
        Document::parse_with_options(xml, options).map_err(|e| format!("not a feed: {e}"))?;
    let items = document
        .descendants()
        .filter_map(
            |node| match (node.tag_name().name(), node.tag_name().namespace()) {
                ("item", None | Some(RSS_1)) => Some(rss_item(node)),
                ("entry", Some(ATOM)) => Some(atom_entry(node)),
                _ => None,
            },
        )
        .filter(|item| !item.headline.is_empty())
        .collect();
    Ok(items)
}

fn rss_item(item: Node) -> Item {
    let field = |name| child(item, name, &[None, Some(RSS_1)]).map(text);
    Item {
        headline: clean(&field("title").unwrap_or_default(), HEADLINE_CHARS),
        description: clean(&field("description").unwrap_or_default(), DESCRIPTION_CHARS),
        link: field("link").as_deref().and_then(web_link),
        published: clean(
            &field("pubDate")
                .or_else(|| child(item, "date", &[Some(DUBLIN_CORE)]).map(text))
                .unwrap_or_default(),
            DATE_CHARS,
        ),
    }
}

fn atom_entry(entry: Node) -> Item {
    let field = |name| child(entry, name, &[Some(ATOM)]).map(text);
    let link = entry
        .children()
        .filter(|node| node.has_tag_name((ATOM, "link")))
        .find(|link| matches!(link.attribute("rel"), None | Some("alternate")))
        .and_then(|link| link.attribute("href"))
        .and_then(web_link);
    Item {
        headline: clean(&field("title").unwrap_or_default(), HEADLINE_CHARS),
        description: clean(
            &field("summary")
                .or_else(|| field("content"))
                .unwrap_or_default(),
            DESCRIPTION_CHARS,
        ),
        link,
        published: clean(
            &field("published")
                .or_else(|| field("updated"))
                .unwrap_or_default(),
            DATE_CHARS,
        ),
    }
}

/// The first child element of `parent` called `name` in one of
/// `namespaces`, so that `media:description` isn't taken for `description`.
fn child<'a>(
    parent: Node<'a, 'a>,
    name: &str,
    namespaces: &[Option<&str>],
) -> Option<Node<'a, 'a>> {
    parent.children().find(|node| {
        node.is_element()
            && node.tag_name().name() == name
            && namespaces.contains(&node.tag_name().namespace())
    })
}

fn text(node: Node) -> String {
    node.descendants()
        .filter(Node::is_text)
        .filter_map(|text| text.text())
        .collect()
}

fn web_link(link: &str) -> Option<String> {
    let link = link.trim();
    let lower = link.to_ascii_lowercase();
    let web = lower.starts_with("http://") || lower.starts_with("https://");
    let clean = !link.chars().any(|c| c.is_whitespace() || c.is_control());
    (web && clean).then(|| link.to_owned())
}

/// Plain text from feed text that may hold HTML: tags (and script and style
/// contents) removed, entities decoded, whitespace collapsed and the length
/// capped at `max` characters.
fn clean(html: &str, max: usize) -> String {
    let decoded = decode_entities(&strip_tags(html));
    let words: Vec<&str> = decoded
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect();
    let text = words.join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    cut.push('…');
    cut
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tag = &rest[start..];
        let Some(end) = tag.find('>') else {
            rest = "";
            break;
        };
        let name: String = tag[1..end]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        rest = &tag[end + 1..];
        if name == "script" || name == "style" {
            let close = format!("</{name}");
            rest = match rest.to_ascii_lowercase().find(&close) {
                Some(at) => rest[at..].find('>').map_or("", |gt| &rest[at + gt + 1..]),
                None => "",
            };
        }
        // Keep words either side of a tag apart.
        out.push(' ');
    }
    out.push_str(rest);
    out
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let entity = &rest[start..];
        let decoded = entity
            .find(';')
            .filter(|&end| end <= 10)
            .and_then(|end| Some((entity_char(&entity[1..end])?, end)));
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &entity[end + 1..];
            }
            None => {
                out.push('&');
                rest = &entity[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity_char(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_atom_feed_reads_like_an_rss_one() {
        let atom = r#"<?xml version="1.0" encoding="utf-8"?>
            <feed xmlns="http://www.w3.org/2005/Atom">
              <title>Example</title>
              <entry>
                <title>Bridge reopens</title>
                <updated>2026-11-13T08:00:00Z</updated>
                <link rel="enclosure" href="https://example.com/photo.jpg"/>
                <link href="https://example.com/bridge"/>
                <summary type="html">&lt;p&gt;The bridge &amp;amp; road reopen.&lt;/p&gt;</summary>
              </entry>
            </feed>"#;

        assert_eq!(
            parse(atom).unwrap(),
            vec![Item {
                headline: "Bridge reopens".into(),
                description: "The bridge & road reopen.".into(),
                link: Some("https://example.com/bridge".into()),
                published: "2026-11-13T08:00:00Z".into(),
            }]
        );
    }
}
