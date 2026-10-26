//! User settings, stored as TOML in `%APPDATA%\stocktray\config.toml`.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::quote::Symbol;

/// Written on first run so there is something to edit.
const TEMPLATE: &str = r#"# stocktray settings. Saved changes are picked up within a few seconds.

# Yahoo Finance symbol. ASX listings end in .AX (WTC.AX, CBA.AX, BHP.AX) and
# "ASX:WTC" works too. US listings have no suffix (MSFT, AAPL).
symbol = "WTC.AX"

# Seconds between refreshes while the market is open, and while it is closed.
poll_open_secs = 60
poll_closed_secs = 900
"#;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawConfig")]
pub struct Config {
    pub symbol: Symbol,
    pub poll_open: Duration,
    pub poll_closed: Duration,
}

impl Default for Config {
    fn default() -> Self {
        RawConfig::default().try_into().expect("the default config is valid")
    }
}

/// The file as written, before validation.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    symbol: String,
    poll_open_secs: u64,
    poll_closed_secs: u64,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self { symbol: "WTC.AX".to_owned(), poll_open_secs: 60, poll_closed_secs: 900 }
    }
}

impl TryFrom<RawConfig> for Config {
    type Error = anyhow::Error;

    /// Normalises the symbol and floors the intervals, so a typo can't hammer the quote APIs.
    fn try_from(raw: RawConfig) -> Result<Self> {
        Ok(Self {
            symbol: raw.symbol.parse()?,
            poll_open: Duration::from_secs(raw.poll_open_secs.max(15)),
            poll_closed: Duration::from_secs(raw.poll_closed_secs.max(60)),
        })
    }
}

/// The config file, remembering when it was last read so edits can be picked up.
pub struct ConfigFile {
    pub path: PathBuf,
    loaded: Option<SystemTime>,
}

impl ConfigFile {
    pub fn in_appdata() -> Self {
        let appdata = std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("."), PathBuf::from);
        Self { path: appdata.join("stocktray").join("config.toml"), loaded: None }
    }

    /// Reads the file, first writing the commented default if it doesn't exist.
    pub fn load(&mut self) -> Result<Config> {
        if !self.path.exists() {
            fs::create_dir_all(self.path.parent().context("config path has no parent")?)?;
            fs::write(&self.path, TEMPLATE)?;
        }
        self.loaded = self.modified();
        let text = fs::read_to_string(&self.path).with_context(|| format!("reading {}", self.path.display()))?;
        Ok(toml::from_str(&text)?)
    }

    /// Reloads if the file changed since it was last read.
    pub fn reload_if_changed(&mut self) -> Option<Result<Config>> {
        (self.modified() != self.loaded).then(|| self.load())
    }

    fn modified(&self) -> Option<SystemTime> {
        fs::metadata(&self.path).and_then(|m| m.modified()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(toml)
    }

    #[test]
    fn template_parses_to_default() {
        assert_eq!(parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn missing_fields_take_defaults() {
        let config = parse(r#"symbol = "cba.ax""#).unwrap();
        assert_eq!((config.symbol.to_string(), config.poll_open), ("CBA.AX".to_owned(), Duration::from_secs(60)));
    }

    #[test]
    fn poll_intervals_are_floored() {
        let config = parse("poll_open_secs = 1\npoll_closed_secs = 2").unwrap();
        assert_eq!((config.poll_open.as_secs(), config.poll_closed.as_secs()), (15, 60));
    }

    #[test]
    fn rejects_typos_and_bad_symbols() {
        assert!(parse(r#"symbl = "WTC.AX""#).is_err());
        assert!(parse(r#"symbol = "WTC AX""#).is_err());
    }
}
