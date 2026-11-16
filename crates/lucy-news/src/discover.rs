use crate::feed;
use crate::Web;

const COMMON_PATHS: &[&str] = &["/feed", "/rss", "/feed.xml", "/rss.xml", "/atom.xml"];

#[derive(Debug, PartialEq)]
pub struct SiteFeed {
    pub host: String,
    pub feed: String,
    pub front_page: String,
}

pub fn home_page(site: &str) -> Option<(String, String)> {
    let site = site.trim();
    let (scheme, rest) = match site.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("https".to_owned(), site),
    };
    let host = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    (matches!(&*scheme, "http" | "https") && is_public_name(&host))
        .then(|| (format!("{scheme}://{host}/"), host))
}

fn is_public_name(host: &str) -> bool {
    let top_level = host.rsplit('.').next().unwrap_or_default();
    host.contains('.')
        && !top_level.is_empty()
        && top_level.chars().all(|c| c.is_ascii_alphabetic())
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

fn is_on_public_site(url: &str) -> bool {
    home_page(url).is_some()
}

pub fn find(web: &dyn Web, site: &str) -> Option<SiteFeed> {
    let (front_page, host) = home_page(site)?;
    let origin = front_page.trim_end_matches('/');
    let declared = web
        .get(&front_page)
        .map(|html| alternate_feeds(&html, &front_page))
        .unwrap_or_default()
        .into_iter()
        .filter(|url| is_on_public_site(url));
    let common = COMMON_PATHS.iter().map(|path| format!("{origin}{path}"));
    let feed = declared.chain(common).find(|url| {
        web.get(url)
            .and_then(|xml| feed::parse(&xml))
            .is_ok_and(|items| !items.is_empty())
    })?;
    Some(SiteFeed {
        host,
        feed,
        front_page,
    })
}

fn alternate_feeds(html: &str, page: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut feeds = Vec::new();
    let mut rest = 0;
    while let Some(start) = lower[rest..].find("<link").map(|i| rest + i) {
        let end = lower[start..].find('>').map_or(lower.len(), |i| start + i);
        let tag = &html[start..end];
        rest = end;
        let attribute = |name: &str| attribute(tag, name);
        let is_feed = matches!(
            attribute("type")
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("application/rss+xml" | "application/atom+xml")
        );
        let alternate = attribute("rel").is_some_and(|rel| {
            rel.split_whitespace()
                .any(|r| r.eq_ignore_ascii_case("alternate"))
        });
        if let (true, true, Some(href)) = (is_feed, alternate, attribute("href")) {
            if let Some(url) = resolve(page, &href) {
                feeds.push(url);
            }
        }
    }
    feeds
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name).map(|i| from + i) {
        from = at + name.len();
        let before = lower[..at].chars().next_back();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let after = lower[from..].trim_start();
        let Some(value) = after.strip_prefix('=') else {
            continue;
        };
        let value = original_case(tag, value).trim_start();
        return Some(match value.chars().next() {
            Some(quote @ ('"' | '\'')) => value[1..].split(quote).next()?.to_owned(),
            _ => {
                let token = value.split_whitespace().next()?;
                let ends_tag = value.trim_end() == token;
                match token.strip_suffix('/') {
                    Some(unclosed) if ends_tag && !unclosed.is_empty() => unclosed.to_owned(),
                    _ => token.to_owned(),
                }
            }
        });
    }
    None
}

fn original_case<'a>(text: &'a str, lowercased_tail: &str) -> &'a str {
    &text[text.len() - lowercased_tail.len()..]
}

fn resolve(page: &str, href: &str) -> Option<String> {
    let href = href.trim();
    let (scheme, rest) = page.split_once("://")?;
    let origin = format!("{scheme}://{}", rest.split('/').next()?);
    let url = if href.starts_with("http://") || href.starts_with("https://") {
        href.to_owned()
    } else if let Some(path) = href.strip_prefix("//") {
        format!("{scheme}://{path}")
    } else if href.starts_with('/') {
        format!("{origin}{href}")
    } else if href.contains(':') {
        return None;
    } else {
        format!("{}{href}", &page[..page.rfind('/')? + 1])
    };
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_is_named_by_its_address_with_or_without_a_scheme() {
        assert_eq!(
            home_page("DailyPlanet.example"),
            Some((
                "https://dailyplanet.example/".into(),
                "dailyplanet.example".into()
            ))
        );
        assert_eq!(
            home_page("http://www.dailyplanet.example/world?x=1"),
            Some((
                "http://www.dailyplanet.example/".into(),
                "www.dailyplanet.example".into()
            ))
        );
        assert_eq!(home_page("The Daily Planet"), None);
        assert_eq!(home_page("file:///etc/passwd"), None);
        assert_eq!(home_page("192.168.1.1"), None);
        assert_eq!(home_page("dailyplanet.example:8080"), None);
    }

    #[test]
    fn only_alternate_rss_and_atom_links_are_feeds_and_they_resolve_against_the_page() {
        let html = r#"<html><head>
            <LINK rel="stylesheet" type="text/css" href="/style.css">
            <link rel="alternate" type="application/rss+xml" title="All" href="/feeds/all.rss">
            <link href='https://feeds.example/atom' type='application/atom+xml' rel='alternate'/>
            <link rel=alternate type=application/rss+xml href=world.rss>
            <link rel="alternate" type="application/rss+xml" href="javascript:alert(1)">
            <link rel="alternate" hreflang="fr" href="/fr/">
        </head></html>"#;

        assert_eq!(
            alternate_feeds(html, "https://dailyplanet.example/"),
            [
                "https://dailyplanet.example/feeds/all.rss",
                "https://feeds.example/atom",
                "https://dailyplanet.example/world.rss",
            ]
        );
    }
}
