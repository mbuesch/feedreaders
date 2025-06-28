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

fn parse_regex(name: &str, value: &str) -> ah::Result<Regex> {
    match Regex::new(value) {
        Ok(re) => Ok(re),
        Err(e) => Err(err!("Configuration entry '{name}' invalid regex: {e}")),
    }
}

fn parse_regex_array(name: &str, value: &Value) -> ah::Result<Vec<Regex>> {
    let mut ret = vec![];
    if let Value::Array(a) = value {
        for item in a {
            if let Value::String(s) = item {
                ret.push(parse_regex(name, s)?);
            } else {
                return Err(err!(
                    "Configuration entry '{name}' array element is not a string."
                ));
            }
        }
    } else {
        return Err(err!("Configuration entry '{name}' is not an array."));
    }
    Ok(ret)
}

#[derive(Debug, Clone, Default)]
pub struct ConfigNoHighlighting {
    pub title: Vec<Regex>,
    pub summary: Vec<Regex>,
    pub url: Vec<Regex>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub no_highlighting: ConfigNoHighlighting,
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
            if name == "no-highlighting"
                && let Value::Table(t) = value
            {
                for (name, value) in t {
                    if name == "title" {
                        config.no_highlighting.title = parse_regex_array(name, value)?;
                        continue;
                    }
                    if name == "summary" {
                        config.no_highlighting.summary = parse_regex_array(name, value)?;
                        continue;
                    }
                    if name == "url" {
                        config.no_highlighting.url = parse_regex_array(name, value)?;
                        continue;
                    }
                    log::warn!("Ignoring configuration entry: {name} = {value:?}");
                }
                continue;
            }

            log::warn!("Ignoring configuration entry: {name} = {value:?}");
        }
        Ok(config)
    }
}

// vim: ts=4 sw=4 expandtab
