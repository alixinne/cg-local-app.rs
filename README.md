# [cg-local-app](https://github.com/alixinne/cg-local-app.rs)

[![Crates.io](https://img.shields.io/crates/v/cg-local-app)](https://crates.io/crates/cg-local-app)
[![Run tests](https://github.com/alixinne/cg-local-app.rs/workflows/Run%20tests/badge.svg?branch=master)](https://github.com/alixinne/cg-local-app.rs/actions)
[![GitHub release](https://img.shields.io/github/v/release/alixinne/cg-local-app.rs)](https://github.com/alixinne/cg-local-app.rs/releases)
![Maintenance](https://img.shields.io/badge/maintenance-activly--developed-brightgreen.svg)
[![Libraries.io dependency status for GitHub repo](https://img.shields.io/librariesio/github/alixinne/cg-local-app.rs)](https://libraries.io/github/alixinne/cg-local-app.rs)
[![License](https://img.shields.io/github/license/alixinne/cg-local-app.rs)](LICENSE)

Rust implementation of the client-side application for the [CG
Local](https://www.codingame.com/forum/t/cg-local/10359) extension. This is a drop-in
replacement for the original [Java application](https://github.com/jmerle/cg-local-app) which
works with the original [browser extension](https://github.com/jmerle/cg-local-ext).

### Install

#### Pre-built packages

Check the [releases](https://github.com/alixinne/cg-local-app.rs/releases) for binaries from
your operating system.

#### Using cargo

```bash
cargo install --force cg-local-app
```

#### From source

```bash
git clone https://github.com/alixinne/cg-local-app.rs.git && cd cg-local-app.rs
cargo install --path .
```

### Usage

```rust
cg-local-app 0.1.2
Alixinne <alixinne@pm.me>
Rust application for CG Local

USAGE:
    cg-local-app [FLAGS] [OPTIONS] --target <target>

FLAGS:
    -d, --download    Download the file from the IDE before synchronizing
    -h, --help        Prints help information
        --no-gui      Disable text user interface
    -p, --play        Auto-play questions on upload
    -V, --version     Prints version information

OPTIONS:
    -b, --bind <bind>        Address to bind to for the extension. Shouldn't need to be changed [default: 127.0.0.1:53135]
    -t, --target <target>    Path to the target file to synchronize with the IDE
```

### Examples

```bash
# Synchronize main.rs with the IDE, enable auto-play by default
cg-local-app -p -t main.rs
```

### Status

Missing features:
* Two-way synchronization

## License

MIT
