//! The tray's right-click menu, which turns clicks into [`Action`]s.

use anyhow::Result;
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Refresh,
    OpenQuotePage,
    EditSettings,
    /// The tick's new state; the menu toggles it before reporting the click.
    SetAutostart(bool),
    Exit,
}

pub struct TrayMenu {
    pub menu: Menu,
    /// The disabled first item, naming the stock.
    pub header: MenuItem,
    pub autostart: CheckMenuItem,
    actions: Vec<(MenuId, Action)>,
}

impl TrayMenu {
    pub fn new(autostart: bool) -> Result<Self> {
        let item = |text| MenuItem::new(text, true, None);
        let (refresh, open, edit, exit) =
            (item("Refresh now"), item("Open quote page"), item("Edit settings…"), item("Exit"));
        let header = MenuItem::new("", false, None);
        let autostart = CheckMenuItem::new("Start with Windows", true, autostart, None);
        let separator = PredefinedMenuItem::separator;
        let menu = Menu::with_items(&[
            &header,
            &separator(),
            &refresh,
            &open,
            &edit,
            &separator(),
            &autostart,
            &separator(),
            &exit,
        ])?;
        let actions = [
            (refresh, Action::Refresh),
            (open, Action::OpenQuotePage),
            (edit, Action::EditSettings),
            (exit, Action::Exit),
        ]
        .map(|(item, action)| (item.id().clone(), action))
        .into();
        Ok(Self { menu, header, autostart, actions })
    }

    pub fn action(&self, id: &MenuId) -> Option<Action> {
        if id == self.autostart.id() {
            return Some(Action::SetAutostart(self.autostart.is_checked()));
        }
        self.actions.iter().find(|(item, _)| item == id).map(|&(_, action)| action)
    }
}
