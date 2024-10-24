#!/bin/sh
# -*- coding: utf-8 -*-

basedir="$(realpath "$0" | xargs dirname)"

die()
{
    echo "=== ERROR: $*" >&2
    exit 1
}

[ -f "$basedir/Cargo.toml" ] || die "basedir sanity check failed"

cd "$basedir" || die "cd basedir failed."
export FEEDREADER_PREFIX="/opt/feedreader"
cargo build || die "Cargo build (debug) failed."
cargo test || die "Cargo test failed."
cargo auditable build --release || die "Cargo build (release) failed."
cargo audit --deny warnings bin \
    target/release/feeds \
    target/release/feedsd \
    || die "Cargo audit failed."

# vim: ts=4 sw=4 expandtab
