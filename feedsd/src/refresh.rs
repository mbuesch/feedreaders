// -*- coding: utf-8 -*-
//
// Copyright (C) 2024-2025 Michael Büsch <m@bues.ch>
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

use anyhow::{self as ah, Context as _, format_err as err};
use chrono::{DateTime, Utc};
use feed_rs::model::Feed as ParsedFeed;
use feedscfg::Config;
use feedsdb::{Db, DbConn, Feed, Item, ItemStatus};
use rand::{prelude::*, rng};
use regex::Regex;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::Semaphore,
    task::{self, JoinSet},
};

fn rand_interval(refresh_interval: Duration, slack_rel: f64) -> Duration {
    let slack = (refresh_interval.as_millis() as f64 * slack_rel) as u64;
    let a = refresh_interval.as_millis() as u64 - (slack / 2);
    let b = refresh_interval.as_millis() as u64 + (slack / 2);
    Duration::from_millis(rng().random_range(a..b))
}

enum FeedResult {
    Feed(Box<ParsedFeed>),
    MovedPermanently(Option<String>),
    Gone,
}

async fn get_feed(config: &Config, href: &str) -> ah::Result<FeedResult> {
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
        .timeout(config.net.timeout)
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
        Ok(parser.parse(&*feed_bytes).map(Box::new)?)
    })
    .await?;

    let feed = feed.map_err(|e| err!("Failed to parse feed '{href}': {e}"))?;

    //TODO: If a feed fails to parse too often, disable it.

    Ok(FeedResult::Feed(feed))
}

fn highlight_re_matches(name: &str, s: &str, re: &Regex) -> bool {
    let matches = re.is_match(s);
    if matches {
        log::debug!("no-highlighting rule {name}/{re} matches '{s}'.");
    }
    matches
}

fn should_highlight(config: &Config, item: &Item) -> bool {
    if config
        .no_highlighting
        .title
        .iter()
        .any(|re| highlight_re_matches("title", &item.title, re))
    {
        return false;
    }
    if config
        .no_highlighting
        .summary
        .iter()
        .any(|re| highlight_re_matches("summary", &item.summary, re))
    {
        return false;
    }
    if config
        .no_highlighting
        .url
        .iter()
        .any(|re| highlight_re_matches("url", &item.link, re))
    {
        return false;
    }
    true
}

struct FilteredItem {
    item: Item,
    status: ItemStatus,
    highlight: bool,
}

async fn get_items(
    config: &Config,
    conn: &mut DbConn,
    parsed_feed: &ParsedFeed,
    now: DateTime<Utc>,
) -> ah::Result<(Vec<FilteredItem>, DateTime<Utc>)> {
    let mut items = Vec::with_capacity(16);
    let mut oldest = now;
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
            let Some(fefeid) = feed_item_id.split('=').next_back() else {
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

        if published < oldest {
            oldest = published;
        }

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
        item.item_id = Some(item.make_id().await);

        match conn
            .check_item_exists(&item)
            .await
            .context("Check item exists")?
        {
            ItemStatus::Exists => (),
            s @ ItemStatus::New | s @ ItemStatus::Updated => {
                let highlight = should_highlight(config, &item);
                if !highlight && config.no_highlighting.set_seen {
                    item.seen = true;
                }
                let fil_item = FilteredItem {
                    item,
                    status: s,
                    highlight,
                };
                items.push(fil_item);
            }
        }
    }
    Ok((items, oldest))
}

async fn refresh_feed(
    config: Arc<Config>,
    db: Arc<Db>,
    mut feed: Feed,
    next_retrieval: DateTime<Utc>,
    net_sema: Arc<Semaphore>,
) -> ah::Result<()> {
    log::debug!("Refreshing {} ...", feed.title);

    let parsed_feed = {
        let _permit = net_sema.acquire().await?;

        match get_feed(&config, &feed.href).await? {
            FeedResult::Feed(f) => f,
            FeedResult::MovedPermanently(location) => {
                if let Some(location) = location {
                    feed.href = location;
                } else {
                    feed.disabled = true;
                }
                db.open()
                    .await
                    .context("Open database")?
                    .update_feed(&feed, &[], None, true)
                    .await
                    .context("Update feed")?;
                return Ok(());
            }
            FeedResult::Gone => {
                feed.disabled = true;
                db.open()
                    .await
                    .context("Open database")?
                    .update_feed(&feed, &[], None, true)
                    .await
                    .context("Update feed")?;
                return Ok(());
            }
        }
    };

    let now = Utc::now();
    let mut conn = db.open().await.context("Open database")?;
    let (items, oldest) = get_items(&config, &mut conn, &parsed_feed, now).await?;

    let new_items_count: i64 = items
        .iter()
        .map(|i| (i.status == ItemStatus::New && i.highlight) as i64)
        .sum();

    if let Some(title) = parsed_feed.title.as_ref() {
        feed.title = title.content.clone();
    }
    feed.last_retrieval = now;
    feed.next_retrieval = next_retrieval;

    let mut increment_update_revision = false;
    if !items.is_empty() {
        feed.last_activity = now;
        if config.db.highlight_updated_items {
            feed.updated_items += items.len() as i64;
            increment_update_revision = true;
        } else {
            feed.updated_items += new_items_count;
            if new_items_count > 0 {
                increment_update_revision = true;
            }
        }
    }

    let gc_thres = oldest - config.db.gc_age_offset;

    let items: Vec<Item> = items.into_iter().map(|i| i.item).collect();
    conn.update_feed(&feed, &items, Some(gc_thres), increment_update_revision)
        .await
        .context("Update feed")?;

    Ok(())
}

pub async fn refresh_feeds(config: Arc<Config>, db: Arc<Db>) -> ah::Result<Duration> {
    let next_retrieval =
        Utc::now() + rand_interval(config.db.refresh_interval, config.db.refresh_slack);

    let feeds_due = db
        .open()
        .await
        .context("Open database")?
        .get_feeds_due()
        .await
        .context("Get feeds due")?;

    let net_sema = Arc::new(Semaphore::new(config.net.concurrency.into()));

    let mut set = JoinSet::new();
    for feed in feeds_due {
        set.spawn({
            let config = Arc::clone(&config);
            let db = Arc::clone(&db);
            let net_sema = Arc::clone(&net_sema);
            async move { refresh_feed(config, db, feed, next_retrieval, net_sema).await }
        });
    }
    while let Some(result) = set.join_next().await {
        let _: () = result??;
    }

    let next_due = db
        .open()
        .await
        .context("Open database")?
        .get_next_due_time()
        .await
        .context("Update feed")?;
    let dur = (next_due - Utc::now()).num_milliseconds().max(0);
    let sleep_dur = Duration::from_millis(dur.try_into().unwrap());
    let sleep_dur = sleep_dur + Duration::from_secs(1);

    Ok(sleep_dur)
}

// vim: ts=4 sw=4 expandtab
