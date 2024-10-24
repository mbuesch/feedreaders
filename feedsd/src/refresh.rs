// -*- coding: utf-8 -*-
//
// Copyright (C) 2024 Michael Büsch <m@bues.ch>
// Copyright (C) 2020 Marco Lochen
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// SPDX-License-Identifier: GPL-2.0-or-later

use anyhow::{self as ah, format_err as err, Context as _};
use chrono::{DateTime, Utc};
use feed_rs::model::{Entry as ParsedEntry, Feed as ParsedFeed};
use feedsdb::{make_item_id, Db, DbConn, Enclosure, Item};
use rand::{thread_rng, Rng as _};
use std::time::Duration;
use tokio::task;

const NET_TIMEOUT: Duration = Duration::from_secs(10);
const REFRESH_SLACK: f64 = 0.1;

fn rand_interval(refresh_interval: Duration, slack_rel: f64) -> Duration {
    let slack = (refresh_interval.as_millis() as f64 * slack_rel) as u64;
    let a = refresh_interval.as_millis() as u64 - (slack / 2);
    let b = refresh_interval.as_millis() as u64 + (slack / 2);
    Duration::from_millis(thread_rng().gen_range(a..b))
}

enum FeedResult {
    Feed(Box<ParsedFeed>),
    MovedPermanently(Option<String>),
    Gone,
}

async fn get_feed(href: &str) -> ah::Result<FeedResult> {
    use feed_rs::parser;
    use reqwest::{Client, StatusCode};

    let user_agent = concat!(
        "feedreader/",
        env!("CARGO_PKG_VERSION"),
        " (feedreader; Rust variant)"
    );
    let client = Client::builder()
        .user_agent(user_agent)
        .referer(false)
        .timeout(NET_TIMEOUT)
        .build()
        .context("Retrieve feed")?;

    let feed_resp = client.get(href).send().await.context("Retrieve feed")?;

    match feed_resp.status() {
        StatusCode::OK => (),
        StatusCode::MOVED_PERMANENTLY => {
            let mut location = feed_resp
                .headers()
                .get("Location")
                .map(|l| l.to_str().unwrap_or_default().to_string());
            if let Some(l) = location.as_ref() {
                if l.trim().is_empty() {
                    location = None;
                }
            }
            return Ok(FeedResult::MovedPermanently(location));
        }
        StatusCode::GONE => {
            return Ok(FeedResult::Gone);
        }
        code => {
            return Err(err!("Feed fetch error: {code}"));
        }
    }

    let feed_bytes = feed_resp.bytes().await.context("Retrieve feed")?;

    let feed = task::spawn_blocking(move || -> ah::Result<Box<ParsedFeed>> {
        let parser = parser::Builder::new().build();
        let parsed_feed = Box::new(parser.parse(&*feed_bytes)?);
        Ok(parsed_feed)
    })
    .await
    .context("Parse feed")??;

    Ok(FeedResult::Feed(feed))
}

async fn get_enclosures(_parsed_entry: &ParsedEntry) -> ah::Result<Vec<Enclosure>> {
    Ok(vec![]) //TODO
}

async fn get_items(
    conn: &mut DbConn,
    parsed_feed: &ParsedFeed,
    now: DateTime<Utc>,
) -> ah::Result<Vec<(Item, Vec<Enclosure>)>> {
    let mut items = Vec::with_capacity(16);
    for parsed_entry in &parsed_feed.entries {
        let feed_item_id = parsed_entry.id.clone();

        let author = itertools::join(parsed_entry.authors.iter().map(|a| &a.name), ", ");

        let title = parsed_entry
            .title
            .as_ref()
            .map(|t| t.content.clone())
            .unwrap_or_default();

        let link = parsed_entry
            .links
            .iter()
            .map(|l| l.href.clone())
            .next()
            .unwrap_or_default();

        let published = if let Some(published) = &parsed_entry.published {
            *published
        } else if let Some(updated) = &parsed_entry.updated {
            *updated
        } else if feed_item_id.contains("blog.fefe.de") {
            // Fefe-workaround :-/
            let Some(fefeid) = feed_item_id.split('=').last() else {
                continue;
            };
            let Ok(fefeid) = i64::from_str_radix(fefeid, 16) else {
                continue;
            };
            let stamp = fefeid ^ 0xfefec0de;
            let Some(stamp) = DateTime::<Utc>::from_timestamp(stamp, 0) else {
                continue;
            };
            stamp
        } else {
            now
        };

        let mut summary = parsed_entry
            .summary
            .as_ref()
            .map(|s| s.content.clone())
            .unwrap_or_default();
        if summary.trim().is_empty() {
            for media in &parsed_entry.media {
                if let Some(description) = &media.description {
                    summary = description.content.clone();
                    break;
                }
            }
        }

        let enclosures = get_enclosures(parsed_entry).await?;

        let mut item = Item {
            item_id: None,
            feed_id: None,
            retrieved: now,
            seen: false,
            author,
            title,
            feed_item_id,
            link,
            published,
            summary,
        };
        item.item_id = Some(make_item_id(&item, &enclosures).await);

        if !conn
            .check_item_exists(&item)
            .await
            .context("Check item exists")?
        {
            items.push((item, enclosures));
        }
    }
    Ok(items)
}

pub async fn refresh_feeds(db: &Db, refresh_interval: Duration) -> ah::Result<()> {
    let mut conn = db.open().await.context("Open database")?;

    for mut feed in conn.get_feeds_due().await.context("Get feeds due")? {
        let parsed_feed = match get_feed(&feed.href).await? {
            FeedResult::Feed(f) => f,
            FeedResult::MovedPermanently(location) => {
                if let Some(location) = location {
                    feed.href = location;
                } else {
                    feed.disabled = true;
                }
                conn.update_feed(&feed, &[]).await.context("Update feed")?;
                continue;
            }
            FeedResult::Gone => {
                feed.disabled = true;
                conn.update_feed(&feed, &[]).await.context("Update feed")?;
                continue;
            }
        };

        let now = Utc::now();

        let items = get_items(&mut conn, &parsed_feed, now).await?;

        if let Some(title) = parsed_feed.title.as_ref() {
            feed.title = title.content.clone();
        }
        feed.last_retrieval = now;
        feed.next_retrieval = now + rand_interval(refresh_interval, REFRESH_SLACK);

        if !items.is_empty() {
            feed.last_activity = now;
            feed.updated_items += items.len() as i64;
        }

        conn.update_feed(&feed, &items)
            .await
            .context("Update feed")?;
    }
    Ok(())
}

// vim: ts=4 sw=4 expandtab
