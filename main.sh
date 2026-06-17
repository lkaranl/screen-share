#!/usr/bin/env fish

git pull
and cargo build --release
and sudo ./target/release/screen-share