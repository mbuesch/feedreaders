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

use crate::GetKv;
use anyhow::{self as ah, Context as _};
use feedsdb::Db;

pub async fn command_getkv(db: &Db, key: &GetKv) -> ah::Result<()> {
    let mut conn = db.open().await.context("Open database")?;

    match key {
        GetKv::FeedUpdateRev => {
            let rev = conn.get_feed_update_revision().await?;
            println!("{rev}");
        }
    }

    Ok(())
}

// vim: ts=4 sw=4 expandtab
