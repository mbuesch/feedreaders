# feedreader

Feedreader/rs is a Rust re-implementation of the original [feedreader](https://github.com/mlochen/feedreader) Python implementation.

The database format is almost fully compatible with the original.
Re-using an existing database is possible.
We don't implement 'enclosures', though.
Existing enclosures in the database will be deleted.

# Thanks

Thanks to [Marco Lochen](https://github.com/mlochen) for the idea and for the original Python implementation.

# Building and installing

## Prerequisites

The Rust compiler must be installed to build feedreader.
It is recommended to use the latest version of the stable Rust compiler.
But Rust versions down to and including Rust 1.77 are supported by feedreader.

[Rust installer](https://www.rust-lang.org/tools/install)

The Rust installer will install the compiler and the build tool `cargo`.

The build requires the additional `cargo-audit` and `cargo-auditable` tools to be installed.
Run this command to install both tools:

```sh
cargo install cargo-audit cargo-auditable
```

The SQLite3 database has to be installed in your operating system.

If you use Debian Linux, you can use the following command to install SQLite3:

```sh
sudo apt install libsqlite3-dev
```

## Building feedreader

Run the `build.sh` script to build feedreader.

After installing all build prerequisites, run the build script:

```sh
./build.sh
```

## Installing feedreader

Then run the `install.sh` to install the feedreader to `/opt/feedreader/`:

```sh
./install.sh
```

## -lsqlite3: No such file or directory

If during build you get the following error:

```
= note: /usr/bin/ld: cannot find -lsqlite3: No such file or directory
    collect2: error: ld returned 1 exit status
```

please install the SQLite 3 libraries and development files to your system.

On Debian Linux that is done with:

```sh
sudo apt install libsqlite3-dev
```

# Configuring web server CGI

The web frontend `feeds` needs to be configured in your web browser as CGI application.

## lighttpd web server

Add the following configuration to `/etc/lighttpd/conf-enabled/10-cgi.conf`:

```
server.modules += ( "mod_cgi" )

$HTTP["url"] =~ "^/cgi-bin/" {
    cgi.assign = ( "" => "" )
    alias.url += ( "/cgi-bin/" => "/opt/feedreader/lib/cgi-bin/" )
}
```

## Apache web server

Add the following configuration to `/etc/apache2/conf-enabled/feedreader.conf`:

```
ScriptAlias /cgi-bin/feeds /opt/feedreader/lib/cgi-bin/feeds
<Directory /opt/feedreader/lib/cgi-bin>
    AllowOverride None
    Options +ExecCGI -MultiViews +SymLinksIfOwnerMatch -Indexes
    Require all granted
</Directory>
```

# License / Copyright

Copyright (C) 2024 Michael Büsch

Copyright (C) 2020 Marco Lochen

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 2 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.
