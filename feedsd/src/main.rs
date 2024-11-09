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
use std::{fs::OpenOptions, io::Write as _, num::NonZeroUsize, sync::Arc, time::Duration};
use tokio::{
    runtime,
    signal::unix::{signal, SignalKind},
    sync, task,
};

/// Create the PID-file in the /run subdirectory.
fn make_pidfile() -> ah::Result<()> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("/run/feedsd/feedsd.pid")
        .context("Open PID-file")?
        .write_all(format!("{}\n", std::process::id()).as_bytes())
        .context("Write to PID-file")
}

#[derive(Parser, Debug, Clone)]
struct Opts {
    #[arg(long, default_value = "feeds")]
    db: String,

    /// Set the number async worker threads.
    #[arg(long, default_value = "4")]
    worker_threads: NonZeroUsize,

    /// Feed refresh interval, in seconds.
    #[arg(long, default_value = "600")]
    refresh_interval: u64,
}

impl Opts {
    pub fn refresh_interval(&self) -> Duration {
        Duration::from_secs(self.refresh_interval)
    }
}

#[must_use]
async fn do_refresh(db: Arc<Db>, opts: &Opts) -> (bool, Duration) {
    if DEBUG {
        eprintln!("Refreshing...");
    }
    match refresh_feeds(db, opts.refresh_interval()).await {
        Err(e) => {
            eprintln!("ERROR: {e:?}");
            (false, Duration::from_secs(60))
        }
        Ok(sleep_dur) => {
            if DEBUG {
                eprintln!("Refreshed. Sleeping {sleep_dur:?}.");
            }
            (true, sleep_dur)
        }
    }
}

async fn async_main(opts: Opts) -> ah::Result<()> {
    let opts = Arc::new(opts);

    // Create pid-file in /run.
    make_pidfile()?;

    // Register unix signal handlers.
    let mut sigterm = signal(SignalKind::terminate()).unwrap();
    let mut sigint = signal(SignalKind::interrupt()).unwrap();
    let mut sighup = signal(SignalKind::hangup()).unwrap();

    // Create async IPC channels.
    let (exit_sock_tx, mut exit_sock_rx) = sync::mpsc::channel(1);

    // Create the database access object.
    let db = Arc::new(Db::new(&opts.db).await.context("Database")?);

    // Initialize the database, if not already done.
    db.open()
        .await
        .context("Open database")?
        .init()
        .await
        .context("Initialize database")?;

    // Ready-signal to systemd.
    systemd_notify_ready().context("Notify systemd")?;

    // Vacuum the database.
    db.open()
        .await
        .context("Open database")?
        .vacuum()
        .await
        .context("Vacuum database")?;

    // Task: DB refresher.
    task::spawn({
        let db = Arc::clone(&db);
        let opts = Arc::clone(&opts);

        async move {
            let mut err_count = 0_u32;
            loop {
                let (ok, sleep_dur) = do_refresh(Arc::clone(&db), &opts).await;
                if ok {
                    err_count = err_count.saturating_sub(1);
                } else {
                    err_count = err_count.saturating_add(3);
                    if err_count >= 9 {
                        let e = Err(err!("Too many errors. Bailing to systemd."));
                        let _ = exit_sock_tx.send(e).await;
                        break;
                    }
                }
                tokio::time::sleep(sleep_dur).await;
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
                println!("SIGHUP: Triggering database refresh.");
                let _ = do_refresh(Arc::clone(&db), &opts).await;
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
        .worker_threads(opts.worker_threads.into())
        .max_blocking_threads(opts.worker_threads.into()) // one blocking per worker.
        .thread_keep_alive(Duration::from_secs(10))
        .enable_all()
        .build()
        .context("Tokio runtime builder")?
        .block_on(async_main(opts))
}

// vim: ts=4 sw=4 expandtab
