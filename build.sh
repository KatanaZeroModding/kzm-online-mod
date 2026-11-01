#!/bin/sh
set -e
cd "$(dirname "$0")"
cargo build --release --target i686-pc-windows-msvc
mkdir -p dist
cp -f target/i686-pc-windows-msvc/release/online.dll dist/online.dll
