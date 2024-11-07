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

mod refresh;
mod systemd;

use crate::{refresh::refresh_feeds, systemd::systemd_notify_ready};
use anyhow::{self as ah, format_err as err, Context as _};
use clap::Parser;
use feedsdb::{Db, DEBUG};
use std::{num::NonZeroUsize, time::Duration};
use tokio::{
    runtime,
    signal::unix::{signal, SignalKind},
    sync, task, time,
};

#[derive(Parser, Debug, Clone)]
struct Opts {
    #[arg(long, default_value = "feeds")]
    db: String,

    /// Set the number async worker threads.
    #[arg(long, default_value = "4")]
    worker_threads: NonZeroUsize,

    /// Feed refresh interval, in seconds.
    #[arg(long, default_value = "60")]
    refresh_interval: u64,
}

impl Opts {
    pub fn refresh_interval(&self) -> Duration {
        Duration::from_secs(self.refresh_interval)
    }
}

async fn async_main(opts: Opts) -> ah::Result<()> {
    // Register unix signal handlers.
    let mut sigterm = signal(SignalKind::terminate()).unwrap();
    let mut sigint = signal(SignalKind::interrupt()).unwrap();
    let mut sighup = signal(SignalKind::hangup()).unwrap();

    // Create async IPC channels.
    let (exit_sock_tx, mut exit_sock_rx) = sync::mpsc::channel(1);

    // Create the database access object.
    let db = Db::new(&opts.db).await.context("Database")?;
    // Initialize the database, if not already done.
    db.open()
        .await
        .context("Open database")?
        .init()
        .await
        .context("Initialize database")?;

    // Ready-signal to systemd.
    systemd_notify_ready().context("Notify systemd")?;

    // Task: DB refresher.
    task::spawn({
        async move {
            if DEBUG {
                eprintln!("Refreshing...");
            }
            if let Err(e) = refresh_feeds(&db, opts.refresh_interval()).await {
                eprintln!("ERROR: {e:?}");
            } else if DEBUG {
                eprintln!("Refreshed.");
            }

            let mut interval = time::interval(opts.refresh_interval() / 10); //TODO
            interval.reset();
            let mut err_count = 0_u32;
            loop {
                interval.tick().await;
                if DEBUG {
                    eprintln!("Refreshing...");
                }
                if let Err(e) = refresh_feeds(&db, opts.refresh_interval()).await {
                    err_count = err_count.saturating_add(3);
                    if err_count >= 9 {
                        let e = Err(err!("Too many errors. Bailing to systemd."));
                        let _ = exit_sock_tx.send(e).await;
                        break;
                    }
                    eprintln!("ERROR: {e:?}");
                } else {
                    err_count = err_count.saturating_sub(1);
                    if DEBUG {
                        eprintln!("Refreshed.");
                    }
                }
            }
        }
    });

    // Task: Main loop.
    let exitcode;
    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                eprintln!("SIGTERM: Terminating.");
                exitcode = Ok(());
                break;
            }
            _ = sigint.recv() => {
                exitcode = Err(err!("Interrupted by SIGINT."));
                break;
            }
            _ = sighup.recv() => {
                //TODO trigger refresh
            }
            code = exit_sock_rx.recv() => {
                exitcode = code.unwrap_or_else(|| Err(err!("Unknown error code.")));
                break;
            }
        }
    }
    exitcode
}

fn main() -> ah::Result<()> {
    let opts = Opts::parse();

    runtime::Builder::new_multi_thread()
        .thread_keep_alive(Duration::from_secs(10))
        .worker_threads(opts.worker_threads.into())
        .enable_all()
        .build()
        .context("Tokio runtime builder")?
        .block_on(async_main(opts))
}

// vim: ts=4 sw=4 expandtab
