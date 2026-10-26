//! Quotes and where they come from. Each API is a [`QuoteSource`]; [`Fallback`] tries several
//! in turn.

mod asx;
mod yahoo;

use std::fmt;
use std::ops::Range;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use time::OffsetDateTime;

pub use asx::Asx;
pub use yahoo::Yahoo;

/// A normalised Yahoo Finance symbol such as `WTC.AX`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol(String);

impl FromStr for Symbol {
    type Err = anyhow::Error;

    /// Upper-cases, and turns `ASX:WTC` into `WTC.AX`.
    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim().to_ascii_uppercase();
        let s = match s.strip_prefix("ASX:") {
            Some(code) => format!("{}.AX", code.trim()),
            None => s,
        };
        let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '^' | '=');
        if s.is_empty() || !s.chars().all(valid) {
            bail!("invalid symbol {s:?}");
        }
        Ok(Self(s))
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Symbol {
    /// The ASX code for symbols listed there, e.g. `WTC` for `WTC.AX`.
    pub fn asx_code(&self) -> Option<&str> {
        self.0.strip_suffix(".AX").filter(|code| !code.is_empty())
    }

    /// The ASX company page for ASX listings, Yahoo's quote page otherwise.
    pub fn quote_page_url(&self) -> String {
        match self.asx_code() {
            Some(code) => format!("https://www.asx.com.au/markets/company/{code}"),
            None => format!("https://finance.yahoo.com/quote/{}", self.url_encoded()),
        }
    }

    /// `^` and `=` are the only valid symbol characters that need escaping in a URL.
    fn url_encoded(&self) -> String {
        self.0.replace('^', "%5E").replace('=', "%3D")
    }
}

/// An amount in the quote's currency.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Price(pub f64);

impl Price {
    /// Decimal places to show: sub-dollar stocks trade in fractions of a cent.
    pub fn decimals(self) -> usize {
        if self.0.abs() < 1.0 { 3 } else { 2 }
    }
}

/// When a quote was struck, in the exchange's time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteTime {
    pub at: OffsetDateTime,
    /// Abbreviation such as `AEDT`.
    pub zone: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub name: Option<String>,
    pub price: Price,
    pub previous_close: Price,
    pub currency: Option<String>,
    /// Unknown when only the ASX answered.
    pub time: Option<QuoteTime>,
    /// The regular trading session.
    pub session: Option<Range<OffsetDateTime>>,
}

impl Quote {
    pub fn change(&self) -> Price {
        Price(self.price.0 - self.previous_close.0)
    }

    pub fn change_pct(&self) -> f64 {
        if self.previous_close.0 == 0.0 { 0.0 } else { self.change().0 / self.previous_close.0 * 100.0 }
    }
}

pub trait QuoteSource: Send {
    /// Whether this source lists `symbol` at all.
    fn covers(&self, _symbol: &Symbol) -> bool {
        true
    }

    fn fetch(&self, symbol: &Symbol) -> Result<Quote>;
}

/// Tries each source that covers the symbol, in order, returning the first quote.
pub struct Fallback(pub Vec<Box<dyn QuoteSource>>);

impl QuoteSource for Fallback {
    fn fetch(&self, symbol: &Symbol) -> Result<Quote> {
        let mut errors = Vec::new();
        for source in self.0.iter().filter(|s| s.covers(symbol)) {
            match source.fetch(symbol) {
                Ok(quote) => return Ok(quote),
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
        bail!("{}", errors.join("; "))
    }
}

/// An HTTP client set up for the quote APIs.
pub fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        // Both APIs explain failures in a JSON body, so read it rather than failing on status.
        .http_status_as_error(false)
        // Yahoo turns away user agents that don't look like a browser.
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .build()
        .into()
}

fn get_json<T: DeserializeOwned>(agent: &ureq::Agent, url: &str) -> Result<T> {
    let mut response = agent.get(url).call()?;
    let status = response.status();
    let body = response.body_mut().read_to_string()?;
    serde_json::from_str(&body).with_context(|| format!("HTTP {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(s: &str) -> Symbol {
        s.parse().unwrap()
    }

    #[test]
    fn symbols_are_normalised() {
        assert_eq!(symbol("ASX: wtc"), symbol("WTC.AX"));
        assert_eq!(symbol(" msft ").to_string(), "MSFT");
        for bad in ["", "WTC AX", "WTC/AX"] {
            assert!(bad.parse::<Symbol>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn quote_pages() {
        assert_eq!(symbol("WTC.AX").quote_page_url(), "https://www.asx.com.au/markets/company/WTC");
        assert_eq!(symbol("^AXJO").quote_page_url(), "https://finance.yahoo.com/quote/%5EAXJO");
        assert_eq!(symbol(".AX").asx_code(), None);
    }

    /// Answers with `price`, or fails when that's `None`.
    struct Fake {
        covers: bool,
        price: Option<f64>,
    }

    impl QuoteSource for Fake {
        fn covers(&self, _: &Symbol) -> bool {
            self.covers
        }

        fn fetch(&self, _: &Symbol) -> Result<Quote> {
            let price = Price(self.price.context("down")?);
            Ok(Quote { name: None, price, previous_close: price, currency: None, time: None, session: None })
        }
    }

    fn fake(covers: bool, price: Option<f64>) -> Box<dyn QuoteSource> {
        Box::new(Fake { covers, price })
    }

    #[test]
    fn fallback_uses_first_covering_source_that_answers() {
        let sources = Fallback(vec![fake(true, None), fake(false, Some(1.0)), fake(true, Some(2.0))]);
        assert_eq!(sources.fetch(&symbol("WTC.AX")).unwrap().price, Price(2.0));
    }

    #[test]
    fn fallback_reports_every_failure() {
        let sources = Fallback(vec![fake(true, None), fake(true, None)]);
        assert_eq!(sources.fetch(&symbol("WTC.AX")).unwrap_err().to_string(), "down; down");
    }

    /// Hits the real APIs: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_fetch() {
        let agent = http_agent();
        for s in ["WTC.AX", "MSFT"] {
            let quote = Yahoo(agent.clone()).fetch(&symbol(s)).unwrap();
            assert!(quote.price.0 > 0.0 && quote.time.is_some() && quote.session.is_some(), "{quote:?}");
        }
        assert!(Asx(agent.clone()).fetch(&symbol("WTC.AX")).unwrap().price.0 > 0.0);
        let both = Fallback(vec![Box::new(Yahoo(agent.clone())), Box::new(Asx(agent))]);
        let err = format!("{:#}", both.fetch(&symbol("ZZZQ.AX")).unwrap_err());
        assert!(err.starts_with("Yahoo: ") && err.contains("; ASX: "), "{err}");
    }
}
