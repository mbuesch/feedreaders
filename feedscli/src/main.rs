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

mod command;

use crate::command::{getkv::command_getkv, list::command_list, seen::command_seen};
use anyhow::{self as ah, Context as _};
use clap::{Parser, Subcommand};
use feedsdb::Db;
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use tokio::runtime;

#[derive(Parser, Debug, Clone)]
struct Opts {
    /// The name of the database to use.
    #[arg(long, default_value = "feeds")]
    db: String,

    /// Set the number async worker threads.
    #[arg(long, default_value = "2")]
    worker_threads: NonZeroUsize,

    /// Enable `tokio-console` tracing support.
    ///
    /// See https://crates.io/crates/tokio-console
    #[arg(long)]
    tokio_console: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    /// List all feeds from the database.
    List,

    /// Mark items as "seen".
    Seen {
        /// The feed ID to set to "seen".
        /// Or alternatively "all" to set all feeds to seen.
        id: String,
    },

    /// Get a value from the key-value-store.
    #[command(subcommand)]
    GetKv(GetKv),
}

#[derive(Subcommand, Debug, Clone, Copy)]
enum GetKv {
    FeedUpdateRev,
}

async fn async_main(opts: Opts) -> ah::Result<()> {
    let opts = Arc::new(opts);

    let db = Db::new(&opts.db).await.context("Database")?;

    match &opts.command {
        Command::List => command_list(&db).await,
        Command::Seen { id } => command_seen(&db, id).await,
        Command::GetKv(kv) => command_getkv(&db, kv).await,
    }
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

    runtime::Builder::new_multi_thread()
        .worker_threads(opts.worker_threads.into())
        .max_blocking_threads(opts.worker_threads.into()) // one blocking per worker.
        .thread_keep_alive(Duration::from_secs(1))
        .enable_all()
        .build()
        .context("Tokio runtime builder")?
        .block_on(async_main(opts))
}

// vim: ts=4 sw=4 expandtab
