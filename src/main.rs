// Release builds have no console window; debug builds keep one for panics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menu;
mod win;

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use stocktray::config::{Config, ConfigFile};
use stocktray::quote::{Asx, Fallback, Yahoo, http_agent};
use stocktray::update::{self, GitHub};
use stocktray::view::{Feed, View};
use stocktray::{icon, poller};
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
    Quote(poller::Fetched),
    Update(update::Event),
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
            Event::UserEvent(UserEvent::Quote(fetched)) => app.on_quote(fetched),
            Event::UserEvent(UserEvent::Update(event)) => app.on_update(event),
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

/// A thread's `notify` callback, or a tray-icon event handler, that re-sends its events into the
/// event loop. Returns false once the loop has gone.
fn forward<E>(
    proxy: &EventLoopProxy<UserEvent>,
    wrap: fn(E) -> UserEvent,
) -> impl Fn(E) -> bool + Send + Sync + use<E> {
    let proxy = proxy.clone();
    move |event| proxy.send_event(wrap(event)).is_ok()
}

struct App {
    tray: TrayIcon,
    menu: TrayMenu,
    font: Option<icon::Font>,
    icon_size: u32,
    config_file: ConfigFile,
    config: Config,
    config_error: Option<String>,
    poller: Sender<poller::Command>,
    feed: Feed,
    updater: Sender<update::Command>,
    update: update::State,
    next_tick: Instant,
    last_tick: SystemTime,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Result<Self> {
        let (on_menu, on_tray) = (forward(&proxy, UserEvent::Menu), forward(&proxy, UserEvent::Tray));
        MenuEvent::set_event_handler(Some(move |e| {
            on_menu(e);
        }));
        TrayIconEvent::set_event_handler(Some(move |e| {
            on_tray(e);
        }));

        let mut config_file = ConfigFile::in_appdata();
        let (config, config_error) = match config_file.load() {
            Ok(config) => (config, None),
            Err(e) => (Config::default(), Some(format!("{e:#}"))),
        };
        let menu = TrayMenu::new(win::autostart_enabled())?;
        let tray =
            TrayIconBuilder::new().with_menu(Box::new(menu.menu.clone())).with_menu_on_left_click(false).build()?;
        let agent = http_agent();
        let source = Fallback(vec![Box::new(Yahoo(agent.clone())), Box::new(Asx(agent.clone()))]);

        let mut app = Self {
            tray,
            menu,
            font: icon::load_font(),
            icon_size: win::tray_icon_size(),
            config_file,
            poller: poller::spawn(source, config.clone(), forward(&proxy, UserEvent::Quote)),
            config,
            config_error,
            feed: Feed::Loading,
            updater: update::spawn(GitHub::from_manifest(agent), forward(&proxy, UserEvent::Update)),
            update: update::State::Checking,
            next_tick: Instant::now() + TICK,
            last_tick: SystemTime::now(),
        };
        app.render();
        Ok(app)
    }

    fn render(&mut self) {
        let config_note = self.config_error.as_ref().map(|e| format!("⚠ Settings ignored: {e}"));
        let notes: Vec<String> = config_note.into_iter().chain(self.update.note()).collect();
        let view = View::new(&self.config.symbol, &self.feed, &notes, OffsetDateTime::now_utc());
        let rgba = icon::render(self.font.as_ref(), &view.lines, view.trend, self.icon_size);
        if let Ok(icon) = Icon::from_rgba(rgba, self.icon_size, self.icon_size) {
            let _ = self.tray.set_icon(Some(icon));
        }
        let _ = self.tray.set_tooltip(Some(view.tooltip));
        self.menu.header.set_text(view.header);
        let (text, enabled) = self.update.menu_item();
        self.menu.update.set_text(text);
        self.menu.update.set_enabled(enabled);
    }

    fn on_quote(&mut self, fetched: poller::Fetched) {
        // A result for the previous symbol can land after a settings change.
        if fetched.symbol == self.config.symbol {
            self.feed.update(fetched.result);
            self.render();
        }
    }

    fn on_update(&mut self, event: update::Event) {
        if let Some(installer) = self.update.apply(event) {
            match win::run_installer(&installer) {
                Ok(()) => self.exit(),
                Err(e) => self.update = update::State::Failed(format!("couldn't start the installer: {e}")),
            }
        }
        self.render();
    }

    fn tick(&mut self) {
        let now = SystemTime::now();
        if now.duration_since(self.last_tick).unwrap_or_default() > TICK + SLEEP_GAP {
            let _ = self.poller.send(poller::Command::Refresh);
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
                    let _ = self.poller.send(poller::Command::SetConfig(config));
                }
            }
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Refresh => drop(self.poller.send(poller::Command::Refresh)),
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
            Action::Update => {
                if let Some(command) = self.update.click() {
                    let _ = self.updater.send(command);
                    self.render();
                }
            }
            Action::Exit => self.exit(),
        }
    }

    fn exit(&self) -> ! {
        // Exiting skips destructors, so remove the icon first or the tray keeps a ghost.
        let _ = self.tray.set_visible(false);
        std::process::exit(0)
    }
}
