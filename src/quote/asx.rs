//! The ASX's own API: ASX listings only, with no quote time or session.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{Price, Quote, QuoteSource, Symbol, get_json};

pub struct Asx(pub ureq::Agent);

impl QuoteSource for Asx {
    fn covers(&self, symbol: &Symbol) -> bool {
        symbol.asx_code().is_some()
    }

    fn fetch(&self, symbol: &Symbol) -> Result<Quote> {
        let code = symbol.asx_code().context("not an ASX symbol")?;
        let url = format!("https://asx.api.markitdigital.com/asx-research/1.0/companies/{code}/header");
        get_json::<Response>(&self.0, &url).and_then(Response::into_quote).context("ASX")
    }
}

#[derive(Deserialize)]
struct Response {
    data: Option<Data>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    display_name: Option<String>,
    price_last: f64,
    price_change: f64,
}

impl Response {
    fn into_quote(self) -> Result<Quote> {
        if let Some(error) = self.error {
            bail!(error.message);
        }
        let data = self.data.context("no data")?;
        Ok(Quote {
            name: data.display_name,
            price: Price(data.price_last),
            previous_close: Price(data.price_last - data.price_change),
            currency: Some("AUD".to_owned()),
            time: None,
            session: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Quote> {
        serde_json::from_str::<Response>(json)?.into_quote()
    }

    #[test]
    fn parses_quote() {
        let quote = parse(r#"{"data":{"displayName":"WISETECH GLOBAL LIMITED","priceLast":40.57,"priceChange":0.45}}"#)
            .unwrap();
        assert_eq!(quote.price, Price(40.57));
        assert!((quote.previous_close.0 - 40.12).abs() < 1e-9);
    }

    #[test]
    fn surfaces_api_error() {
        let err = parse(r#"{"error":{"code":400,"message":"Bad Request: Symbol not found"}}"#);
        assert_eq!(err.unwrap_err().to_string(), "Bad Request: Symbol not found");
    }
}
