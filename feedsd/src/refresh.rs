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
use feed_rs::model::Feed as ParsedFeed;
use feedsdb::{Db, DbConn, Feed, Item, DEBUG};
use rand::{thread_rng, Rng as _};
use std::{sync::Arc, time::Duration};
use tokio::task::{self, JoinSet};

const NET_TIMEOUT: Duration = Duration::from_secs(10);
const REFRESH_SLACK: f64 = 0.1;
const GC_AGE_OFFSET: Duration = Duration::from_secs(365 * 24 * 60 * 60); // 1 year

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

async fn get_items(
    conn: &mut DbConn,
    parsed_feed: &ParsedFeed,
    now: DateTime<Utc>,
) -> ah::Result<(Vec<Item>, DateTime<Utc>)> {
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

        if !conn
            .check_item_exists(&item)
            .await
            .context("Check item exists")?
        {
            items.push(item);
        }
    }
    Ok((items, oldest))
}

async fn refresh_feed(
    db: Arc<Db>,
    mut feed: Feed,
    next_retrieval: DateTime<Utc>,
) -> ah::Result<()> {
    if DEBUG {
        println!("Refreshing {} ...", feed.title);
    }

    let parsed_feed = match get_feed(&feed.href).await? {
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
                .update_feed(&feed, &[], None)
                .await
                .context("Update feed")?;
            return Ok(());
        }
        FeedResult::Gone => {
            feed.disabled = true;
            db.open()
                .await
                .context("Open database")?
                .update_feed(&feed, &[], None)
                .await
                .context("Update feed")?;
            return Ok(());
        }
    };

    let now = Utc::now();
    let mut conn = db.open().await.context("Open database")?;
    let (items, oldest) = get_items(&mut conn, &parsed_feed, now).await?;

    if let Some(title) = parsed_feed.title.as_ref() {
        feed.title = title.content.clone();
    }
    feed.last_retrieval = now;
    feed.next_retrieval = next_retrieval;

    if !items.is_empty() {
        feed.last_activity = now;
        feed.updated_items += items.len() as i64;
    }

    let gc_thres = oldest - GC_AGE_OFFSET;

    conn.update_feed(&feed, &items, Some(gc_thres))
        .await
        .context("Update feed")?;

    Ok(())
}

pub async fn refresh_feeds(db: Arc<Db>, refresh_interval: Duration) -> ah::Result<Duration> {
    let next_retrieval = Utc::now() + rand_interval(refresh_interval, REFRESH_SLACK);

    let feeds_due = db
        .open()
        .await
        .context("Open database")?
        .get_feeds_due()
        .await
        .context("Get feeds due")?;

    let mut set = JoinSet::new();
    for feed in feeds_due {
        set.spawn({
            let db = Arc::clone(&db);
            async move { refresh_feed(db, feed, next_retrieval).await }
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
