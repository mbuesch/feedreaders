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

#![forbid(unsafe_code)]

mod cgi;
mod formfields;
mod pagegen;
mod query;
mod wakeup;

use crate::{cgi::Cgi, pagegen::PageGen};
use anyhow::{self as ah, Context as _};
use clap::Parser;
use feedsdb::Db;
use std::time::Duration;
use tokio::runtime;

#[derive(Parser, Debug, Clone)]
struct Opts {
    /// The name of the database to use.
    #[arg(long, default_value = "feeds")]
    db: String,

    /// Enable `tokio-console` tracing support.
    ///
    /// See https://crates.io/crates/tokio-console
    #[arg(long)]
    tokio_console: bool,
}

async fn async_main(opts: Opts) -> ah::Result<()> {
    // Create the database access object.
    let db = Db::new(&opts.db).await.context("Database")?;

    // Create the page generator.
    let mut pagegen = PageGen::new(&db)
        .await
        .context("Initialze page generator")?;

    // Handle the CGI with the web server.
    let mut cgi = Cgi::new().await.context("Initialize CGI")?;
    cgi.run(&mut pagegen).await;
    Ok(())
}

fn main() -> ah::Result<()> {
    env_logger::init_from_env(
        env_logger::Env::new()
            .filter_or("FEEDREADER_LOG", "info")
            .write_style_or("FEEDREADER_LOG_STYLE", "auto"),
    );

    let opts = Opts::parse();

    if opts.tokio_console {
        console_subscriber::init();
    }

    runtime::Builder::new_current_thread()
        .worker_threads(1)
        .max_blocking_threads(4)
        .thread_keep_alive(Duration::from_secs(1))
        .enable_all()
        .build()
        .context("Tokio runtime builder")?
        .block_on(async_main(opts))
}

// vim: ts=4 sw=4 expandtab
