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

use crate::{
    formfields::FormFields,
    pagegen::{GetBody, PageGen},
    query::Query,
};
use anyhow::{self as ah, format_err as err};
use feedsdb::DEBUG;
use std::{
    env,
    ffi::OsString,
    io::{self, Read as _, Stdout, Write as _},
    time::Instant,
};

const MAX_CGIENV_LEN: usize = 1024 * 4;
const MAX_CGIENV_U32_LEN: usize = 10;
const MAX_POST_BODY_LEN: u32 = 1024 * 1024;

fn get_cgienv(name: &str) -> ah::Result<OsString> {
    let value = env::var_os(name).unwrap_or_default();
    if value.len() <= MAX_CGIENV_LEN {
        Ok(value)
    } else {
        Err(err!("Environment variable '{name}' is too long."))
    }
}

fn get_cgienv_str(name: &str) -> ah::Result<String> {
    if let Ok(s) = get_cgienv(name)?.into_string() {
        Ok(s)
    } else {
        Err(err!("Environment variable '{name}' is not valid UTF-8."))
    }
}

fn get_cgienv_u32(name: &str) -> ah::Result<u32> {
    let value = get_cgienv_str(name)?;
    let value = value.trim();
    if value.len() <= MAX_CGIENV_U32_LEN {
        Ok(value.parse::<u32>()?)
    } else {
        Err(err!("Environment variable '{name}' is too long (u32)."))
    }
}

fn out(f: &mut Stdout, data: &[u8]) {
    f.write_all(data).unwrap();
}

fn outstr(f: &mut Stdout, data: &str) {
    out(f, data.as_bytes());
}

fn response_200_ok(
    body: Option<&[u8]>,
    mime: &str,
    extra_headers: &[String],
    start_stamp: Option<Instant>,
) {
    let mut f = io::stdout();
    outstr(&mut f, &format!("Content-type: {mime}\n"));
    for header in extra_headers {
        outstr(&mut f, &format!("{header}\n"));
    }
    outstr(&mut f, "Status: 200 Ok\n");
    if let Some(start_stamp) = start_stamp {
        let runtime = (Instant::now() - start_stamp).as_micros();
        outstr(&mut f, &format!("X-feedreader-Cgi-Runtime: {runtime} us\n"));
    }
    outstr(&mut f, "\n");
    if let Some(body) = body {
        out(&mut f, body);
    }
}

fn response_400_bad_request(err: &str) {
    let mut f = io::stdout();
    outstr(&mut f, "Content-type: text/plain\n");
    outstr(&mut f, "Status: 400 Bad Request\n");
    outstr(&mut f, "\n");
    outstr(&mut f, err);
}

fn response_500_internal_error(err: &str) {
    let mut f = io::stdout();
    outstr(&mut f, "Content-type: text/plain\n");
    outstr(&mut f, "Status: 500 Internal Server Error\n");
    outstr(&mut f, "\n");
    outstr(&mut f, err);
}

pub struct Cgi {
    query: String,
    meth: String,
    _path: String,
    body_len: u32,
    body_type: String,
    _host: String,
    _cookie: OsString,
    start_stamp: Option<Instant>,
}

impl Cgi {
    pub async fn new() -> ah::Result<Self> {
        let start_stamp = if DEBUG { Some(Instant::now()) } else { None };

        let query = get_cgienv_str("QUERY_STRING").unwrap_or_default();
        let meth = get_cgienv_str("REQUEST_METHOD")?.trim().to_string();
        let path = get_cgienv_str("PATH_INFO").unwrap_or_default();
        let body_len = get_cgienv_u32("CONTENT_LENGTH").unwrap_or_default();
        let body_type = get_cgienv_str("CONTENT_TYPE").unwrap_or_default();
        let host = get_cgienv_str("HTTP_HOST").unwrap_or_default();
        let cookie = get_cgienv("HTTP_COOKIE")?;

        Ok(Self {
            query,
            meth,
            _path: path,
            body_len,
            body_type,
            _host: host,
            _cookie: cookie,
            start_stamp,
        })
    }

    pub async fn run(&mut self, pagegen: &mut PageGen<'_>) {
        let Ok(query) = Query::parse(&self.query) else {
            response_400_bad_request("Invalid QUERY_STRING in URI.");
            return;
        };

        match &self.meth[..] {
            "HEAD" => match pagegen.get(&query, GetBody::No).await {
                Ok(res) => response_200_ok(None, &res.mime, &[], self.start_stamp),
                Err(e) => {
                    if DEBUG {
                        response_500_internal_error(&format!("{e:?}"));
                    } else {
                        response_500_internal_error("HEAD failed");
                    }
                }
            },
            "GET" => match pagegen.get(&query, GetBody::Yes).await {
                Ok(res) => {
                    response_200_ok(Some(res.body.as_bytes()), &res.mime, &[], self.start_stamp)
                }
                Err(e) => {
                    if DEBUG {
                        response_500_internal_error(&format!("{e:?}"));
                    } else {
                        response_500_internal_error("GET failed");
                    }
                }
            },
            "POST" => {
                if self.body_len == 0 {
                    response_400_bad_request("POST: CONTENT_LENGTH is zero.");
                    return;
                }
                if self.body_len > MAX_POST_BODY_LEN {
                    response_400_bad_request("POST: CONTENT_LENGTH is too large.");
                    return;
                }
                if self.body_type.is_empty() {
                    response_400_bad_request("POST: Invalid CONTENT_TYPE.");
                    return;
                }

                let mut body = vec![0; self.body_len.try_into().unwrap()];
                if io::stdin().read_exact(&mut body).is_err() {
                    response_500_internal_error("CGI stdin read failed.");
                    return;
                }

                let Ok(formfields) = FormFields::new(&body, &self.body_type).await else {
                    response_500_internal_error("POST: Parsing form-fields failed.");
                    return;
                };

                match pagegen.post(&query, &formfields).await {
                    Ok(res) => {
                        response_200_ok(Some(res.body.as_bytes()), &res.mime, &[], self.start_stamp)
                    }
                    Err(e) => {
                        if DEBUG {
                            response_500_internal_error(&format!("{e:?}"));
                        } else {
                            response_500_internal_error("POST failed");
                        }
                    }
                }
            }
            m => {
                response_400_bad_request(&format!("Unsupported REQUEST_METHOD: '{m}'"));
            }
        }
    }
}

// vim: ts=4 sw=4 expandtab
