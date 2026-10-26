//! Background thread that fetches quotes: often while the market is open, rarely otherwise.

use std::ops::Range;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use time::OffsetDateTime;

use crate::config::Config;
use crate::quote::{Quote, QuoteSource, Symbol};

/// Free feeds lag the market by about 20 minutes, so keep polling often for a while after the
/// close to catch the final price.
const DELAYED_FEED_TAIL: time::Duration = time::Duration::minutes(30);

pub enum Command {
    Refresh,
    SetConfig(Config),
}

pub struct Fetched {
    /// The symbol fetched, so a result that lands after a symbol change can be dropped.
    pub symbol: Symbol,
    pub result: Result<Quote, String>,
}

/// Polls `source` on its own thread, passing each result to `notify`, which returns false once
/// nobody is listening.
pub fn spawn(
    source: impl QuoteSource + 'static,
    mut config: Config,
    notify: impl Fn(Fetched) -> bool + Send + 'static,
) -> Sender<Command> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // The ASX reports no session, so remember the last one Yahoo gave.
        let mut session = None;
        loop {
            let result = source.fetch(&config.symbol);
            let delay = match &result {
                Ok(quote) => {
                    session = quote.session.clone().or(session);
                    next_delay(OffsetDateTime::now_utc(), session.as_ref(), &config)
                }
                Err(_) => config.poll_open,
            };
            if !notify(Fetched { symbol: config.symbol.clone(), result: result.map_err(|e| format!("{e:#}")) }) {
                return;
            }
            let first = match rx.recv_timeout(delay) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            // Refresh clicks that queued up while waiting add up to one fetch.
            for command in std::iter::once(first).chain(rx.try_iter()) {
                if let Command::SetConfig(new) = command {
                    if new.symbol != config.symbol {
                        session = None;
                    }
                    config = new;
                }
            }
        }
    });
    tx
}

pub fn next_delay(now: OffsetDateTime, session: Option<&Range<OffsetDateTime>>, config: &Config) -> Duration {
    match session {
        // Unknown hours: assume open.
        None => config.poll_open,
        // Wake just after the open, not up to a whole closed interval late.
        Some(s) if now < s.start => config.poll_closed.min((s.start - now).unsigned_abs() + Duration::from_secs(5)),
        Some(s) if now <= s.end + DELAYED_FEED_TAIL => config.poll_open,
        Some(_) => config.poll_closed,
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    const OPEN: OffsetDateTime = datetime!(2026-10-26 10:00 +11);
    const CLOSE: OffsetDateTime = datetime!(2026-10-26 16:12 +11);

    fn delay(now: OffsetDateTime) -> u64 {
        next_delay(now, Some(&(OPEN..CLOSE)), &Config::default()).as_secs()
    }

    #[test]
    fn often_while_open_and_just_after_close() {
        assert_eq!(delay(OPEN), 60);
        assert_eq!(delay(CLOSE + DELAYED_FEED_TAIL), 60);
    }

    #[test]
    fn rarely_once_the_tail_is_over() {
        assert_eq!(delay(CLOSE + DELAYED_FEED_TAIL + time::Duration::SECOND), 900);
    }

    #[test]
    fn wakes_for_the_open() {
        assert_eq!(delay(OPEN - time::Duration::seconds(100)), 105);
        assert_eq!(delay(OPEN - time::Duration::hours(3)), 900);
    }

    #[test]
    fn unknown_session_polls_often() {
        assert_eq!(next_delay(OPEN, None, &Config::default()).as_secs(), 60);
    }
}
