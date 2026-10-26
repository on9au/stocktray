//! Everything in stocktray that doesn't need the tray itself, kept in a library so it can be
//! unit-tested and previewed (`cargo run --example preview`) without an event loop.

pub mod config;
pub mod icon;
pub mod poller;
pub mod quote;
pub mod update;
pub mod view;
