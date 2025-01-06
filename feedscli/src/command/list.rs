// -*- coding: utf-8 -*-
//
// Copyright (C) 2024-2025 Michael Büsch <m@bues.ch>
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

use anyhow::{self as ah, Context as _};
use feedsdb::Db;

pub async fn command_list(db: &Db) -> ah::Result<()> {
    let (mut feeds, _) = db
        .open()
        .await
        .context("Open database")?
        .get_feeds(None)
        .await
        .context("Database: Get feeds")?;

    feeds.sort_unstable_by(|a, b| a.feed_id.cmp(&b.feed_id));

    for feed in &feeds {
        println!("{}", feed.title);
        if feed.disabled {
            println!("  DISABLED");
        }
        println!("  href           = {}", feed.href);
        println!("  last-activity  = {}", feed.last_activity);
        println!("  last-retrieval = {}", feed.last_retrieval);
        println!("  next-retrieval = {}", feed.next_retrieval);
        println!("  updated-items  = {}", feed.updated_items);
        println!("  feed-id        = {}", feed.feed_id.expect("No feed id"));
        println!();
    }
    println!("{} feeds total", feeds.len());

    Ok(())
}

// vim: ts=4 sw=4 expandtab
