use std::collections::BTreeMap;

use gpui::{
    AnyElement, App, Context, Entity, IntoElement, Render, SharedString,
    StatefulInteractiveElement as _, UpdateGlobal as _, Window, WindowControlArea, div, prelude::*,
    px, rgb, rgba,
};
use gpui_component::{
    Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::app::AppController;
use crate::assets::AppIcon;
use crate::model::{
    ForwardConfig, ForwardDraft, ForwardId, ForwardMode, ForwardStatus, ForwardView, SshHost,
};

const SIDEBAR_WIDTH: f32 = 260.;
const EDITOR_WIDTH: f32 = 360.;

const APP_BG: u32 = 0x0010_1214;
const TITLEBAR_BG: u32 = 0x0015_181c;
const WORKSPACE_HEADER_BG: u32 = 0x0011_1417;
const PANEL_BG: u32 = 0x0017_1b20;
const INPUT_BG: u32 = 0x0010_1317;
const LOG_BG: u32 = 0x000d_1013;
const BORDER: u32 = 0x002b_3138;
const SHELL_BORDER: u32 = 0x0025_2a30;
const FOREGROUND: u32 = 0x00d7_dee8;
const STRONG_FOREGROUND: u32 = 0x00f5_f7fb;
const MUTED: u32 = 0x008f_9aa8;
const SUBTLE: u32 = 0x009b_a6b5;
const SELECTED_BG: u32 = 0x0026_3241;
const SELECTED_BORDER: u32 = 0x003b_4b60;
const CARD_SELECTED_BG: u32 = 0x001b_2430;
const CARD_SELECTED_BORDER: u32 = 0x005b_7da5;
const PRIMARY_BG: u32 = 0x001f_6f5f;
const PRIMARY_BORDER: u32 = 0x002c_8d78;
const MODE_BG: u32 = 0x002d_3138;
const WARNING: u32 = 0x00fd_d663;
const ERROR: u32 = 0x00ff_9f89;

pub(crate) struct PanelView {
    selected_host: Option<String>,
    selected_forward: Option<ForwardId>,
    draft_id: Option<ForwardId>,
    draft_mode: ForwardMode,
    name: Entity<InputState>,
    host_alias: Entity<InputState>,
    bind_address: Entity<InputState>,
    listen_port: Entity<InputState>,
    target_host: Entity<InputState>,
    target_port: Entity<InputState>,
    message: Option<Message>,
}

#[derive(Clone)]
struct Message {
    kind: MessageKind,
    text: SharedString,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Error,
    Info,
}

impl PanelView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            selected_host: None,
            selected_forward: None,
            draft_id: None,
            draft_mode: ForwardMode::L,
            name: input_state("Dev server", "", window, cx),
            host_alias: input_state("Host alias", "", window, cx),
            bind_address: input_state("本地地址", "127.0.0.1", window, cx),
            listen_port: input_state("本地端口", "3000", window, cx),
            target_host: input_state("目标地址", "127.0.0.1", window, cx),
            target_port: input_state("目标端口", "3000", window, cx),
            message: None,
        }
    }

    fn new_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.selected_host.clone().unwrap_or_else(|| {
            cx.global::<AppController>()
                .manager
                .hosts()
                .first()
                .map(|item| item.alias.clone())
                .unwrap_or_default()
        });
        self.draft_id = None;
        self.selected_forward = None;
        self.draft_mode = ForwardMode::L;
        self.message = None;
        set_input(&self.name, "", window, cx);
        set_input(&self.host_alias, host, window, cx);
        set_input(&self.bind_address, "127.0.0.1", window, cx);
        set_input(&self.listen_port, "3000", window, cx);
        set_input(&self.target_host, "127.0.0.1", window, cx);
        set_input(&self.target_port, "3000", window, cx);
        cx.notify();
    }

    fn load_forward(
        &mut self,
        forward: &ForwardConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_forward = Some(forward.id.clone());
        self.draft_id = Some(forward.id.clone());
        self.draft_mode = forward.mode;
        self.message = None;
        set_input(&self.name, &forward.name, window, cx);
        set_input(&self.host_alias, &forward.host_alias, window, cx);
        set_input(&self.bind_address, &forward.bind_address, window, cx);
        set_input(
            &self.listen_port,
            forward.listen_port.to_string(),
            window,
            cx,
        );
        set_input(&self.target_host, &forward.target_host, window, cx);
        set_input(
            &self.target_port,
            forward.target_port.to_string(),
            window,
            cx,
        );
        cx.notify();
    }

    fn save_draft(&mut self, cx: &mut Context<Self>) {
        let listen_port = match parse_port(&self.listen_port, cx) {
            Ok(port) => port,
            Err(message) => {
                self.set_error(message, cx);
                return;
            }
        };
        let target_port = match parse_port(&self.target_port, cx) {
            Ok(port) => port,
            Err(message) => {
                self.set_error(message, cx);
                return;
            }
        };
        let draft = ForwardDraft {
            id: self.draft_id.clone(),
            name: input_value(&self.name, cx),
            host_alias: input_value(&self.host_alias, cx),
            mode: self.draft_mode,
            bind_address: input_value(&self.bind_address, cx),
            listen_port,
            target_host: input_value(&self.target_host, cx),
            target_port,
        };
        let result = AppController::update_global(cx, |controller, _| {
            controller.manager.save_forward(draft)
        });
        match result {
            Ok(id) => {
                self.selected_forward = Some(id.clone());
                self.draft_id = Some(id);
                self.message = Some(Message {
                    kind: MessageKind::Info,
                    text: "已保存".into(),
                });
            }
            Err(error) => self.set_error(error.to_string(), cx),
        }
        cx.notify();
    }

    fn delete_forward(&mut self, id: &ForwardId, window: &mut Window, cx: &mut Context<Self>) {
        let result =
            AppController::update_global(cx, |controller, _| controller.manager.delete_forward(id));
        match result {
            Ok(()) => {
                if self.draft_id.as_ref() == Some(id) {
                    self.new_draft(window, cx);
                } else {
                    cx.notify();
                }
            }
            Err(error) => self.set_error(error.to_string(), cx),
        }
    }

    fn start_forward(&mut self, id: &ForwardId, cx: &mut Context<Self>) {
        self.message = None;
        let result =
            AppController::update_global(cx, |controller, _| controller.manager.start_forward(id));
        if let Err(error) = result {
            self.set_error(error.to_string(), cx);
        } else {
            self.selected_forward = Some(id.clone());
            cx.notify();
        }
    }

    fn stop_forward(&mut self, id: &ForwardId, cx: &mut Context<Self>) {
        self.message = None;
        self.selected_forward = Some(id.clone());
        AppController::update_global(cx, |controller, _| {
            controller.manager.stop_forward(id);
        });
        cx.notify();
    }

    fn clear_logs(id: &ForwardId, cx: &mut Context<Self>) {
        AppController::update_global(cx, |controller, _| {
            controller.manager.clear_logs(id);
        });
        cx.notify();
    }

    fn start_all(&mut self, cx: &mut Context<Self>) {
        let failures =
            AppController::update_global(cx, |controller, _| controller.manager.start_all());
        self.message = if failures.is_empty() {
            None
        } else {
            Some(Message {
                kind: MessageKind::Error,
                text: format!("{} 个转发启动失败", failures.len()).into(),
            })
        };
        cx.notify();
    }

    fn stop_all(&mut self, cx: &mut Context<Self>) {
        AppController::update_global(cx, |controller, _| controller.manager.stop_all());
        self.message = None;
        cx.notify();
    }

    fn refresh_hosts(&mut self, cx: &mut Context<Self>) {
        let result =
            AppController::update_global(cx, |controller, _| controller.manager.refresh_hosts());
        self.message = result.err().map(|error| Message {
            kind: MessageKind::Error,
            text: error.to_string().into(),
        });
        cx.notify();
    }

    fn set_error(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.message = Some(Message {
            kind: MessageKind::Error,
            text: message.into(),
        });
        cx.notify();
    }

    fn render_titlebar(window: &Window) -> impl IntoElement {
        h_flex()
            .h(px(34.))
            .flex_shrink_0()
            .bg(rgb(TITLEBAR_BG))
            .border_b_1()
            .border_color(rgb(SHELL_BORDER))
            .child(
                h_flex()
                    .id("titlebar-drag-area")
                    .flex_1()
                    .h_full()
                    .pl(px(14.))
                    .gap_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(rgb(0x00cf_d8e5))
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        Icon::new(AppIcon::Cable)
                            .small()
                            .text_color(rgb(0x0081_c995)),
                    )
                    .child("SSH Tunnel Panel"),
            )
            .child(window_control(
                "window-minimize",
                IconName::WindowMinimize,
                WindowControlArea::Min,
                false,
            ))
            .child(window_control(
                "window-maximize",
                if window.is_maximized() {
                    IconName::WindowRestore
                } else {
                    IconName::WindowMaximize
                },
                WindowControlArea::Max,
                false,
            ))
            .child(window_control(
                "window-close",
                IconName::WindowClose,
                WindowControlArea::Close,
                true,
            ))
    }

    fn render_sidebar(
        &self,
        hosts: &[SshHost],
        forwards: &[ForwardView],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut counts = BTreeMap::<&str, usize>::new();
        for forward in forwards {
            *counts.entry(&forward.config.host_alias).or_default() += 1;
        }

        v_flex()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(rgb(SHELL_BORDER))
            .bg(rgb(TITLEBAR_BG))
            .px_2()
            .py_3()
            .child(
                h_flex()
                    .h(px(42.))
                    .px_2()
                    .gap_2()
                    .font_semibold()
                    .text_color(rgb(STRONG_FOREGROUND))
                    .child(Icon::new(AppIcon::Cable).small())
                    .child("SSH Tunnels"),
            )
            .child(Self::host_row(
                "host-all",
                AppIcon::CircleDot,
                "全部转发",
                forwards.len(),
                self.selected_host.is_none(),
                cx,
            ))
            .child(
                h_flex()
                    .mt_4()
                    .mx_2()
                    .mb_2()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("SSH HOSTS")
                    .child(
                        Button::new("refresh-hosts")
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Redo2))
                            .tooltip("刷新 SSH config")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_hosts(cx))),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(hosts.iter().map(|host| {
                        Self::host_row(
                            SharedString::from(format!("host-{}", host.id)),
                            AppIcon::Server,
                            &host.alias,
                            counts.get(host.alias.as_str()).copied().unwrap_or_default(),
                            self.selected_host.as_deref() == Some(host.alias.as_str()),
                            cx,
                        )
                    })),
            )
    }

    fn host_row(
        id: impl Into<SharedString>,
        icon: AppIcon,
        alias: &str,
        count: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_alias = alias.to_owned();
        h_flex()
            .id(id.into())
            .w_full()
            .min_h(px(36.))
            .px_2()
            .gap_2()
            .rounded(px(6.))
            .border_1()
            .border_color(if selected {
                rgb(SELECTED_BORDER)
            } else {
                rgba(0x0000_0000)
            })
            .bg(if selected {
                rgb(SELECTED_BG)
            } else {
                rgba(0x0000_0000)
            })
            .hover(|style| style.bg(rgb(0x0024_2930)))
            .cursor_pointer()
            .child(Icon::new(icon).small().flex_shrink_0())
            .child(div().flex_1().min_w_0().truncate().child(alias.to_owned()))
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(rgb(MUTED))
                    .child(count.to_string()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                if selected_alias == "全部转发" {
                    this.selected_host = None;
                } else {
                    this.selected_host = Some(selected_alias.clone());
                    if this.draft_id.is_none() {
                        set_input(&this.host_alias, selected_alias.clone(), window, cx);
                    }
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.selected_host.as_deref().unwrap_or("转发面板");
        h_flex()
            .flex_shrink_0()
            .px(px(22.))
            .pt(px(18.))
            .pb(px(14.))
            .gap_5()
            .border_b_1()
            .border_color(rgb(SHELL_BORDER))
            .bg(rgb(WORKSPACE_HEADER_BG))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(22.))
                            .line_height(px(27.5))
                            .font_bold()
                            .text_color(rgb(STRONG_FOREGROUND))
                            .child(title.to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child("持久化配置，手动启动，托盘退出时清理所有 SSH 进程。"),
                    ),
            )
            .child(
                h_flex()
                    .mt_1()
                    .gap_2()
                    .child(
                        toolbar_button("toolbar-start-all", AppIcon::Zap, "一键启动")
                            .on_click(cx.listener(|this, _, _, cx| this.start_all(cx))),
                    )
                    .child(
                        toolbar_button("toolbar-stop-all", AppIcon::CircleStop, "停止全部")
                            .on_click(cx.listener(|this, _, _, cx| this.stop_all(cx))),
                    )
                    .child(
                        toolbar_button("toolbar-new", IconName::Plus, "新建")
                            .bg(rgb(PRIMARY_BG))
                            .border_color(rgb(PRIMARY_BORDER))
                            .text_color(rgb(0x00ff_ffff))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_draft(window, cx);
                            })),
                    ),
            )
    }

    fn render_error(&self) -> Option<AnyElement> {
        let message = self
            .message
            .as_ref()
            .filter(|message| message.kind == MessageKind::Error)?;
        Some(
            h_flex()
                .mx(px(22.))
                .mt_3()
                .px_3()
                .py_2()
                .gap_2()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(0x008f_4d39))
                .bg(rgb(0x002a_1b18))
                .text_sm()
                .text_color(rgb(0x00ff_b49d))
                .child(Icon::new(IconName::TriangleAlert).small())
                .child(message.text.clone())
                .into_any_element(),
        )
    }

    fn render_forward_list(
        &self,
        forwards: &[ForwardView],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let filtered: Vec<_> = forwards
            .iter()
            .filter(|forward| {
                self.selected_host
                    .as_ref()
                    .is_none_or(|host| forward.config.host_alias == *host)
            })
            .collect();

        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scrollbar()
            .when(filtered.is_empty(), |this| {
                this.child(
                    v_flex()
                        .h(px(180.))
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .rounded(px(8.))
                        .border_1()
                        .border_dashed()
                        .border_color(rgb(0x0034_3a40))
                        .text_color(rgb(MUTED))
                        .child(Icon::new(AppIcon::Cable).size_6())
                        .child("还没有转发配置"),
                )
            })
            .children(
                filtered
                    .into_iter()
                    .map(|forward| self.render_forward_row(forward, cx)),
            )
    }

    fn render_forward_row(&self, forward: &ForwardView, cx: &mut Context<Self>) -> AnyElement {
        let config = &forward.config;
        let runtime = &forward.runtime;
        let id = config.id.clone();
        let selected = self.selected_forward.as_ref() == Some(&id);
        let active = matches!(
            runtime.status,
            ForwardStatus::Starting | ForwardStatus::Running
        );
        let port_label = runtime.actual_listen_port.map_or_else(
            || config.listen_port.to_string(),
            |actual| {
                if actual == config.listen_port {
                    actual.to_string()
                } else {
                    format!("{} -> {actual}", config.listen_port)
                }
            },
        );
        let endpoint = format!(
            "{} / {}:{port_label} / {}:{}",
            config.host_alias, config.bind_address, config.target_host, config.target_port
        );
        let config_for_edit = config.clone();
        let start_or_stop_id = id.clone();
        let edit_config = config.clone();
        let clear_id = id.clone();
        let delete_id = id.clone();

        h_flex()
            .id(SharedString::from(format!("forward-{}", id.0)))
            .w_full()
            .min_h(px(56.))
            .mb(px(10.))
            .p(px(9.))
            .gap_3()
            .rounded(px(8.))
            .border_1()
            .border_color(if selected {
                rgb(CARD_SELECTED_BORDER)
            } else {
                rgb(BORDER)
            })
            .bg(if selected {
                rgb(CARD_SELECTED_BG)
            } else {
                rgb(PANEL_BG)
            })
            .hover(|style| style.bg(rgb(0x001b_2026)))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.load_forward(&config_for_edit, window, cx);
            }))
            .child(
                div()
                    .size(px(34.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(rgb(MODE_BG))
                    .font_bold()
                    .text_color(rgb(WARNING))
                    .child(mode_label(config.mode)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_semibold()
                            .truncate()
                            .child(config.name.clone()),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_xs()
                            .font_family("Cascadia Code")
                            .text_color(rgb(SUBTLE))
                            .truncate()
                            .child(endpoint),
                    )
                    .when(
                        runtime
                            .actual_listen_port
                            .is_some_and(|actual| actual != config.listen_port),
                        |this| {
                            this.child(div().mt_1().text_xs().text_color(rgb(WARNING)).child(
                                format!(
                                    "请求端口 {} 被占用，已改用 {}",
                                    config.listen_port,
                                    runtime.actual_listen_port.unwrap_or(config.listen_port)
                                ),
                            ))
                        },
                    )
                    .when_some(runtime.error.clone(), |this, error| {
                        this.child(
                            div()
                                .mt_1()
                                .text_xs()
                                .text_color(rgb(ERROR))
                                .truncate()
                                .child(error),
                        )
                    }),
            )
            .child(status_badge(runtime.status))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        row_icon_button(
                            SharedString::from(format!("toggle-{}", id.0)),
                            if active {
                                AppIcon::Square
                            } else {
                                AppIcon::Zap
                            },
                            if active { "停止" } else { "启动" },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if active {
                                this.stop_forward(&start_or_stop_id, cx);
                            } else {
                                this.start_forward(&start_or_stop_id, cx);
                            }
                        })),
                    )
                    .child(
                        row_icon_button(
                            SharedString::from(format!("edit-{}", id.0)),
                            AppIcon::Save,
                            "编辑",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.load_forward(&edit_config, window, cx);
                            },
                        )),
                    )
                    .child(
                        row_icon_button(
                            SharedString::from(format!("clear-{}", id.0)),
                            AppIcon::Eraser,
                            "清空日志",
                        )
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            Self::clear_logs(&clear_id, cx);
                        })),
                    )
                    .child(
                        row_icon_button(
                            SharedString::from(format!("delete-{}", id.0)),
                            AppIcon::Trash,
                            "删除",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.delete_forward(&delete_id, window, cx);
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    fn render_editor(&self, forwards: &[ForwardView], cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.draft_id.is_some();
        let active = self.draft_id.as_ref().is_some_and(|id| {
            forwards.iter().any(|forward| {
                &forward.config.id == id
                    && matches!(
                        forward.runtime.status,
                        ForwardStatus::Starting | ForwardStatus::Running
                    )
            })
        });
        let selected = self
            .selected_forward
            .as_ref()
            .and_then(|id| forwards.iter().find(|forward| &forward.config.id == id));

        v_flex()
            .w(px(EDITOR_WIDTH))
            .h_full()
            .min_h_0()
            .flex_shrink_0()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL_BG))
            .child(
                h_flex()
                    .h(px(58.))
                    .px_3()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(div().text_sm().font_semibold().child(if editing {
                        "编辑转发"
                    } else {
                        "新建转发"
                    }))
                    .child(
                        Button::new("save-forward")
                            .xsmall()
                            .icon(Icon::new(AppIcon::Save))
                            .tooltip("保存")
                            .w(px(32.))
                            .h(px(32.))
                            .bg(rgb(PRIMARY_BG))
                            .border_color(rgb(PRIMARY_BORDER))
                            .text_color(rgb(0x00ff_ffff))
                            .disabled(active)
                            .on_click(cx.listener(|this, _, _, cx| this.save_draft(cx))),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .p_3()
                    .child(form_field("名称", panel_input(&self.name)))
                    .child(form_field("SSH 主机", panel_input(&self.host_alias)))
                    .child(
                        v_flex().gap(px(6.)).child(field_label("方向")).child(
                            h_flex()
                                .id("direction-toggle")
                                .h(px(34.))
                                .px_2()
                                .justify_between()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(rgb(0x0035_3c45))
                                .bg(rgb(INPUT_BG))
                                .font_family(".SystemUIFont")
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .cursor_pointer()
                                .child(match self.draft_mode {
                                    ForwardMode::L => "-L 本地监听",
                                    ForwardMode::R => "-R 远程监听",
                                })
                                .child(Icon::new(IconName::ChevronDown).xsmall())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.draft_mode = match this.draft_mode {
                                        ForwardMode::L => ForwardMode::R,
                                        ForwardMode::R => ForwardMode::L,
                                    };
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                form_field("本地地址", panel_input(&self.bind_address))
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                form_field("本地端口", panel_input(&self.listen_port)).w(px(110.)),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                form_field("目标地址", panel_input(&self.target_host))
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                form_field("目标端口", panel_input(&self.target_port)).w(px(110.)),
                            ),
                    )
                    .when(active, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child("运行中的转发需停止后才能修改"),
                        )
                    })
                    .when_some(selected, |this, forward| {
                        this.when(
                            forward
                                .runtime
                                .actual_listen_port
                                .is_some_and(|actual| actual != forward.config.listen_port),
                            |this| {
                                this.child(
                                    div()
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(rgb(0x006c_5520))
                                        .bg(rgb(0x0024_1f12))
                                        .px_2()
                                        .py_2()
                                        .text_xs()
                                        .text_color(rgb(WARNING))
                                        .child(format!(
                                            "请求端口 {} 被占用，当前使用 {}",
                                            forward.config.listen_port,
                                            forward
                                                .runtime
                                                .actual_listen_port
                                                .unwrap_or(forward.config.listen_port)
                                        )),
                                )
                            },
                        )
                    }),
            )
            .child(Self::render_logs(selected, cx))
    }

    fn render_logs(selected: Option<&ForwardView>, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = selected
            .map(|forward| forward.runtime.logs.clone())
            .unwrap_or_default();
        let clear_id = selected.map(|forward| forward.config.id.clone());

        v_flex()
            .flex_1()
            .min_h(px(120.))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                h_flex()
                    .h(px(40.))
                    .px_3()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(SUBTLE))
                    .child("日志")
                    .when_some(clear_id, |this, id| {
                        this.child(
                            Button::new("clear-selected-logs")
                                .xsmall()
                                .ghost()
                                .icon(Icon::new(AppIcon::Eraser))
                                .tooltip("清空日志")
                                .on_click(cx.listener(move |_this, _, _, cx| {
                                    Self::clear_logs(&id, cx);
                                })),
                        )
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_3()
                    .py_2()
                    .gap_1()
                    .bg(rgb(LOG_BG))
                    .font_family("Cascadia Code")
                    .text_xs()
                    .line_height(px(18.))
                    .text_color(rgb(0x00c9_d3df))
                    .when(lines.is_empty(), |this| {
                        this.child(
                            div()
                                .font_family(".SystemUIFont")
                                .text_size(px(12.))
                                .line_height(px(18.6))
                                .child("选择一条转发查看日志。"),
                        )
                    })
                    .children(
                        lines
                            .iter()
                            .cloned()
                            .map(|line| div().child(SharedString::from(line))),
                    ),
            )
    }
}

impl Render for PanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let controller = cx.global::<AppController>();
        let hosts = controller.manager.hosts();
        let forwards = controller.manager.forwards();

        v_flex()
            .size_full()
            .bg(rgb(APP_BG))
            .font_family(".SystemUIFont")
            .text_color(rgb(FOREGROUND))
            .child(Self::render_titlebar(window))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(&hosts, &forwards, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .child(self.render_toolbar(cx))
                            .children(self.render_error())
                            .child(
                                h_flex()
                                    .flex_1()
                                    .h_full()
                                    .min_h_0()
                                    .gap(px(18.))
                                    .px(px(22.))
                                    .py(px(18.))
                                    .child(self.render_forward_list(&forwards, cx))
                                    .child(self.render_editor(&forwards, cx)),
                            ),
                    ),
            )
    }
}

fn input_state(
    placeholder: impl Into<SharedString>,
    value: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<PanelView>,
) -> Entity<InputState> {
    let placeholder = placeholder.into();
    let value = value.into();
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

fn set_input(
    input: &Entity<InputState>,
    value: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<PanelView>,
) {
    input.update(cx, |state, cx| state.set_value(value, window, cx));
}

fn input_value(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).value().to_string()
}

fn parse_port(input: &Entity<InputState>, cx: &App) -> Result<u16, &'static str> {
    input_value(input, cx)
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("端口必须在 1 到 65535 之间")
}

fn field_label(label: &'static str) -> impl IntoElement {
    div()
        .font_family(".SystemUIFont")
        .text_size(px(12.))
        .line_height(px(15.))
        .text_color(rgb(SUBTLE))
        .child(label)
}

fn form_field(label: &'static str, input: Input) -> gpui::Div {
    v_flex().gap(px(6.)).child(field_label(label)).child(input)
}

fn panel_input(state: &Entity<InputState>) -> Input {
    Input::new(state)
        .min_h(px(34.))
        .px(px(10.))
        .py_0()
        .font_family(".SystemUIFont")
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(rgb(0x00ee_f3fa))
}

fn toolbar_button(
    id: impl Into<gpui::ElementId>,
    icon: impl Into<Icon>,
    label: &'static str,
) -> Button {
    Button::new(id)
        .small()
        .h(px(34.))
        .px_2()
        .icon(icon)
        .label(label)
}

fn row_icon_button(
    id: impl Into<gpui::ElementId>,
    icon: impl Into<Icon>,
    tooltip: &'static str,
) -> Button {
    Button::new(id)
        .xsmall()
        .ghost()
        .w(px(38.))
        .h(px(36.))
        .icon(icon)
        .tooltip(tooltip)
}

fn window_control(
    id: &'static str,
    icon: IconName,
    area: WindowControlArea,
    close: bool,
) -> AnyElement {
    let hover_background = if close { 0x00c4_2b1c } else { 0x0024_2930 };
    div()
        .id(id)
        .w(px(46.))
        .h_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(0x00b8_c0cc))
        .window_control_area(area)
        .hover(move |style| style.bg(rgb(hover_background)).text_color(rgb(0x00ff_ffff)))
        .child(Icon::new(icon).small())
        .into_any_element()
}

fn mode_label(mode: ForwardMode) -> &'static str {
    match mode {
        ForwardMode::L => "L",
        ForwardMode::R => "R",
    }
}

fn status_label(status: ForwardStatus) -> &'static str {
    match status {
        ForwardStatus::Stopped => "未启动",
        ForwardStatus::Starting => "启动中",
        ForwardStatus::Running => "运行中",
        ForwardStatus::Failed => "失败",
    }
}

fn status_colors(status: ForwardStatus) -> (gpui::Rgba, gpui::Rgba) {
    match status {
        ForwardStatus::Stopped => (rgb(0x002a_2e34), rgb(0x00b8_c0cc)),
        ForwardStatus::Starting => (rgb(0x0030_2a17), rgb(WARNING)),
        ForwardStatus::Running => (rgb(0x0013_2820), rgb(0x0081_c995)),
        ForwardStatus::Failed => (rgb(0x0032_1b18), rgb(ERROR)),
    }
}

fn status_badge(status: ForwardStatus) -> impl IntoElement {
    let (background, foreground) = status_colors(status);
    div()
        .w(px(82.))
        .flex_shrink_0()
        .flex()
        .justify_start()
        .child(
            div()
                .min_w(px(66.))
                .px_2()
                .py_1()
                .rounded_full()
                .bg(background)
                .text_center()
                .text_xs()
                .text_color(foreground)
                .child(status_label(status)),
        )
}
