// -*- coding: utf-8 -*-
//
// Copyright (C) 2024 Michael Büsch <m@bues.ch>
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
use multer::{parse_boundary, Constraints, Multipart, SizeLimit};
use std::collections::HashMap;

const LIMIT_WHOLE_STREAM: u64 = 1024 * 128;
const LIMIT_PER_FIELD: u64 = 1024 * 4;

pub struct FormFields {
    items: HashMap<String, Vec<String>>,
}

impl FormFields {
    pub async fn new(body: &[u8], body_mime: &str) -> ah::Result<Self> {
        // Parse the form data.
        let boundary = parse_boundary(body_mime).context("Parse form-data boundary")?;
        let sizelim = SizeLimit::new()
            .whole_stream(LIMIT_WHOLE_STREAM)
            .per_field(LIMIT_PER_FIELD);
        let constr = Constraints::new().size_limit(sizelim);
        let mut multipart = Multipart::with_reader_with_constraints(body, boundary, constr);

        // Put the form data into a HashMap.
        let mut items: HashMap<_, Vec<String>> = HashMap::with_capacity(8);
        while let Some(field) = multipart.next_field().await.context("Multipart field")? {
            let Some(name) = field.name() else {
                continue;
            };
            let name = name.to_string();
            let Ok(data) = field.bytes().await else {
                continue;
            };
            let Ok(data) = String::from_utf8(data.to_vec()) else {
                continue;
            };
            items.entry(name).or_default().push(data);
        }
        Ok(Self { items })
    }

    pub fn get_one(&self, key: &str) -> Option<&String> {
        self.items.get(key).and_then(|l| l.iter().last())
    }

    pub fn get_list(&self, key: &str) -> Option<&[String]> {
        self.items.get(key).map(|l| &**l)
    }

    pub fn get_list_i64(&self, key: &str) -> Option<Vec<i64>> {
        self.get_list(key).map(|l| {
            l.iter()
                .filter_map(|v| v.trim().parse::<i64>().ok())
                .collect()
        })
    }
}

// vim: ts=4 sw=4 expandtab
