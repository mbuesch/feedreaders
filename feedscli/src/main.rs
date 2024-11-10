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

#![forbid(unsafe_code)]

mod command;

use crate::command::list::command_list;
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

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    /// List all feeds from the database.
    List,
}

async fn async_main(opts: Opts) -> ah::Result<()> {
    let opts = Arc::new(opts);

    let db = Db::new(&opts.db).await.context("Database")?;

    match opts.command {
        Command::List => command_list(&db).await,
    }
}

fn main() -> ah::Result<()> {
    let opts = Opts::parse();

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
