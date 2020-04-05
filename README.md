[![Crates.io](https://img.shields.io/crates/v/cg-local-app)](https://crates.io/crates/cg-local-app) [![Build Status](https://travis-ci.com/vtavernier/cg-local-app.rs.svg?branch=master)](https://travis-ci.com/vtavernier/cg-local-app.rs) [![Build Status](https://ci.appveyor.com/api/projects/status/github/vtavernier/cg-local-app.rs?branch=master&svg=true)](https://ci.appveyor.com/project/vtavernier/cg-local-app.rs/branch/master) ![Crates.io](https://img.shields.io/crates/l/cg-local-app) [![Libraries.io dependency status for GitHub repo](https://img.shields.io/librariesio/github/vtavernier/cg-local-app.rs)](https://libraries.io/github/vtavernier/cg-local-app.rs)

# cg-local-app

Rust implementation of the client-side application for the [CG
Local](https://www.codingame.com/forum/t/cg-local/10359) extension. This is a drop-in
replacement for the original [Java application](https://github.com/jmerle/cg-local-app) which
works with the original [browser extension](https://github.com/jmerle/cg-local-ext).

### Install

#### Pre-built packages

Check the [releases](https://github.com/vtavernier/cg-local-app.rs/releases) for binaries from
your operating system.

#### Using cargo

```bash
cargo install --force cg-local-app
```

#### From source

```bash
git clone https://github.com/vtavernier/cg-local-app.rs.git && cd cg-local-app.rs
cargo install --path .
```

### Usage

```rust
cg-local-app 0.1.1
Vincent Tavernier <vince.tavernier@gmail.com>
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
