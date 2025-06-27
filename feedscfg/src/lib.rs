// -*- coding: utf-8 -*-
//
// Copyright (C) 2025 Michael Büsch <m@bues.ch>
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
use regex::Regex;
use std::path::Path;
use toml::{Table, Value};

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub title_deny_highlighting: Vec<Regex>,
    pub summary_deny_highlighting: Vec<Regex>,
}

impl Config {
    fn new() -> Self {
        Default::default()
    }

    pub fn parse_default_file() -> ah::Result<Self> {
        Self::parse_file(Path::new("/opt/feedreader/etc/feedreader/feedreader.conf"))
    }

    pub fn parse_file(path: &Path) -> ah::Result<Self> {
        let s = std::fs::read_to_string(path).context("Read configuration file")?;
        Self::parse_str(&s)
    }

    pub fn parse_str(s: &str) -> ah::Result<Self> {
        let table: Table = toml::from_str(s).context("Parse configuration file")?;
        let mut config = Config::new();
        for (name, value) in &table {
            if name == "title_deny_highlighting"
                && let Value::Array(list) = value
            {
                for item in list {
                    if let Value::String(item) = item {
                        match Regex::new(item) {
                            Ok(re) => {
                                config.title_deny_highlighting.push(re);
                            }
                            Err(e) => {
                                return Err(err!(
                                    "Configuration entry '{name}' invalid regex: {e}"
                                ));
                            }
                        }
                    } else {
                        return Err(err!("Configuration entry '{name}' is not a string."));
                    }
                }
                continue;
            }

            if name == "summary_deny_highlighting"
                && let Value::Array(list) = value
            {
                for item in list {
                    if let Value::String(item) = item {
                        match Regex::new(item) {
                            Ok(re) => {
                                config.summary_deny_highlighting.push(re);
                            }
                            Err(e) => {
                                return Err(err!(
                                    "Configuration entry '{name}' invalid regex: {e}"
                                ));
                            }
                        }
                    } else {
                        return Err(err!("Configuration entry '{name}' is not a string."));
                    }
                }
                continue;
            }

            eprintln!("Ignoring configuration entry: {name} = {value:?}");
        }
        Ok(config)
    }
}

// vim: ts=4 sw=4 expandtab
