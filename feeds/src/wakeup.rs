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
use nix::{
    sys::signal::{kill, Signal},
    unistd::Pid,
};
use tokio::fs::read_to_string;

/// Get the PID of the `feedsd` daemon process.
async fn get_feedsd_pid() -> ah::Result<Pid> {
    let pid = read_to_string("/run/feedsd/feedsd.pid")
        .await
        .context("Read PID-file of 'feedsd' daemon")?;
    let pid: i32 = pid
        .trim()
        .parse()
        .context("Parse 'feedsd' PID-file string to number")?;
    Ok(Pid::from_raw(pid))
}

pub async fn wakeup_feedsd() {
    let pid = match get_feedsd_pid().await {
        Ok(pid) => pid,
        Err(e) => {
            eprintln!("Failed to get feedsd pid: {e:?}");
            return;
        }
    };
    if let Err(e) = kill(pid, Signal::SIGHUP) {
        eprintln!("Failed to send SIGHUP to feedsd: {e:?}");
    }
}

// vim: ts=4 sw=4 expandtab
