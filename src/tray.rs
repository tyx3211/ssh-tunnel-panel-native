use thiserror::Error;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    StartAll,
    StopAll,
    Exit,
}

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("加载托盘图标失败")]
    Icon(#[source] tray_icon::BadIcon),
    #[error("创建托盘菜单失败")]
    Menu(#[source] tray_icon::menu::Error),
    #[error("创建 Windows 托盘图标失败")]
    Create(#[source] tray_icon::Error),
}

pub struct TrayController {
    _icon: TrayIcon,
    action_receiver: async_channel::Receiver<TrayAction>,
}

impl TrayController {
    /// Creates the application tray icon and context menu.
    ///
    /// # Errors
    ///
    /// Returns an error when embedded icon loading or native tray/menu creation fails.
    pub fn new(wake_sender: async_channel::Sender<()>) -> Result<Self, TrayError> {
        let show = MenuItem::with_id("show", "显示面板", true, None);
        let start_all = MenuItem::with_id("start-all", "启动全部", true, None);
        let stop_all = MenuItem::with_id("stop-all", "停止全部", true, None);
        let exit = MenuItem::with_id("exit", "退出", true, None);
        let separator = PredefinedMenuItem::separator();
        let menu = Menu::with_items(&[&show, &start_all, &stop_all, &separator, &exit])
            .map_err(TrayError::Menu)?;
        let icon = Icon::from_resource(1, Some((32, 32))).map_err(TrayError::Icon)?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("SSH Tunnel Panel")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_icon(icon)
            .build()
            .map_err(TrayError::Create)?;

        let show_id = show.id().clone();
        let start_all_id = start_all.id().clone();
        let stop_all_id = stop_all.id().clone();
        let exit_id = exit.id().clone();
        let (action_sender, action_receiver) = async_channel::unbounded();
        let menu_action_sender = action_sender.clone();
        let menu_wake_sender = wake_sender.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if event.id == show_id {
                Some(TrayAction::Show)
            } else if event.id == start_all_id {
                Some(TrayAction::StartAll)
            } else if event.id == stop_all_id {
                Some(TrayAction::StopAll)
            } else if event.id == exit_id {
                Some(TrayAction::Exit)
            } else {
                None
            };
            if let Some(action) = action {
                let _ = menu_action_sender.try_send(action);
                let _ = menu_wake_sender.try_send(());
            }
        }));

        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = action_sender.try_send(TrayAction::Show);
                let _ = wake_sender.try_send(());
            }
        }));

        Ok(Self {
            _icon: tray,
            action_receiver,
        })
    }

    #[must_use]
    pub fn drain_actions(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(action) = self.action_receiver.try_recv() {
            actions.push(action);
        }
        actions
    }
}
