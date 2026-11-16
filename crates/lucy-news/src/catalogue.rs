#[derive(Debug)]
pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub about: &'static str,
    pub feed: Option<&'static str>,
    pub front_page: &'static str,
    pub ai_summary_allowed: bool,
}

impl Source {
    pub fn readable(&self) -> bool {
        self.feed.is_some() && self.ai_summary_allowed
    }

    pub fn why_not(&self) -> Option<&'static str> {
        match (self.feed, self.ai_summary_allowed) {
            (None, _) => Some("it has no public feed"),
            (Some(_), false) => Some("its terms don't allow AI summaries of its content"),
            (Some(_), true) => None,
        }
    }
}

pub const DEFAULT_ROTATION: &[&str] = &["cnn", "npr", "sky"];

pub const SOURCES: &[Source] = &[
    Source {
        id: "cnn",
        name: "CNN",
        aliases: &["cnn international"],
        about: "US cable news, international edition",
        feed: Some("http://rss.cnn.com/rss/edition.rss"),
        front_page: "https://edition.cnn.com",
        ai_summary_allowed: true,
    },
    Source {
        id: "npr",
        name: "NPR",
        aliases: &["national public radio"],
        about: "US public radio: calm, national and world news",
        feed: Some("https://feeds.npr.org/1001/rss.xml"),
        front_page: "https://www.npr.org",
        ai_summary_allowed: true,
    },
    Source {
        id: "sky",
        name: "Sky News",
        aliases: &["sky"],
        about: "UK broadcaster: UK and world news",
        feed: Some("https://feeds.skynews.com/feeds/rss/home.xml"),
        front_page: "https://news.sky.com",
        ai_summary_allowed: true,
    },
    Source {
        id: "fox",
        name: "Fox News",
        aliases: &["fox"],
        about: "US cable news, conservative-leaning",
        feed: Some("https://moxie.foxnews.com/google-publisher/latest.xml"),
        front_page: "https://www.foxnews.com",
        ai_summary_allowed: true,
    },
    Source {
        id: "aljazeera",
        name: "Al Jazeera",
        aliases: &["al jazeera english", "aljazeera"],
        about: "Qatar-based international news, strong on the Middle East and Global South",
        feed: Some("https://www.aljazeera.com/xml/rss/all.xml"),
        front_page: "https://www.aljazeera.com",
        ai_summary_allowed: true,
    },
    Source {
        id: "bbc",
        name: "BBC News",
        aliases: &["bbc", "the bbc"],
        about: "UK public broadcaster",
        feed: Some("https://feeds.bbci.co.uk/news/rss.xml"),
        front_page: "https://www.bbc.co.uk/news",
        ai_summary_allowed: false,
    },
    Source {
        id: "guardian",
        name: "The Guardian",
        aliases: &["guardian"],
        about: "UK newspaper, left-leaning",
        feed: Some("https://www.theguardian.com/international/rss"),
        front_page: "https://www.theguardian.com",
        ai_summary_allowed: false,
    },
    Source {
        id: "nyt",
        name: "The New York Times",
        aliases: &["new york times", "nyt"],
        about: "US newspaper",
        feed: Some("https://rss.nytimes.com/services/xml/rss/nyt/HomePage.xml"),
        front_page: "https://www.nytimes.com",
        ai_summary_allowed: false,
    },
    Source {
        id: "reuters",
        name: "Reuters",
        aliases: &[],
        about: "International wire service",
        feed: None,
        front_page: "https://www.reuters.com",
        ai_summary_allowed: false,
    },
    Source {
        id: "ap",
        name: "AP News",
        aliases: &["ap", "associated press", "the associated press"],
        about: "US wire service",
        feed: None,
        front_page: "https://apnews.com",
        ai_summary_allowed: false,
    },
];

pub fn by_id(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|source| source.id == id)
}

pub fn find(name: &str) -> Option<&'static Source> {
    let wanted = normalise(name);
    SOURCES.iter().find(|source| {
        normalise(source.id) == wanted
            || normalise(source.name) == wanted
            || source
                .aliases
                .iter()
                .any(|alias| normalise(alias) == wanted)
    })
}

fn normalise(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower
        .strip_prefix("the ")
        .unwrap_or(&lower)
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_declares_exactly_the_feed_hosts_of_readable_sources() {
        let manifest: toml::Table = toml::from_str(include_str!("../manifest.toml")).unwrap();
        let mut declared: Vec<&str> = manifest["access"]["network"]
            .as_array()
            .unwrap()
            .iter()
            .map(|host| host.as_str().unwrap())
            .collect();
        let mut hosts: Vec<&str> = SOURCES
            .iter()
            .filter(|source| source.readable())
            .map(|source| {
                let feed = source.feed.unwrap();
                let rest = feed.split_once("://").unwrap().1;
                rest.split('/').next().unwrap()
            })
            .collect();
        declared.sort();
        hosts.sort();
        assert_eq!(declared, hosts);
    }
}
