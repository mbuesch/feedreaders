// -*- coding: utf-8 -*-
//
// Copyright (C) 2026 Michael Büsch <m@bues.ch>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 2 of the License, or
// (at your option) any later version.
//
// SPDX-License-Identifier: GPL-2.0-or-later

use anyhow::{self as ah, format_err as err};
use jiff::{Timestamp, fmt::rfc2822};
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, PartialEq)]
pub struct Feed {
    pub title: Option<String>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub author: String,
    pub title: Option<String>,
    pub link: String,
    pub published: Option<Timestamp>,
    pub updated: Option<Timestamp>,
    pub summary: Option<String>,
}

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b([a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,})\b")
        .expect("valid email regular expression")
});

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn rss_author_name(value: &str) -> Option<String> {
    let Some(email) = EMAIL_RE.find(value) else {
        return nonempty(Some(value));
    };

    let prefix = value[..email.start()]
        .trim()
        .trim_matches(['<', '(', '['])
        .trim();
    let suffix = value[email.end()..]
        .trim()
        .trim_matches(['>', ')', ']'])
        .trim();
    nonempty(Some(if prefix.len() >= suffix.len() {
        prefix
    } else {
        suffix
    }))
}

fn rss_authors(item: &rss::Item) -> String {
    let authors = item
        .author()
        .into_iter()
        .chain(
            item.dublin_core_ext
                .iter()
                .flat_map(|extension| extension.creators().iter().map(String::as_str)),
        )
        .filter_map(rss_author_name);
    authors.collect::<Vec<_>>().join(", ")
}

fn parse_rss_date(value: &str) -> Option<Timestamp> {
    static PARSER: rfc2822::DateTimeParser = rfc2822::DateTimeParser::new().relaxed_weekday(true);

    PARSER
        .parse_timestamp(value)
        .ok()
        .or_else(|| value.parse().ok())
}

fn rss_published(item: &rss::Item) -> Option<Timestamp> {
    item.pub_date().and_then(parse_rss_date).or_else(|| {
        item.dublin_core_ext
            .iter()
            .flat_map(|extension| extension.dates())
            .find_map(|date| parse_rss_date(date))
    })
}

fn rss_updated(item: &rss::Item) -> Option<Timestamp> {
    item.extensions
        .values()
        .filter_map(|extensions| extensions.get("updated"))
        .flatten()
        .find_map(|extension| extension.value().and_then(parse_rss_date))
        .or_else(|| rss_published(item))
}

fn rss_extension_text(extension: &rss::extension::Extension, name: &str) -> Option<String> {
    extension.children.get(name)?.iter().find_map(|extension| {
        nonempty(extension.value()).or_else(|| rss_extension_text(extension, name))
    })
}

fn rss_media_summary(item: &rss::Item) -> Option<String> {
    item.itunes_ext
        .iter()
        .find_map(|extension| nonempty(extension.summary()))
        .or_else(|| {
            item.extensions
                .values()
                .flat_map(|extensions| {
                    extensions
                        .get("description")
                        .into_iter()
                        .flatten()
                        .filter_map(|extension| nonempty(extension.value()))
                })
                .next()
        })
        .or_else(|| {
            item.extensions
                .values()
                .flat_map(|extensions| extensions.values())
                .flatten()
                .find_map(|extension| rss_extension_text(extension, "description"))
        })
}

fn rss_id(item: &rss::Item) -> String {
    nonempty(item.guid().map(rss::Guid::value))
        .or_else(|| nonempty(item.link()))
        .or_else(|| nonempty(item.title()))
        .unwrap_or_default() //FIXME what did feed-rs do?
}

fn rss_link(item: &rss::Item) -> String {
    nonempty(item.link())
        .or_else(|| {
            item.extensions
                .values()
                .filter_map(|extensions| extensions.get("link"))
                .flatten()
                .find_map(|extension| {
                    extension
                        .attrs
                        .get("href")
                        .and_then(|href| nonempty(Some(href)))
                })
        })
        .unwrap_or_default()
}

fn from_rss(channel: rss::Channel) -> Feed {
    let entries = channel
        .items
        .iter()
        .map(|item| Entry {
            id: rss_id(item),
            author: rss_authors(item),
            title: nonempty(item.title()),
            link: rss_link(item),
            published: rss_published(item),
            updated: rss_updated(item),
            summary: nonempty(item.description()).or_else(|| rss_media_summary(item)),
        })
        .collect();

    Feed {
        title: nonempty(Some(channel.title())),
        entries,
    }
}

fn atom_timestamp(date: &atom_syndication::FixedDateTime) -> Option<Timestamp> {
    Timestamp::new(
        date.timestamp(),
        date.timestamp_subsec_nanos().try_into().ok()?,
    )
    .ok()
}

fn atom_extension_text(
    extension: &atom_syndication::extension::Extension,
    name: &str,
) -> Option<String> {
    extension.children.get(name)?.iter().find_map(|extension| {
        nonempty(extension.value()).or_else(|| atom_extension_text(extension, name))
    })
}

fn atom_media_summary(entry: &atom_syndication::Entry) -> Option<String> {
    entry
        .extensions
        .values()
        .flat_map(|extensions| {
            extensions
                .get("description")
                .into_iter()
                .flatten()
                .filter_map(|extension| nonempty(extension.value()))
        })
        .next()
        .or_else(|| {
            entry
                .extensions
                .values()
                .flat_map(|extensions| extensions.values())
                .flatten()
                .find_map(|extension| atom_extension_text(extension, "description"))
        })
}

fn atom_link(entry: &atom_syndication::Entry) -> String {
    entry
        .links
        .iter()
        .find(|link| link.rel() == "alternate")
        .or_else(|| entry.links.first())
        .map(atom_syndication::Link::href)
        .and_then(|href| nonempty(Some(href)))
        .unwrap_or_default()
}

fn from_atom(feed: atom_syndication::Feed) -> Feed {
    let entries = feed
        .entries
        .iter()
        .map(|entry| {
            let authors = if entry.authors.is_empty() {
                feed.authors.as_slice()
            } else {
                entry.authors.as_slice()
            };
            let link = atom_link(entry);
            Entry {
                id: nonempty(Some(entry.id()))
                    .or_else(|| nonempty(Some(&link)))
                    .or_else(|| nonempty(Some(&entry.title.value)))
                    .unwrap_or_default(), //FIXME what did feed-rs do?
                author: authors
                    .iter()
                    .filter_map(|person| nonempty(Some(&person.name)))
                    .collect::<Vec<_>>()
                    .join(", "),
                title: nonempty(Some(&entry.title.value)),
                link,
                published: entry.published().and_then(atom_timestamp),
                updated: atom_timestamp(entry.updated()),
                summary: entry
                    .summary()
                    .and_then(|summary| nonempty(Some(&summary.value)))
                    .or_else(|| atom_media_summary(entry)),
            }
        })
        .collect();

    Feed {
        title: nonempty(Some(&feed.title.value)),
        entries,
    }
}

pub fn parse(bytes: &[u8]) -> ah::Result<Feed> {
    match rss::Channel::read_from(bytes) {
        Ok(channel) => Ok(from_rss(channel)),
        Err(rss_error) => match atom_syndication::Feed::read_from(bytes) {
            Ok(feed) => Ok(from_atom(feed)),
            Err(atom_error) => Err(err!(
                "Failed to parse as RSS ({rss_error}) or Atom ({atom_error})"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rss() {
        let feed = parse(
            br#"<?xml version="1.0"?>
                <rss version="2.0"
                     xmlns:dc="http://purl.org/dc/elements/1.1/"
                     xmlns:media="http://search.yahoo.com/mrss/">
                  <channel>
                    <title>RSS title</title>
                    <link>https://example.com</link>
                    <description>RSS feed</description>
                    <item>
                      <title>Entry title</title>
                      <link>https://example.com/entry</link>
                      <guid>entry-id</guid>
                      <dc:creator>Entry Author</dc:creator>
                      <pubDate>Sun, 13 Jul 2024 15:09:59 -0400</pubDate>
                      <description></description>
                      <media:description>Media summary</media:description>
                    </item>
                  </channel>
                </rss>"#,
        )
        .unwrap();

        assert_eq!(feed.title.as_deref(), Some("RSS title"));
        assert_eq!(feed.entries.len(), 1);
        let entry = &feed.entries[0];
        assert_eq!(entry.id, "entry-id");
        assert_eq!(entry.author, "Entry Author");
        assert_eq!(entry.title.as_deref(), Some("Entry title"));
        assert_eq!(entry.link, "https://example.com/entry");
        assert_eq!(entry.summary.as_deref(), Some("Media summary"));
        assert_eq!(
            entry.published,
            Some("2024-07-13T19:09:59Z".parse().unwrap())
        );
    }

    #[test]
    fn parses_atom() {
        let feed = parse(
            br#"<?xml version="1.0"?>
                <feed xmlns="http://www.w3.org/2005/Atom">
                  <title>Atom title</title>
                  <id>https://example.com/feed</id>
                  <updated>2024-07-13T19:09:59Z</updated>
                  <author><name>Feed Author</name></author>
                  <entry>
                    <title>Entry title</title>
                    <id>entry-id</id>
                    <updated>2024-07-14T19:09:59Z</updated>
                    <published>2024-07-13T19:09:59Z</published>
                    <link rel="alternate" href="https://example.com/entry" />
                    <summary>Entry summary</summary>
                  </entry>
                </feed>"#,
        )
        .unwrap();

        assert_eq!(feed.title.as_deref(), Some("Atom title"));
        assert_eq!(feed.entries.len(), 1);
        let entry = &feed.entries[0];
        assert_eq!(entry.id, "entry-id");
        assert_eq!(entry.author, "Feed Author");
        assert_eq!(entry.link, "https://example.com/entry");
        assert_eq!(entry.summary.as_deref(), Some("Entry summary"));
        assert_eq!(
            entry.published,
            Some("2024-07-13T19:09:59Z".parse().unwrap())
        );
        assert_eq!(entry.updated, Some("2024-07-14T19:09:59Z".parse().unwrap()));
    }

    #[test]
    fn rejects_invalid_feed() {
        let error = parse(b"<not-a-feed />").unwrap_err().to_string();
        assert!(error.contains("RSS"));
        assert!(error.contains("Atom"));
    }

    #[test]
    fn test_rss_empty_title_and_description() {
        let feed = parse(
            br#"
            <feed xmlns="http://www.w3.org/2005/Atom"
                  xmlns:media="http://search.example.com/mrss/">
            <title>Test channel</title>
            <id>channel:test</id>
            <entry>
                <id>video:test</id>
                <title>Test video</title>
                <media:group>
                <media:title></media:title>
                <media:description></media:description>
                <media:content url="https://www.example.com/v/test"
                               type="application/x-shockwave-flash"/>
                </media:group>
            </entry>
            </feed>
        "#,
        )
        .unwrap();

        assert_eq!(feed.title.as_deref(), Some("Test channel"));
        let entry = &feed.entries[0];
        assert_eq!(entry.title.as_deref(), Some("Test video"));
    }
}
