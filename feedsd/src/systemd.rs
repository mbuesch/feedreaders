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

use anyhow as ah;

/// Notify ready-status to systemd.
///
/// All environment variables related to this operation will be cleared.
pub fn systemd_notify_ready() -> ah::Result<()> {
    sd_notify::notify(true, &[sd_notify::NotifyState::Ready])?;
    Ok(())
}

// vim: ts=4 sw=4 expandtab
