// Release builds have no console window; debug builds keep one for panics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menu;
mod win;

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use stocktray::config::{Config, ConfigFile};
use stocktray::icon;
use stocktray::poller::{self, Command, Update};
use stocktray::quote::{Asx, Fallback, Yahoo, http_agent};
use stocktray::view::{Feed, View};
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use time::OffsetDateTime;
use tray_icon::menu::MenuEvent;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::menu::{Action, TrayMenu};

/// How often to check the config file, the taskbar DPI and the wall clock.
const TICK: Duration = Duration::from_secs(2);
/// A tick this much later than due means the machine slept, so the quote is old.
const SLEEP_GAP: Duration = Duration::from_secs(30);

enum UserEvent {
    Quote(Update),
    Menu(MenuEvent),
    Tray(TrayIconEvent),
}

fn main() {
    if !win::claim_single_instance() {
        return win::message_box("stocktray is already running. If you can't see it, click ^ in the taskbar.");
    }
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let mut app = match App::new(event_loop.create_proxy()) {
        Ok(app) => app,
        Err(e) => return win::message_box(&format!("stocktray couldn't start: {e:#}")),
    };
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::NewEvents(_) if Instant::now() >= app.next_tick => app.tick(),
            Event::UserEvent(UserEvent::Quote(update)) => app.on_update(update),
            Event::UserEvent(UserEvent::Menu(e)) => {
                if let Some(action) = app.menu.action(&e.id) {
                    app.run(action);
                }
            }
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::DoubleClick { .. })) => app.run(Action::OpenQuotePage),
            _ => {}
        }
        *control_flow = ControlFlow::WaitUntil(app.next_tick);
    });
}

/// A tray-icon event handler that re-sends its events into the event loop.
fn forward<E>(proxy: EventLoopProxy<UserEvent>, wrap: fn(E) -> UserEvent) -> impl Fn(E) + Send + Sync {
    move |event| drop(proxy.send_event(wrap(event)))
}

struct App {
    tray: TrayIcon,
    menu: TrayMenu,
    font: Option<icon::Font>,
    icon_size: u32,
    config_file: ConfigFile,
    config: Config,
    config_error: Option<String>,
    poller: Sender<Command>,
    feed: Feed,
    next_tick: Instant,
    last_tick: SystemTime,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Result<Self> {
        MenuEvent::set_event_handler(Some(forward(proxy.clone(), UserEvent::Menu)));
        TrayIconEvent::set_event_handler(Some(forward(proxy.clone(), UserEvent::Tray)));

        let mut config_file = ConfigFile::in_appdata();
        let (config, config_error) = match config_file.load() {
            Ok(config) => (config, None),
            Err(e) => (Config::default(), Some(format!("{e:#}"))),
        };
        let menu = TrayMenu::new(win::autostart_enabled())?;
        let tray =
            TrayIconBuilder::new().with_menu(Box::new(menu.menu.clone())).with_menu_on_left_click(false).build()?;
        let agent = http_agent();
        let source = Fallback(vec![Box::new(Yahoo(agent.clone())), Box::new(Asx(agent))]);
        let poller =
            poller::spawn(source, config.clone(), move |update| proxy.send_event(UserEvent::Quote(update)).is_ok());

        let mut app = Self {
            tray,
            menu,
            font: icon::load_font(),
            icon_size: win::tray_icon_size(),
            config_file,
            config,
            config_error,
            poller,
            feed: Feed::Loading,
            next_tick: Instant::now() + TICK,
            last_tick: SystemTime::now(),
        };
        app.render();
        Ok(app)
    }

    fn render(&mut self) {
        let view = View::new(&self.config.symbol, &self.feed, self.config_error.as_deref(), OffsetDateTime::now_utc());
        let rgba = icon::render(self.font.as_ref(), &view.lines, view.trend, self.icon_size);
        if let Ok(icon) = Icon::from_rgba(rgba, self.icon_size, self.icon_size) {
            let _ = self.tray.set_icon(Some(icon));
        }
        let _ = self.tray.set_tooltip(Some(view.tooltip));
        self.menu.header.set_text(view.header);
    }

    fn on_update(&mut self, update: Update) {
        // A result for the previous symbol can land after a settings change.
        if update.symbol == self.config.symbol {
            self.feed.update(update.result);
            self.render();
        }
    }

    fn tick(&mut self) {
        let now = SystemTime::now();
        if now.duration_since(self.last_tick).unwrap_or_default() > TICK + SLEEP_GAP {
            let _ = self.poller.send(Command::Refresh);
        }
        self.last_tick = now;
        self.next_tick = Instant::now() + TICK;

        let size = win::tray_icon_size();
        let mut changed = size != self.icon_size;
        self.icon_size = size;
        if let Some(result) = self.config_file.reload_if_changed() {
            self.apply_config(result);
            changed = true;
        }
        if changed {
            self.render();
        }
    }

    fn apply_config(&mut self, result: Result<Config>) {
        match result {
            Err(e) => self.config_error = Some(format!("{e:#}")),
            Ok(config) => {
                self.config_error = None;
                if config.symbol != self.config.symbol {
                    self.feed = Feed::Loading;
                }
                if config != self.config {
                    self.config = config.clone();
                    let _ = self.poller.send(Command::SetConfig(config));
                }
            }
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Refresh => drop(self.poller.send(Command::Refresh)),
            Action::OpenQuotePage => win::open_url(&self.config.symbol.quote_page_url()),
            Action::EditSettings => {
                if let Err(e) = win::edit_file(&self.config_file.path) {
                    win::message_box(&format!("Couldn't open {}: {e}", self.config_file.path.display()));
                }
            }
            Action::SetAutostart(on) => {
                if let Err(e) = win::set_autostart(on) {
                    self.menu.autostart.set_checked(!on);
                    win::message_box(&format!("Couldn't change the startup setting: {e}"));
                }
            }
            Action::Exit => {
                // Exiting skips destructors, so remove the icon first or the tray keeps a ghost.
                let _ = self.tray.set_visible(false);
                std::process::exit(0);
            }
        }
    }
}
