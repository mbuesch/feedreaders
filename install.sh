#!/bin/sh
# -*- coding: utf-8 -*-

basedir="$(realpath "$0" | xargs dirname)"

info()
{
    echo "--- $*"
}

error()
{
    echo "=== ERROR: $*" >&2
}

warning()
{
    echo "=== WARNING: $*" >&2
}

die()
{
    error "$*"
    exit 1
}

do_install()
{
    info "install $*"
    install "$@" || die "Failed install $*"
}

do_systemctl()
{
    info "systemctl $*"
    systemctl "$@" || die "Failed to systemctl $*"
}

do_chown()
{
    info "chown $*"
    chown "$@" || die "Failed to chown $*"
}

do_chmod()
{
    info "chmod $*"
    chmod "$@" || die "Failed to chmod $*"
}

try_systemctl()
{
    info "systemctl $*"
    systemctl "$@" 2>/dev/null
}

do_chown()
{
    info "chown $*"
    chown "$@" || die "Failed to chown $*"
}

do_chmod()
{
    info "chmod $*"
    chmod "$@" || die "Failed to chmod $*"
}

entry_checks()
{
    [ -d "$target" ] || die "feedreader is not built! Run ./build.sh"
    [ "$(id -u)" = "0" ] || die "Must be root to install feedreader."
}

stop_services()
{
    try_systemctl stop feedsd.service
    try_systemctl disable feedsd.service
}

start_services()
{
    do_systemctl start feedsd.service
}

install_dirs()
{
    do_install \
        -o root -g root -m 0755 \
        -d /opt/feedreader/bin

    do_install \
        -o root -g root -m 0755 \
        -d /opt/feedreader/lib/cgi-bin

    do_install \
        -o root -g root -m 0755 \
        -d /opt/feedreader/var/lib

    do_install \
        -o root -g www-data -m 0775 \
        -d /opt/feedreader/var/lib/feedreader

    do_install \
        -o root -g root -m 0755 \
        -d /opt/feedreader/share/feedreader
}

install_feedsd()
{
    do_install \
        -o root -g root -m 0755 \
        "$target/feedsd" \
        /opt/feedreader/bin/

    do_install \
        -o root -g root -m 0644 \
        "$basedir/feedsd/feedsd.service" \
        /etc/systemd/system/

    do_systemctl enable feedsd.service
}

install_feeds()
{
    do_install \
        -o root -g root -m 0755 \
        "$target/feeds" \
        /opt/feedreader/lib/cgi-bin/

    do_install \
        -o root -g root -m 0644 \
        "$basedir/resources/icon.png" \
        /opt/feedreader/share/feedreader/

    do_install \
        -o root -g root -m 0644 \
        "$basedir/resources/style.css" \
        /opt/feedreader/share/feedreader/
}

release="release"
while [ $# -ge 1 ]; do
    case "$1" in
        --debug|-d)
            release="debug"
            ;;
        --release|-r)
            release="release"
            ;;
        *)
            die "Invalid option: $1"
            ;;
    esac
    shift
done
target="$basedir/target/$release"

entry_checks
stop_services
install_dirs
install_feedsd
install_feeds
start_services

# vim: ts=4 sw=4 expandtab
