//! Yahoo Finance's chart API: covers every exchange, and reports the quote time and session.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use time::{OffsetDateTime, UtcOffset};

use super::{Price, Quote, QuoteSource, QuoteTime, Symbol, get_json};

pub struct Yahoo(pub ureq::Agent);

impl QuoteSource for Yahoo {
    fn fetch(&self, symbol: &Symbol) -> Result<Quote> {
        let url =
            format!("https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d", symbol.url_encoded());
        get_json::<Response>(&self.0, &url).and_then(Response::into_quote).context("Yahoo")
    }
}

#[derive(Deserialize)]
struct Response {
    chart: Chart,
}

#[derive(Deserialize)]
struct Chart {
    result: Option<Vec<ChartResult>>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    description: String,
}

#[derive(Deserialize)]
struct ChartResult {
    meta: Meta,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    currency: Option<String>,
    long_name: Option<String>,
    short_name: Option<String>,
    regular_market_price: f64,
    chart_previous_close: f64,
    #[serde(with = "time::serde::timestamp")]
    regular_market_time: OffsetDateTime,
    /// The exchange's UTC offset, in seconds.
    gmtoffset: i32,
    timezone: String,
    current_trading_period: Option<TradingPeriods>,
}

#[derive(Deserialize)]
struct TradingPeriods {
    regular: Period,
}

#[derive(Deserialize)]
struct Period {
    #[serde(with = "time::serde::timestamp")]
    start: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    end: OffsetDateTime,
}

impl Response {
    fn into_quote(self) -> Result<Quote> {
        if let Some(error) = self.chart.error {
            bail!(error.description);
        }
        let meta = self.chart.result.and_then(|results| results.into_iter().next()).context("no result")?.meta;
        let offset = UtcOffset::from_whole_seconds(meta.gmtoffset)?;
        Ok(Quote {
            name: meta.long_name.or(meta.short_name),
            price: Price(meta.regular_market_price),
            previous_close: Price(meta.chart_previous_close),
            currency: meta.currency,
            time: Some(QuoteTime { at: meta.regular_market_time.to_offset(offset), zone: meta.timezone }),
            session: meta
                .current_trading_period
                .map(|p| p.regular.start.to_offset(offset)..p.regular.end.to_offset(offset)),
        })
    }
}

#[cfg(test)]
mod tests {
    use time::macros::{datetime, offset};

    use super::*;

    fn parse(json: &str) -> Result<Quote> {
        serde_json::from_str::<Response>(json)?.into_quote()
    }

    #[test]
    fn parses_quote() {
        let quote = parse(
            r#"{"chart":{"result":[{"meta":{"currency":"AUD","symbol":"WTC.AX",
            "regularMarketTime":1792988254,"gmtoffset":39600,"timezone":"AEDT",
            "regularMarketPrice":40.575,"longName":"WiseTech Global Limited",
            "chartPreviousClose":40.12,"currentTradingPeriod":{"regular":{"timezone":"AEDT",
            "start":1792969200,"end":1792991520,"gmtoffset":39600}}}}],"error":null}}"#,
        )
        .unwrap();
        assert_eq!(quote.name.as_deref(), Some("WiseTech Global Limited"));
        assert_eq!((quote.price, quote.previous_close), (Price(40.575), Price(40.12)));
        let time = quote.time.unwrap();
        assert_eq!(
            (time.at, time.at.offset(), time.zone.as_str()),
            (datetime!(2026-10-26 15:17:34 +11), offset!(+11), "AEDT")
        );
        assert_eq!(quote.session, Some(datetime!(2026-10-26 10:00 +11)..datetime!(2026-10-26 16:12 +11)));
    }

    #[test]
    fn surfaces_api_error() {
        let err = parse(
            r#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found, symbol may be delisted"}}}"#,
        );
        assert_eq!(err.unwrap_err().to_string(), "No data found, symbol may be delisted");
    }
}
