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

#![forbid(unsafe_code)]

mod error;

use crate::error::Error;
use anyhow::{self as ah, format_err as err, Context as _};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, Row};
use sha2::{Digest as _, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::task::spawn_blocking;

pub const DEBUG: bool = true;
const TIMEOUT: Duration = Duration::from_millis(10_000);

pub fn get_prefix() -> PathBuf {
    option_env!("FEEDREADER_PREFIX").unwrap_or("/").into()
}

pub fn get_varlib() -> PathBuf {
    get_prefix().join("var/lib/feedreader")
}

fn sql_to_dt(timestamp: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now)
}

fn dt_to_sql(dt: &DateTime<Utc>) -> i64 {
    dt.timestamp()
}

#[derive(Clone, Debug)]
pub struct Feed {
    pub feed_id: Option<i64>,
    pub href: String,
    pub title: String,
    pub last_retrieval: DateTime<Utc>,
    pub next_retrieval: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub disabled: bool,
    pub updated_items: i64,
}

impl Feed {
    fn from_sql_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            feed_id: Some(row.get(0)?),
            href: row.get(1)?,
            title: row.get(2)?,
            last_retrieval: sql_to_dt(row.get(3)?),
            next_retrieval: sql_to_dt(row.get(4)?),
            last_activity: sql_to_dt(row.get(5)?),
            disabled: row.get(6)?,
            updated_items: row.get(7)?,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub item_id: Option<String>,
    pub feed_id: Option<i64>,
    pub retrieved: DateTime<Utc>,
    pub seen: bool,
    pub author: String,
    pub title: String,
    pub feed_item_id: String,
    pub link: String,
    pub published: DateTime<Utc>,
    pub summary: String,
}

impl Item {
    fn from_sql_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            item_id: Some(row.get(0)?),
            feed_id: Some(row.get(1)?),
            retrieved: sql_to_dt(row.get(2)?),
            seen: row.get(3)?,
            author: row.get(4)?,
            title: row.get(5)?,
            feed_item_id: row.get(6)?,
            link: row.get(7)?,
            published: sql_to_dt(row.get(8)?),
            summary: row.get(9)?,
        })
    }

    fn from_sql_row_with_count(row: &Row<'_>) -> rusqlite::Result<(Self, i64)> {
        Ok((Self::from_sql_row(row)?, row.get(10)?))
    }

    pub async fn make_id(&self) -> String {
        let mut h = Sha256::new();
        h.update(&self.feed_item_id);
        h.update(&self.author);
        h.update(&self.title);
        h.update(&self.link);
        h.update(format!("{}", self.published));
        h.update(&self.summary);
        hex::encode(h.finalize())
    }
}

async fn transaction<F, R>(conn: Arc<Mutex<Connection>>, mut f: F) -> ah::Result<R>
where
    F: FnMut(rusqlite::Transaction) -> Result<R, Error> + Send + 'static,
    R: Send + 'static,
{
    spawn_blocking(move || {
        let timeout = Instant::now() + TIMEOUT;
        loop {
            let mut conn = conn.lock().expect("Mutex poisoned");
            let trans = conn.transaction()?;
            match f(trans) {
                Ok(r) => {
                    break Ok(r);
                }
                Err(Error::Sql(
                    e @ rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error {
                            code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                            ..
                        },
                        ..,
                    ),
                )) => {
                    drop(conn); // unlock
                    if Instant::now() >= timeout {
                        break Err(e.into());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    break Err(e.into());
                }
            }
        }
    })
    .await?
}

pub struct DbConn {
    conn: Arc<Mutex<Connection>>,
}

impl DbConn {
    async fn new(path: &Path) -> ah::Result<Self> {
        let path = path.to_path_buf();

        let conn = spawn_blocking(move || -> ah::Result<Connection> {
            let timeout = Instant::now() + TIMEOUT;

            loop {
                let conn = match Connection::open_with_flags(
                    &path,
                    OpenFlags::SQLITE_OPEN_READ_WRITE
                        | OpenFlags::SQLITE_OPEN_CREATE
                        | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                ) {
                    Ok(conn) => conn,
                    Err(
                        e @ rusqlite::Error::SqliteFailure(
                            rusqlite::ffi::Error {
                                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                                ..
                            },
                            ..,
                        ),
                    ) => {
                        if Instant::now() >= timeout {
                            break Err(e.into());
                        }
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    Err(e) => {
                        break Err(e.into());
                    }
                };
                conn.busy_timeout(TIMEOUT)?;
                conn.set_prepared_statement_cache_capacity(64);
                break Ok(conn);
            }
        })
        .await?
        .context("Open SQLite database")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    #[rustfmt::skip]
    pub async fn init(&mut self) -> ah::Result<()> {
        transaction(Arc::clone(&self.conn), move |t| {
            t.execute(
                "\
                    CREATE TABLE IF NOT EXISTS feeds (\
                        feed_id INTEGER PRIMARY KEY, \
                        href VARCHAR, \
                        title VARCHAR, \
                        last_retrieval TIMESTAMP, \
                        next_retrieval TIMESTAMP, \
                        last_activity TIMESTAMP, \
                        disabled BOOLEAN, \
                        updated_items INTEGER\
                    )",
                [],
            )?;
            t.execute(
                "\
                    CREATE TABLE IF NOT EXISTS items (\
                        item_id VARCHAR PRIMARY KEY, \
                        feed_id INTEGER, \
                        retrieved TIMESTAMP, \
                        seen BOOLEAN, \
                        author VARCHAR, \
                        title VARCHAR, \
                        feed_item_id VARCHAR, \
                        link VARCHAR, \
                        published TIMESTAMP, \
                        summary VARCHAR, \
                        FOREIGN KEY(feed_id) REFERENCES feeds(feed_id)\
                    )",
                [],
            )?;
            t.execute("CREATE INDEX IF NOT EXISTS feed_id ON feeds(feed_id)", [])?;
            t.execute("CREATE INDEX IF NOT EXISTS item_id ON items(item_id)", [])?;

            // Remove legacy table.
            t.execute("DROP TABLE IF EXISTS enclosures", [])?;

            // Remove dangling items.
            t.execute(
                "\
                    DELETE FROM items \
                    WHERE feed_id NOT IN (\
                        SELECT feed_id FROM feeds\
                    )\
                ",
                []
            )?;

            t.commit()?;
            Ok(())
        }).await?;

        {
            let conn = self.conn.lock().expect("Mutex poisoned");
            conn.execute("VACUUM", [])?;
        }

        Ok(())
    }

    pub async fn update_feed(
        &mut self,
        feed: &Feed,
        items: &[Item],
        gc_thres: Option<DateTime<Utc>>,
    ) -> ah::Result<()> {
        let feed = feed.clone();
        let items = items.to_vec();

        transaction(Arc::clone(&self.conn), move |t| {
            let Some(feed_id) = feed.feed_id else {
                return Err(Error::Ah(err!("update_feed(): Invalid feed. No feed_id.")));
            };
            t.prepare_cached(
                "\
                    UPDATE feeds SET \
                        href = ?,
                        title = ?,
                        last_retrieval = ?,
                        next_retrieval = ?,
                        last_activity = ?,
                        disabled = ?,
                        updated_items = ?
                    WHERE feed_id = ?\
                ",
            )?
            .execute((
                &feed.href,
                &feed.title,
                dt_to_sql(&feed.last_retrieval),
                dt_to_sql(&feed.next_retrieval),
                dt_to_sql(&feed.last_activity),
                feed.disabled,
                feed.updated_items,
                feed_id,
            ))?;

            for item in &items {
                let Some(item_id) = &item.item_id else {
                    return Err(Error::Ah(err!("update_feed(): Invalid item. No item_id.")));
                };
                if item.feed_id.is_some() && item.feed_id != Some(feed_id) {
                    return Err(Error::Ah(err!(
                        "update_feed(): Invalid item. Invalid feed_id."
                    )));
                }
                t.prepare_cached(
                    "\
                        INSERT INTO items \
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)\
                    ",
                )?
                .execute((
                    item_id,
                    feed_id,
                    dt_to_sql(&item.retrieved),
                    item.seen,
                    &item.author,
                    &item.title,
                    &item.feed_item_id,
                    &item.link,
                    dt_to_sql(&item.published),
                    &item.summary,
                ))?;
            }

            if let Some(gc_thres) = gc_thres.as_ref() {
                t.prepare_cached(
                    "\
                        DELETE FROM items \
                        WHERE feed_id = ? AND published < ? AND seen = TRUE\
                    ",
                )?
                .execute((feed_id, dt_to_sql(gc_thres)))?;
            }

            t.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn add_feed(&mut self, href: &str) -> ah::Result<()> {
        let href = href.to_string();

        transaction(Arc::clone(&self.conn), move |t| {
            t.prepare_cached(
                "\
                    INSERT INTO feeds \
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?)\
                ",
            )?
            .execute((
                None::<i64>,
                &href,
                "[New feed] Updating...",
                0,
                0,
                0,
                false,
                0,
            ))?;

            t.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn delete_feeds(&mut self, feed_ids: &[i64]) -> ah::Result<()> {
        if !feed_ids.is_empty() {
            let feed_ids = feed_ids.to_vec();

            transaction(Arc::clone(&self.conn), move |t| {
                for feed_id in &feed_ids {
                    t.prepare_cached(
                        "\
                            DELETE FROM items \
                            WHERE feed_id = ?\
                        ",
                    )?
                    .execute([feed_id])?;
                    t.prepare_cached(
                        "\
                            DELETE FROM feeds \
                            WHERE feed_id = ?\
                        ",
                    )?
                    .execute([feed_id])?;
                }

                t.commit()?;
                Ok(())
            })
            .await
        } else {
            Ok(())
        }
    }

    pub async fn get_feeds_due(&mut self) -> ah::Result<Vec<Feed>> {
        let now = Utc::now();

        transaction(Arc::clone(&self.conn), move |t| {
            let feeds: Vec<Feed> = t
                .prepare_cached(
                    "\
                        SELECT * FROM feeds \
                        WHERE next_retrieval < ? AND \
                              disabled == FALSE\
                    ",
                )?
                .query_map([dt_to_sql(&now)], Feed::from_sql_row)?
                .map(|f| f.unwrap())
                .collect();

            t.finish()?;
            Ok(feeds)
        })
        .await
    }

    pub async fn get_next_due_time(&mut self) -> ah::Result<DateTime<Utc>> {
        transaction(Arc::clone(&self.conn), move |t| {
            let next_retrieval = t
                .prepare_cached(
                    "\
                        SELECT min(next_retrieval) FROM feeds \
                        WHERE disabled == FALSE\
                    ",
                )?
                .query([])?
                .next()?
                .unwrap()
                .get(0)?;

            t.finish()?;
            Ok(sql_to_dt(next_retrieval))
        })
        .await
    }

    pub async fn get_feeds(&mut self, active_feed_id: Option<i64>) -> ah::Result<Vec<Feed>> {
        transaction(Arc::clone(&self.conn), move |t| {
            if let Some(active_feed_id) = active_feed_id {
                t.prepare_cached(
                    "\
                        UPDATE feeds \
                        SET updated_items = 0 \
                        WHERE feed_id = ?\
                    ",
                )?
                .execute([active_feed_id])?;
            }

            let feeds: Vec<Feed> = t
                .prepare_cached(
                    "\
                        SELECT * FROM feeds \
                        ORDER BY last_activity DESC\
                    ",
                )?
                .query_map([], Feed::from_sql_row)?
                .map(|f| f.unwrap())
                .collect();

            if active_feed_id.is_some() {
                t.commit()?;
            } else {
                t.finish()?;
            }
            Ok(feeds)
        })
        .await
    }

    pub async fn get_feed_items(&mut self, feed_id: i64) -> ah::Result<Vec<(Item, i64)>> {
        transaction(Arc::clone(&self.conn), move |t| {
            let items: Vec<(Item, i64)> = t
                .prepare_cached(
                    "\
                        SELECT item_id, feed_id, max(retrieved), seen, \
                        author, title, feed_item_id, link, published, \
                        summary, count() as count \
                        FROM items \
                        WHERE feed_id = ? \
                        GROUP BY feed_item_id \
                        ORDER BY published DESC LIMIT 100\
                    ",
                )?
                .query_map([feed_id], Item::from_sql_row_with_count)?
                .map(|i| i.unwrap())
                .collect();

            t.prepare_cached(
                "\
                    UPDATE items \
                    SET seen = TRUE \
                    WHERE feed_id = ?\
                ",
            )?
            .execute([feed_id])?;

            t.commit()?;
            Ok(items)
        })
        .await
    }

    pub async fn get_feed_items_by_item_id(
        &mut self,
        feed_id: i64,
        item_id: &str,
    ) -> ah::Result<Vec<Item>> {
        let item_id = item_id.to_string();

        transaction(Arc::clone(&self.conn), move |t| {
            let items: Vec<Item> = t
                .prepare_cached(
                    "\
                        SELECT * FROM items \
                        WHERE feed_id = ? AND feed_item_id IN (\
                            SELECT feed_item_id FROM items \
                            WHERE item_id = ?\
                        ) \
                        ORDER BY retrieved DESC\
                    ",
                )?
                .query_map((feed_id, &item_id), Item::from_sql_row)?
                .map(|i| i.unwrap())
                .collect();

            t.prepare_cached(
                "\
                    UPDATE items \
                    SET seen = TRUE \
                    WHERE feed_id = ?\
                ",
            )?
            .execute([feed_id])?;

            t.commit()?;
            Ok(items)
        })
        .await
    }

    pub async fn check_item_exists(&mut self, item: &Item) -> ah::Result<bool> {
        if let Some(item_id) = item.item_id.as_ref() {
            let item_id = item_id.to_string();

            transaction(Arc::clone(&self.conn), move |t| {
                let count: Vec<i64> = t
                    .prepare_cached(
                        "\
                            SELECT count(item_id) \
                            FROM items \
                            WHERE item_id = ?\
                        ",
                    )?
                    .query_map([&item_id], |row| row.get(0))?
                    .map(|c| c.unwrap())
                    .collect();
                let exists = *count.first().unwrap_or(&0) > 0;

                t.finish()?;
                Ok(exists)
            })
            .await
        } else {
            Err(err!("check_item_exists(): Invalid item. No item_id."))
        }
    }
}

pub struct Db {
    path: PathBuf,
}

impl Db {
    pub async fn new(name: &str) -> ah::Result<Self> {
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(err!("Invalid name"));
        }
        let path = get_varlib().join(format!("{name}.db"));
        Ok(Self { path })
    }

    pub async fn open(&self) -> ah::Result<DbConn> {
        DbConn::new(&self.path).await
    }
}

// vim: ts=4 sw=4 expandtab
