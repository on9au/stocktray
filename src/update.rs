//! Self-update from GitHub releases. The latest tag comes from the `/releases/latest` redirect
//! (a plain page, so no API rate limit), and installers follow the name `package.ps1` gives them.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use semver::Version;

/// How often to look for a release, besides at startup and when asked.
const CHECK_EVERY: Duration = Duration::from_secs(12 * 60 * 60);

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo versions are semver")
}

/// The installer's file name; must match `OutputBaseFilename` in `installer/stocktray.iss`.
pub fn installer_name(version: &Version) -> String {
    format!("stocktray-{version}-setup.exe")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub installer_url: String,
}

pub trait ReleaseFeed: Send {
    /// The newest release, or `None` before the first one.
    fn latest(&self) -> Result<Option<Release>>;

    fn download(&self, release: &Release, to: &Path) -> Result<()>;
}

pub struct GitHub {
    pub agent: ureq::Agent,
    /// `owner/name`
    pub repo: String,
}

impl GitHub {
    /// The repository named in Cargo.toml.
    pub fn from_manifest(agent: ureq::Agent) -> Self {
        let repo = env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/").to_owned();
        Self { agent, repo }
    }

    /// Reads a release from where `/releases/latest` redirects, e.g. `…/releases/tag/v1.2.3`.
    fn release_at(&self, location: &str) -> Result<Release> {
        let tag = location.rsplit_once("/tag/").context("not a release tag URL")?.1;
        let version = Version::parse(tag.trim_start_matches('v'))?;
        let installer_url =
            format!("https://github.com/{}/releases/download/{tag}/{}", self.repo, installer_name(&version));
        Ok(Release { version, installer_url })
    }
}

impl ReleaseFeed for GitHub {
    fn latest(&self) -> Result<Option<Release>> {
        let url = format!("https://github.com/{}/releases/latest", self.repo);
        let request = self.agent.get(&url).config().max_redirects(0).max_redirects_will_error(false).build();
        let response = request.call()?;
        if response.status() == 404 {
            return Ok(None);
        }
        let location = response.headers().get("location").context("no redirect to a release")?.to_str()?;
        self.release_at(location).map(Some)
    }

    fn download(&self, release: &Release, to: &Path) -> Result<()> {
        let mut response = self.agent.get(&release.installer_url).call()?;
        ensure!(response.status().is_success(), "HTTP {} fetching the installer", response.status());
        std::io::copy(&mut response.body_mut().as_reader(), &mut File::create(to)?)?;
        Ok(())
    }
}

pub enum Command {
    Check,
    Download(Release),
}

pub enum Event {
    /// A release newer than this one, if there is one.
    Checked(Result<Option<Release>, String>),
    Downloaded(Result<PathBuf, String>),
}

/// Checks `feed` now, every [`CHECK_EVERY`] and when asked, on its own thread, passing results to
/// `notify`, which returns false once nobody is listening.
pub fn spawn(feed: impl ReleaseFeed + 'static, notify: impl Fn(Event) -> bool + Send + 'static) -> Sender<Command> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut command = Command::Check;
        loop {
            let event = match command {
                Command::Check => {
                    Event::Checked(feed.latest().map(|r| r.filter(|r| r.version > current_version())).map_err(text))
                }
                Command::Download(release) => {
                    let to = std::env::temp_dir().join(installer_name(&release.version));
                    Event::Downloaded(feed.download(&release, &to).map(|()| to).map_err(text))
                }
            };
            if !notify(event) {
                return;
            }
            command = match rx.recv_timeout(CHECK_EVERY) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => Command::Check,
                Err(RecvTimeoutError::Disconnected) => return,
            };
        }
    });
    tx
}

fn text(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Where the app is in the update cycle, which drives the update menu item.
#[derive(Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release),
    Failed(String),
}

impl State {
    /// Moves on after an [`Event`], returning the installer to run once it's downloaded.
    pub fn apply(&mut self, event: Event) -> Option<PathBuf> {
        *self = match event {
            Event::Checked(Ok(Some(release))) => Self::Available(release),
            Event::Checked(Ok(None)) => Self::UpToDate,
            Event::Checked(Err(e)) | Event::Downloaded(Err(e)) => Self::Failed(e),
            Event::Downloaded(Ok(installer)) => return Some(installer),
        };
        None
    }

    /// What clicking the menu item asks of the update thread, if anything.
    pub fn click(&mut self) -> Option<Command> {
        let (state, command) = match std::mem::take(self) {
            Self::Available(release) => (Self::Downloading(release.clone()), Some(Command::Download(release))),
            busy @ (Self::Checking | Self::Downloading(_)) => (busy, None),
            Self::UpToDate | Self::Failed(_) => (Self::Checking, Some(Command::Check)),
        };
        *self = state;
        command
    }

    /// The menu item's text, and whether it can be clicked.
    pub fn menu_item(&self) -> (String, bool) {
        match self {
            Self::Checking => ("Checking for updates…".to_owned(), false),
            Self::UpToDate => (format!("Up to date (v{}) · check again", current_version()), true),
            Self::Available(release) => (format!("Install update v{}", release.version), true),
            Self::Downloading(release) => (format!("Downloading v{}…", release.version), false),
            Self::Failed(_) => ("Update failed · retry".to_owned(), true),
        }
    }

    /// A tooltip line, when there's news.
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Available(release) => Some(format!("Update v{} available", release.version)),
            Self::Failed(e) => Some(format!("⚠ Update failed: {e}")),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github() -> GitHub {
        GitHub { agent: ureq::agent(), repo: "on9au/stocktray".to_owned() }
    }

    fn release(version: &str) -> Release {
        github().release_at(&format!("https://github.com/on9au/stocktray/releases/tag/v{version}")).unwrap()
    }

    #[test]
    fn reads_release_from_redirect() {
        let release = release("1.2.3");
        assert_eq!(release.version, Version::new(1, 2, 3));
        assert_eq!(
            release.installer_url,
            "https://github.com/on9au/stocktray/releases/download/v1.2.3/stocktray-1.2.3-setup.exe"
        );
        assert!(github().release_at("https://github.com/on9au/stocktray/releases").is_err());
    }

    #[test]
    fn manifest_names_the_repo() {
        assert_eq!(GitHub::from_manifest(ureq::agent()).repo, "on9au/stocktray");
    }

    #[test]
    fn clicking_walks_the_cycle() {
        let mut state = State::UpToDate;
        assert!(matches!(state.click(), Some(Command::Check)));
        assert_eq!(state, State::Checking);
        assert!(state.click().is_none(), "no second check while one is running");

        assert_eq!(state.apply(Event::Checked(Ok(Some(release("9.0.0"))))), None);
        assert!(matches!(state.click(), Some(Command::Download(r)) if r.version.major == 9));
        assert_eq!(state.menu_item(), ("Downloading v9.0.0…".to_owned(), false));

        let installer = PathBuf::from("setup.exe");
        assert_eq!(state.apply(Event::Downloaded(Ok(installer.clone()))), Some(installer));
    }

    #[test]
    fn failures_offer_a_retry() {
        let mut state = State::Checking;
        state.apply(Event::Checked(Err("offline".to_owned())));
        assert_eq!(state.note().as_deref(), Some("⚠ Update failed: offline"));
        assert!(matches!(state.click(), Some(Command::Check)));
    }

    /// Hits github.com: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_latest() {
        let feed = GitHub { agent: crate::quote::http_agent(), repo: "googleapis/release-please".to_owned() };
        assert!(feed.latest().unwrap().unwrap().version.major >= 17);
    }
}
