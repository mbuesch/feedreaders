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

use crate::{formfields::FormFields, query::Query, wakeup::wakeup_feedsd};
use anyhow::{self as ah, Context as _};
use feedsdb::{Db, DbConn};
use std::{fmt::Write as _, write as wr, writeln as ln};

const MIME: &str = "text/html";
const BODY_PREALLOC: usize = 1024 * 1024;

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        s.len()
    } else {
        while i > 0 {
            if s.is_char_boundary(i) {
                break;
            }
            i -= 1;
        }
        i
    }
}

fn escape(s: &str, maxlen: usize) -> String {
    let boundary = floor_char_boundary(s, maxlen);
    let mut snipped = s[0..boundary].to_string();
    if snipped.len() != s.len() {
        snipped.push_str("...");
    }
    html_escape::encode_safe(&snipped).into_owned()
}

fn escape_comment(s: &str) -> String {
    s.replace("-->", "_->")
}

#[rustfmt::skip]
async fn gen_feed_list(
    b: &mut String,
    conn: &mut DbConn,
    active_feed_id: Option<i64>,
) -> ah::Result<()> {
    let feeds = conn.get_feeds(active_feed_id).await
        .context("Database: Get feeds")?;

    ln!(b, r#"<div id="feed_list">"#)?;
    ln!(b, r#"  <form method="post" enctype="multipart/form-data">"#)?;
    ln!(b, r#"    <table align="center" id="feed_table">"#)?;
    ln!(b, r#"      <tr>"#)?;
    ln!(b, r#"        <th colspan="2"><a href="/cgi-bin/feeds">feeds</a></th>"#)?;
    ln!(b, r#"      </tr>"#)?;
    for feed in feeds {
        let tr_class = if feed.feed_id == active_feed_id {
            r#" class="active_row""#
        } else {
            ""
        };

        let mut classes = String::new();
        if feed.disabled {
            wr!(&mut classes, "disabled")?;
        }
        if feed.updated_items > 0 {
            if !classes.is_empty() {
                wr!(&mut classes, " ")?;
            }
            wr!(&mut classes, "new_items")?;
        }

        let feed_id = feed.feed_id.expect("get_feeds() feed_id was None");
        let mut title = escape(&feed.title, 32);
        if feed.disabled {
            title.push_str(" (DISABLED)");
        }

        let updated_items = if feed.updated_items > 0 {
            format!(" ({})", feed.updated_items)
        } else {
            "".to_string()
        };

        ln!(b, r#"      <tr{tr_class}>"#)?;
        ln!(b, r#"        <!-- {} -->"#, escape_comment(&title))?;
        ln!(b, r#"        <!-- {} -->"#, escape_comment(&feed.href))?;
        ln!(b, r#"        <td>"#)?;
        ln!(b, r#"          <input name="del" value="{feed_id}" type="checkbox">"#)?;
        ln!(b, r#"        </td>"#)?;
        ln!(b, r#"        <td class="feed_title">"#)?;
        ln!(b, r#"          <span class="{classes}">"#)?;
        ln!(b, r#"            <a href="/cgi-bin/feeds?id={feed_id}">"#)?;
        ln!(b, r#"              {title}"#)?;
        ln!(b, r#"            </a>"#)?;
        ln!(b, r#"          </span>"#)?;
        ln!(b, r#"          {updated_items}"#)?;
        ln!(b, r#"        </td>"#)?;
        ln!(b, r#"      </tr>"#)?;
    }

    ln!(b, r#"    </table>"#)?;
    ln!(b, r#"    <input type="submit" class="button" value="delete">"#)?;
    ln!(b, r#"  </form>"#)?;
    ln!(b, r#"  <form method="post" enctype="multipart/form-data">"#)?;
    ln!(b, r#"    <input name="add" class="button" type="text">"#)?;
    ln!(b, r#"    <input type="submit" class="button" value="add">"#)?;
    ln!(b, r#"  </form>"#)?;
    ln!(b, r#"</div>"#)?;
    Ok(())
}

#[rustfmt::skip]
async fn gen_item_list(
    b: &mut String,
    conn: &mut DbConn,
    feed_id: i64,
) -> ah::Result<()> {
    let items = conn.get_feed_items(feed_id).await
        .context("Database: Get feed items")?;

    ln!(b, r#"<div id="item_list">"#)?;
    for (item, item_ext) in items {
        let item_id = item.item_id.as_ref().expect("get_feed_items() item_id was None");
        let link = escape(&item.link, 1024);
        let title = escape(&item.title, 256);
        let summary = escape(&item.summary, 4096);
        let classes = if item.seen { "item" } else { "item unseen" };
        let author = if item.author.is_empty() {
            "".to_string()
        } else {
            format!("{} - ", escape(&item.author, 32))
        };
        let timestring = item.published.format("%Y-%m-%d %H:%M:%S");
        let mut new_marker = if item.seen { "" } else { "<b>(NEW)</b> " };
        let mut history = String::new();
        if item_ext.count > 1 {
            if item_ext.any_seen && !item_ext.all_seen {
                new_marker = "<b>(updated)</b> ";
            }
            wr!(&mut history, r#"<a class="history" href="/cgi-bin/feeds?"#)?;
            wr!(&mut history, r#"id={feed_id}&itemid={item_id}">(history)</a>"#)?;
        }

        ln!(b, r#"  <div class="{classes}">"#)?;
        ln!(b, r#"    <a class="title" href="{link}">{author}{title}</a>"#)?;
        ln!(b, r#"    {history}"#)?;
        ln!(b, r#"    <br />"#)?;
        ln!(b, r#"    <div class="date">{new_marker}{timestring}</div>"#)?;
        ln!(b, r#"    <br />"#)?;
        ln!(b, r#"    <div class="summary">{summary}</div>"#)?;
        ln!(b, r#"  </div>"#)?;
        ln!(b, r#"  <hr />"#)?;
    }
    ln!(b, r#"</div>"#)?;
    Ok(())
}

#[rustfmt::skip]
async fn gen_item_history_list(
    b: &mut String,
    conn: &mut DbConn,
    feed_id: i64,
    item_id: &str,
) -> ah::Result<()> {
    let items = conn.get_feed_items_by_item_id(feed_id, item_id).await
        .context("Database: Get items by item_id")?;

    ln!(b, r#"<div id="item_list">"#)?;
    for item in items {
        let link = escape(&item.link, 1024);
        let title = escape(&item.title, 256);
        let summary = escape(&item.summary, 4096);
        let classes = if item.seen { "item" } else { "item unseen" };
        let author = if item.author.is_empty() {
            "".to_string()
        } else {
            format!("{} - ", escape(&item.author, 32))
        };
        let timestring = item.retrieved.format("%Y-%m-%d %H:%M:%S");

        ln!(b, r#"  <div class="{classes}">"#)?;
        ln!(b, r#"    <a class="title" href="{link}">{author}{title}</a>"#)?;
        ln!(b, r#"    <br />"#)?;
        ln!(b, r#"    <div class="date">{timestring}</div>"#)?;
        ln!(b, r#"    <br />"#)?;
        ln!(b, r#"    <div class="summary">{summary}</div>"#)?;
        ln!(b, r#"  </div>"#)?;
        ln!(b, r#"  <hr />"#)?;
    }
    ln!(b, r#"</div>"#)?;
    Ok(())
}

#[rustfmt::skip]
async fn gen_page(
    b: &mut String,
    conn: &mut DbConn,
    query: &Query,
    formfields: Option<&FormFields>,
) -> ah::Result<()> {
    let mut wake_feedsd = false;

    ln!(b, r#"<!DOCTYPE HTML>"#)?;
    ln!(b, r#"<html lang="en">"#)?;
    ln!(b, r#"<head>"#)?;
    ln!(b, r#"  <title>My Feeds</title>"#)?;
    ln!(b, r#"  <link rel="stylesheet" type="text/css" href="/feeds/style.css">"#)?;
    ln!(b, r#"  <link rel="icon" type="image/png" href="/feeds/icon.png">"#)?;
    ln!(b, r#"  <meta http-equiv="Content-Type" content="text/html; charset=UTF-8">"#)?;
    ln!(b, r#"  <meta name="generator" content="feedreader (Rust variant)">"#)?;
    ln!(b, r#"</head>"#)?;
    ln!(b, r#"<body>"#)?;

    if let Some(formfields) = formfields {
        if let Some(add_href) = formfields.get_one("add") {
            conn.add_feed(add_href).await
                .context("Database: Add feed")?;
            wake_feedsd = true;
        }
        if let Some(del_ids) = formfields.get_list_i64("del") {
            conn.delete_feeds(&del_ids).await
                .context("Database: Delete feeds")?;
        }
    }

    let feed_id = query.get_i64("id");
    let item_id = query.get("itemid");

    gen_feed_list(b, conn, feed_id).await?;

    if let Some(feed_id) = feed_id {
        if let Some(item_id) = &item_id {
            gen_item_history_list(b, conn, feed_id, item_id).await?;
        } else {
            gen_item_list(b, conn, feed_id).await?;
        }
    }

    ln!(b, r#"</body>"#)?;
    ln!(b, r#"</html>"#)?;

    if wake_feedsd {
        wakeup_feedsd().await;
    }

    Ok(())
}

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum GetBody {
    No,
    Yes,
}

#[derive(PartialEq, Eq, Clone)]
pub struct PageGenResult {
    pub body: String,
    pub mime: String,
}

pub struct PageGen<'a> {
    db: &'a Db,
}

impl<'a> PageGen<'a> {
    pub async fn new(db: &'a Db) -> ah::Result<Self> {
        Ok(Self { db })
    }

    pub async fn get(&mut self, query: &Query, get_body: GetBody) -> ah::Result<PageGenResult> {
        let body = match get_body {
            GetBody::Yes => {
                let mut body = String::with_capacity(BODY_PREALLOC);
                let mut conn = self.db.open().await.context("Open database")?;
                gen_page(&mut body, &mut conn, query, None)
                    .await
                    .context("Generate page (GET)")?;
                body
            }
            GetBody::No => "".to_string(),
        };

        Ok(PageGenResult {
            body,
            mime: MIME.to_string(),
        })
    }

    pub async fn post(
        &mut self,
        query: &Query,
        formfields: &FormFields,
    ) -> ah::Result<PageGenResult> {
        let mut body = String::with_capacity(BODY_PREALLOC);
        let mut conn = self.db.open().await.context("Open database")?;
        gen_page(&mut body, &mut conn, query, Some(formfields))
            .await
            .context("Generate page (POST)")?;
        Ok(PageGenResult {
            body,
            mime: MIME.to_string(),
        })
    }
}

// vim: ts=4 sw=4 expandtab
