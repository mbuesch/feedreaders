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
use std::{num::NonZeroUsize, path::Path, time::Duration};
use toml::{Table, Value};

const DAYS_TO_SECS: u64 = 24 * 60 * 60;

fn parse_bool(name: &str, value: &Value) -> ah::Result<bool> {
    match value {
        Value::Boolean(b) => Ok(*b),
        _ => Err(err!("Configuration entry '{name}' invalid boolean.")),
    }
}

fn parse_regex(name: &str, value: &Value) -> ah::Result<Regex> {
    if let Value::String(s) = value {
        match Regex::new(s) {
            Ok(re) => Ok(re),
            Err(e) => Err(err!("Configuration entry '{name}' invalid regex: {e}")),
        }
    } else {
        Err(err!(
            "Configuration entry '{name}' array element is not a string."
        ))
    }
}

fn parse_regex_array(name: &str, value: &Value) -> ah::Result<Vec<Regex>> {
    let mut ret = vec![];
    if let Value::Array(array) = value {
        for value in array {
            ret.push(parse_regex(name, value)?);
        }
    } else {
        return Err(err!("Configuration entry '{name}' is not an array."));
    }
    Ok(ret)
}

fn parse_duration_secs(name: &str, value: &Value) -> ah::Result<Duration> {
    match value {
        Value::Integer(secs) if secs >= &0 => Ok(Duration::from_secs(*secs as u64)),
        _ => Err(err!("Configuration entry '{name}' invalid integer.")),
    }
}

fn parse_duration_days(name: &str, value: &Value) -> ah::Result<Duration> {
    match value {
        Value::Integer(days) if days >= &0 => {
            Ok(Duration::from_secs((*days as u64) * DAYS_TO_SECS))
        }
        _ => Err(err!("Configuration entry '{name}' invalid integer.")),
    }
}

fn parse_f64(name: &str, value: &Value) -> ah::Result<f64> {
    match value {
        Value::Float(val) => Ok(*val),
        Value::Integer(val) => Ok(*val as f64),
        _ => Err(err!(
            "Configuration entry '{name}' invalid floating point number."
        )),
    }
}

fn parse_usize(name: &str, value: &Value) -> ah::Result<usize> {
    match value {
        Value::Integer(val) if val >= &0 => Ok(*val as usize),
        _ => Err(err!("Configuration entry '{name}' invalid integer.")),
    }
}

fn parse_nonzerousize(name: &str, value: &Value) -> ah::Result<NonZeroUsize> {
    parse_usize(name, value).and_then(|v| match v.try_into() {
        Ok(v) => Ok(v),
        Err(_) => Err(err!("Configuration entry '{name}' invalid integer.")),
    })
}

#[derive(Debug, Clone)]
pub struct ConfigNet {
    pub timeout: Duration,
    pub concurrency: NonZeroUsize,
}

impl Default for ConfigNet {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            concurrency: NonZeroUsize::new(1).unwrap(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigDb {
    pub refresh_interval: Duration,
    pub refresh_slack: f64,
    pub gc_age_offset: Duration,
    pub highlight_updated_items: bool,
}

impl Default for ConfigDb {
    fn default() -> Self {
        Self {
            refresh_interval: Duration::from_secs(600),
            refresh_slack: 0.1,
            gc_age_offset: Duration::from_secs(180 * DAYS_TO_SECS),
            highlight_updated_items: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConfigNoHighlighting {
    pub title: Vec<Regex>,
    pub summary: Vec<Regex>,
    pub url: Vec<Regex>,
    pub set_seen: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub net: ConfigNet,
    pub db: ConfigDb,
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
        let s = if path.exists() {
            std::fs::read_to_string(path).context("Read configuration file")?
        } else {
            "".to_string()
        };
        Self::parse_str(&s)
    }

    pub fn parse_str(s: &str) -> ah::Result<Self> {
        let table: Table = toml::from_str(s).context("Parse configuration file")?;
        let mut config = Config::new();

        for (name, value) in &table {
            if name == "net"
                && let Value::Table(t) = value
            {
                for (name, value) in t {
                    if name == "timeout-secs" {
                        config.net.timeout = parse_duration_secs(name, value)?;
                        continue;
                    }
                    if name == "concurrency" {
                        config.net.concurrency = parse_nonzerousize(name, value)?;
                        continue;
                    }
                    log::warn!("Ignoring configuration entry: {name} = {value:?}");
                }
                continue;
            }

            if name == "db"
                && let Value::Table(t) = value
            {
                for (name, value) in t {
                    if name == "refresh-interval-secs" {
                        config.db.refresh_interval = parse_duration_secs(name, value)?;
                        continue;
                    }
                    if name == "refresh-slack" {
                        config.db.refresh_slack = parse_f64(name, value)?;
                        continue;
                    }
                    if name == "gc-age-offset-days" {
                        config.db.gc_age_offset = parse_duration_days(name, value)?;
                        continue;
                    }
                    if name == "highlight-updated-items" {
                        config.db.highlight_updated_items = parse_bool(name, value)?;
                        continue;
                    }
                    log::warn!("Ignoring configuration entry: {name} = {value:?}");
                }
                continue;
            }

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
                    if name == "set-seen" {
                        config.no_highlighting.set_seen = parse_bool(name, value)?;
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
