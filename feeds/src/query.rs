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

use anyhow as ah;
use querystrong::QueryStrong;

pub struct Query {
    qs: QueryStrong,
}

impl Query {
    pub fn parse(qs: &str) -> ah::Result<Self> {
        Ok(Self {
            qs: QueryStrong::parse(qs)?,
        })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.qs.get_str(key)
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.qs
            .get_str(key)
            .map(|v| v.trim().parse())
            .transpose()
            .ok()
            .flatten()
    }
}

// vim: ts=4 sw=4 expandtab
