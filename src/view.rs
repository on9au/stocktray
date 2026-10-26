//! What the tray shows, derived from the latest fetch: the icon's text and colour, the hover
//! tooltip and the menu header.

use std::cmp::Ordering;

use time::OffsetDateTime;
use time::macros::format_description;

use crate::icon::Trend;
use crate::quote::{Price, Quote, QuoteTime, Symbol};

/// Windows tray tooltips hold 128 UTF-16 units, including the terminator.
const TOOLTIP_MAX: usize = 127;

/// The latest fetch outcome. A failure keeps the last good quote, shown as stale.
#[derive(Debug, Default)]
pub enum Feed {
    #[default]
    Loading,
    Live(Quote),
    Failed {
        last: Option<Quote>,
        error: String,
    },
}

impl Feed {
    pub fn update(&mut self, result: Result<Quote, String>) {
        *self = match (std::mem::take(self), result) {
            (_, Ok(quote)) => Self::Live(quote),
            (Self::Live(last), Err(error)) => Self::Failed { last: Some(last), error },
            (Self::Failed { last, .. }, Err(error)) => Self::Failed { last, error },
            (Self::Loading, Err(error)) => Self::Failed { last: None, error },
        }
    }

    fn quote(&self) -> Option<&Quote> {
        match self {
            Self::Live(quote) | Self::Failed { last: Some(quote), .. } => Some(quote),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct View {
    /// Rows of text for the icon, top to bottom.
    pub lines: Vec<String>,
    pub trend: Trend,
    pub tooltip: String,
    pub header: String,
}

impl View {
    /// `notes` are extra tooltip lines, such as a settings error or an available update.
    pub fn new(symbol: &Symbol, feed: &Feed, notes: &[String], now: OffsetDateTime) -> Self {
        let (lines, trend, status) = match feed {
            Feed::Loading => (vec!["…".to_owned()], Trend::Unknown, Some("Loading…".to_owned())),
            Feed::Live(quote) => (icon_lines(quote.price), trend(quote.change()), None),
            Feed::Failed { last, error } => (
                last.as_ref().map_or_else(|| vec!["?".to_owned()], |quote| icon_lines(quote.price)),
                Trend::Unknown,
                Some(format!("⚠ Offline: {error}")),
            ),
        };
        let quote = feed.quote();
        let mut tooltip = quote.map_or_else(|| vec![symbol.to_string()], |q| quote_lines(symbol, q, now));
        tooltip.extend(status);
        tooltip.extend_from_slice(notes);
        Self {
            lines,
            trend,
            tooltip: truncate_utf16(&tooltip.join("\n"), TOOLTIP_MAX),
            header: match quote.and_then(|q| q.name.as_deref()) {
                Some(name) => format!("{symbol} · {name}"),
                None => symbol.to_string(),
            },
        }
    }
}

/// Dollars above cents: 40.57 shows as `40` over `57`. Truncated rather than rounded, so the
/// dollars match the tooltip. From $10,000 there's no room for cents: 12,345.67 shows as `12k`.
fn icon_lines(price: Price) -> Vec<String> {
    // The epsilon stops 0.29 becoming 28.999… cents.
    let cents = (price.0 * 100.0 + 1e-6) as u64;
    match (cents / 100, cents % 100) {
        (dollars, _) if dollars >= 10_000 => vec![format!("{}k", dollars / 1000)],
        (dollars, cents) => vec![dollars.to_string(), format!("{cents:02}")],
    }
}

fn trend(change: Price) -> Trend {
    match change.0.partial_cmp(&0.0) {
        Some(Ordering::Greater) => Trend::Up,
        Some(Ordering::Less) => Trend::Down,
        _ => Trend::Flat,
    }
}

fn quote_lines(symbol: &Symbol, quote: &Quote, now: OffsetDateTime) -> Vec<String> {
    let dp = quote.price.decimals();
    let currency = quote.currency.as_deref().unwrap_or_default();
    let change = quote.change();
    let arrow = match trend(change) {
        Trend::Up => "▲",
        Trend::Down => "▼",
        _ => "=",
    };
    let mut lines = vec![
        format!("{symbol}  {:.dp$} {currency}", quote.price.0).trim_end().to_owned(),
        format!("{arrow} {:+.dp$} ({:+.2}%)", change.0, quote.change_pct()),
    ];
    if let Some(time) = &quote.time {
        let closed = quote.session.as_ref().is_some_and(|s| !s.contains(&now));
        lines.push(format!("as of {}{}", format_time(time, now), if closed { " · closed" } else { "" }));
    }
    lines
}

/// Exchange-local time such as `3:57 pm AEDT`, with the weekday unless it's today there.
fn format_time(time: &QuoteTime, now: OffsetDateTime) -> String {
    let at = time.at;
    let clock = if at.date() == now.to_offset(at.offset()).date() {
        at.format(format_description!("[hour repr:12 padding:none]:[minute] [period case:lower]"))
    } else {
        at.format(format_description!("[weekday repr:short] [hour repr:12 padding:none]:[minute] [period case:lower]"))
    };
    format!("{} {}", clock.unwrap_or_default(), time.zone).trim_end().to_owned()
}

fn truncate_utf16(s: &str, max: usize) -> String {
    if s.encode_utf16().count() <= max {
        return s.to_owned();
    }
    let mut units = 0;
    // `<` rather than `<=` leaves room for the ellipsis.
    let mut out: String = s
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units < max
        })
        .collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    const STRUCK: OffsetDateTime = datetime!(2026-10-26 15:57 +11);

    fn wtc() -> Quote {
        Quote {
            name: Some("WiseTech Global Limited".into()),
            price: Price(40.57),
            previous_close: Price(40.12),
            currency: Some("AUD".into()),
            time: Some(QuoteTime { at: STRUCK, zone: "AEDT".into() }),
            session: Some(datetime!(2026-10-26 10:00 +11)..datetime!(2026-10-26 16:12 +11)),
        }
    }

    fn view(feed: &Feed, now: OffsetDateTime) -> View {
        View::new(&"WTC.AX".parse().unwrap(), feed, &[], now)
    }

    #[test]
    fn icon_shows_dollars_over_cents() {
        assert_eq!(icon_lines(Price(40.57)), ["40", "57"]);
        assert_eq!(icon_lines(Price(0.29)), ["0", "29"]);
        assert_eq!(icon_lines(Price(9.999)), ["9", "99"]);
        assert_eq!(icon_lines(Price(1234.5)), ["1234", "50"]);
        assert_eq!(icon_lines(Price(12_345.67)), ["12k"]);
    }

    #[test]
    fn live_while_open() {
        let view = view(&Feed::Live(wtc()), STRUCK);
        assert_eq!(view.lines, ["40", "57"]);
        assert_eq!(view.trend, Trend::Up);
        assert_eq!(view.tooltip, "WTC.AX  40.57 AUD\n▲ +0.45 (+1.12%)\nas of 3:57 pm AEDT");
        assert_eq!(view.header, "WTC.AX · WiseTech Global Limited");
    }

    #[test]
    fn failure_keeps_the_last_quote_as_stale() {
        let mut feed = Feed::Live(wtc());
        feed.update(Err("timed out".into()));
        let view = view(&feed, datetime!(2026-10-27 09:00 +11));
        assert_eq!((view.lines.len(), view.trend), (2, Trend::Unknown));
        assert_eq!(
            view.tooltip,
            "WTC.AX  40.57 AUD\n▲ +0.45 (+1.12%)\nas of Mon 3:57 pm AEDT · closed\n⚠ Offline: timed out"
        );
    }

    #[test]
    fn before_any_quote() {
        assert_eq!(view(&Feed::Loading, STRUCK).tooltip, "WTC.AX\nLoading…");
        let mut feed = Feed::Loading;
        feed.update(Err("x".repeat(300)));
        let view = view(&feed, STRUCK);
        assert_eq!(view.lines, ["?"]);
        assert_eq!(view.tooltip.encode_utf16().count(), TOOLTIP_MAX);
        assert!(view.tooltip.ends_with('…'));
    }
}
