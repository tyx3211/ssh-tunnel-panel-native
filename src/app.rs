use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::panic;
use std::path::PathBuf;

use async_channel::Receiver;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Application, Global, UpdateGlobal as _, WeakEntity,
    WindowBounds, WindowOptions, px, rgb, size,
};
use gpui_component::{Root, Theme, ThemeMode, TitleBar};
use thiserror::Error;

use crate::assets::AppAssets;
use crate::manager::{ManagerError, TunnelManager};
use crate::single_instance::SingleInstance;
use crate::store::{JsonStore, StoreError};
use crate::tray::{TrayAction, TrayController, TrayError};
use crate::ui::PanelView;
use crate::window_visibility::{
    WindowVisibilityError, activate_existing_window, hide_window, show_window,
};

#[derive(Debug, Error)]
enum StartupError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Manager(#[from] ManagerError),
    #[error(transparent)]
    Tray(#[from] TrayError),
}

pub(crate) struct AppController {
    pub manager: TunnelManager,
    tray: TrayController,
    window: Option<AnyWindowHandle>,
    panel: Option<WeakEntity<PanelView>>,
}

impl Global for AppController {}

impl AppController {
    fn initialize() -> Result<(Self, Receiver<()>), StartupError> {
        let (wake_sender, wake_receiver) = async_channel::bounded(1);
        let store = JsonStore::prepare_default()?;
        let manager = TunnelManager::initialize_notifying(store, wake_sender.clone())?;
        let tray = TrayController::new(wake_sender)?;
        Ok((
            Self {
                manager,
                tray,
                window: None,
                panel: None,
            },
            wake_receiver,
        ))
    }
}

pub fn run() {
    install_panic_hook();
    let instance = match SingleInstance::acquire() {
        Ok(instance) => instance,
        Err(error) => {
            write_diagnostic("single-instance", &error.to_string());
            return;
        }
    };
    if !instance.is_primary() {
        if let Err(error) = activate_existing_window() {
            write_diagnostic("activate-existing-window", &error.to_string());
        }
        return;
    }

    Application::new().with_assets(AppAssets).run(|cx| {
        gpui_component::init(cx);
        let (controller, wake_receiver) = match AppController::initialize() {
            Ok(initialized) => initialized,
            Err(error) => {
                write_diagnostic("startup", &error.to_string());
                cx.quit();
                return;
            }
        };
        cx.set_global(controller);

        cx.on_window_closed(|cx| {
            AppController::update_global(cx, |controller, _| {
                controller.window = None;
                controller.panel = None;
            });
        })
        .detach();

        cx.on_app_quit(|cx| {
            AppController::update_global(cx, |controller, _| controller.manager.shutdown());
            async {}
        })
        .detach();

        open_panel(cx);
        start_event_loop(wake_receiver, cx);
    });
}

fn open_panel(cx: &mut App) {
    if let Some(handle) = cx.global::<AppController>().window {
        match handle.update(cx, |_, window, _| {
            show_window(window)?;
            window.activate_window();
            Ok::<(), WindowVisibilityError>(())
        }) {
            Ok(Ok(())) => {
                cx.activate(true);
                return;
            }
            Ok(Err(error)) => write_diagnostic("show-window", &error.to_string()),
            Err(_) => {}
        }
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1180.), px(760.)), cx)),
        window_min_size: Some(size(px(920.), px(620.))),
        titlebar: Some(TitleBar::title_bar_options()),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        window.set_window_title("SSH Tunnel Panel");
        window.on_window_should_close(cx, |window, _cx| match hide_window(window) {
            Ok(()) => false,
            Err(error) => {
                write_diagnostic("hide-window", &error.to_string());
                true
            }
        });
        Theme::change(ThemeMode::Dark, Some(window), cx);
        let theme = Theme::global_mut(cx);
        theme.font_family = ".SystemUIFont".into();
        theme.font_size = px(16.);
        theme.mono_font_family = "Cascadia Code".into();
        theme.radius = px(6.);
        theme.colors.background = rgb(0x0010_1317).into();
        theme.colors.foreground = rgb(0x00d7_dee8).into();
        theme.colors.input = rgb(0x0035_3c45).into();
        theme.colors.border = rgb(0x0034_3a40).into();
        theme.colors.secondary = rgb(0x001a_1d21).into();
        theme.colors.secondary_hover = rgb(0x0024_2930).into();
        theme.colors.secondary_active = rgb(0x0030_3741).into();
        theme.colors.primary = rgb(0x001f_6f5f).into();
        theme.colors.primary_hover = rgb(0x0028_7f6d).into();
        let panel = cx.new(|cx| PanelView::new(window, cx));
        AppController::update_global(cx, |controller, _| {
            controller.panel = Some(panel.downgrade());
        });
        cx.new(|cx| Root::new(panel, window, cx))
    }) {
        Ok(handle) => {
            AppController::update_global(cx, |controller, _| {
                controller.window = Some(handle.into());
            });
            cx.activate(true);
        }
        Err(error) => write_diagnostic("window", &error.to_string()),
    }
}

fn start_event_loop(wake_receiver: Receiver<()>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while wake_receiver.recv().await.is_ok() {
            if cx
                .update(|cx| {
                    let (changed, actions, panel) =
                        AppController::update_global(cx, |controller, _| {
                            let changed = controller.manager.drain_events();
                            let actions = controller.tray.drain_actions();
                            (changed, actions, controller.panel.clone())
                        });

                    if changed && let Some(panel) = panel.and_then(|panel| panel.upgrade()) {
                        panel.update(cx, |_, cx| cx.notify());
                    }

                    for action in actions {
                        handle_tray_action(action, cx);
                    }
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

fn handle_tray_action(action: TrayAction, cx: &mut App) {
    match action {
        TrayAction::Show => open_panel(cx),
        TrayAction::StartAll => {
            AppController::update_global(cx, |controller, _| {
                let _ = controller.manager.start_all();
            });
            notify_panel(cx);
        }
        TrayAction::StopAll => {
            AppController::update_global(cx, |controller, _| controller.manager.stop_all());
            notify_panel(cx);
        }
        TrayAction::Exit => {
            AppController::update_global(cx, |controller, _| controller.manager.shutdown());
            cx.quit();
        }
    }
}

fn notify_panel(cx: &mut App) {
    let panel = cx.global::<AppController>().panel.clone();
    if let Some(panel) = panel.and_then(|panel| panel.upgrade()) {
        panel.update(cx, |_, cx| cx.notify());
    }
}

fn install_panic_hook() {
    panic::set_hook(Box::new(|info| {
        let location = info.location().map_or_else(
            || "unknown".to_owned(),
            |location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            },
        );
        let message = info
            .payload_as_str()
            .map_or_else(|| "non-string panic payload", |message| message);
        write_diagnostic("panic", &format!("{location}: {message}"));
    }));
}

fn write_diagnostic(kind: &str, message: &str) {
    let Some(path) = diagnostic_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "[{}] {kind}: {message}",
            chrono::Local::now().to_rfc3339()
        );
    }
}

fn diagnostic_path() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join("ssh-tunnel-panel").join("diagnostics.log"))
}
