//! The browser panel: a workspace's browser, drawn by zedd and shared with its agents.

mod dev_servers;

use dev_servers::DevServer;

use anyhow::Result;
use browser::{Browser, BrowserEvent, KeyModifiers, MouseAction, Viewport};
use context_server::listener::McpServer;
use editor::Editor;
use fs::Fs;
use gpui::{
    Action, App, AsyncWindowContext, Bounds, ClipboardItem, Context, Entity, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ObjectFit, Pixels, RenderImage, ScrollWheelEvent, Subscription, Task,
    TaskExt as _, WeakEntity, Window, actions, canvas, img, px,
};
use project::{Project, project_settings::ContextServerSettings};
use settings::{DockSide, IntoGpui as _, RegisterSetting, Settings, SettingsStore};
use std::{cell::Cell, path::PathBuf, rc::Rc, sync::Arc};
use ui::{IconButton, IconName, IconSize, Label, LabelSize, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

actions!(
    browser_panel,
    [
        /// Toggles focus on the browser panel.
        ToggleFocus,
    ]
);

const BROWSER_PANEL_KEY: &str = "BrowserPanel";

/// The MCP server name agents see the browser tools under.
const AGENT_TOOLS_SERVER_ID: &str = "zedd-browser";

/// How far one line of a line-based scroll wheel moves the page.
const SCROLL_LINE_HEIGHT: Pixels = px(40.);

#[derive(Clone, Debug, PartialEq, RegisterSetting)]
pub struct BrowserSettings {
    pub button: bool,
    pub dock: DockSide,
    pub default_width: Pixels,
    pub chrome_path: Option<PathBuf>,
    pub agent_tools: bool,
}

impl Settings for BrowserSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let browser = content.browser.as_ref().unwrap();
        Self {
            button: browser.button.unwrap(),
            dock: browser.dock.unwrap(),
            default_width: browser.default_width.unwrap().into_gpui(),
            chrome_path: browser.chrome_path.clone().map(PathBuf::from),
            agent_tools: browser.agent_tools.unwrap(),
        }
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<BrowserPanel>(window, cx);
        });
    })
    .detach();
}

pub struct BrowserPanel {
    browser: Entity<Browser>,
    workspace: WeakEntity<Workspace>,
    /// The project whose agents get the browser tools, when this panel serves them.
    project: Option<Entity<Project>>,
    agent_tools_server: Option<McpServer>,
    address_bar: Entity<Editor>,
    page_focus_handle: FocusHandle,
    fs: Arc<dyn Fs>,
    /// Where the page was last drawn, to turn window positions into page positions.
    page_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The frame drawn last, released from the GPU when a newer one replaces it.
    shown_frame: Option<Arc<RenderImage>>,
    error: Option<SharedString>,
    dev_server_search: DevServerSearch,
    is_zoomed: bool,
    _subscriptions: Vec<Subscription>,
}

enum DevServerSearch {
    NotStarted,
    /// Holds the running search, which stops if dropped.
    Searching {
        _search: Task<()>,
    },
    Done(Vec<DevServer>),
}

impl BrowserPanel {
    pub fn load(
        workspace: WeakEntity<Workspace>,
        cx: AsyncWindowContext,
    ) -> Task<Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                let fs = workspace.app_state().fs.clone();
                let project = workspace.project().clone();
                let workspace = workspace.weak_handle();
                cx.new(|cx| {
                    let mut panel = Self::new(fs, workspace, window, cx);
                    panel.serve_tools_to_agents(project, cx);
                    panel
                })
            })
        })
    }

    fn new(
        fs: Arc<dyn Fs>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let browser = cx.new(|_| {
            Browser::new(Rc::new(|cx| {
                let chrome_path = BrowserSettings::get_global(cx).chrome_path.clone();
                browser::connect_to_shared_chrome(chrome_path, cx)
            }))
        });
        Self::with_browser(browser, fs, workspace, window, cx)
    }

    fn with_browser(
        browser: Entity<Browser>,
        fs: Arc<dyn Fs>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let address_bar = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Type a URL", window, cx);
            editor
        });
        let subscriptions = vec![
            cx.observe_in(&browser, window, |this, _, window, cx| {
                this.show_active_url(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&browser, window, |this, _, event, window, cx| match event {
                BrowserEvent::TabsChanged => this.show_active_url(window, cx),
                BrowserEvent::UsedByAgent => {
                    // Opening the panel updates it, so wait until this update is over.
                    let workspace = this.workspace.clone();
                    window.defer(cx, move |window, cx| {
                        workspace
                            .update(cx, |workspace, cx| workspace.open_panel::<Self>(window, cx))
                            .log_err();
                    });
                }
                BrowserEvent::ElementPicked { backend_node_id } => {
                    this.send_picked_element(*backend_node_id, window, cx)
                }
            }),
            cx.observe_global::<SettingsStore>(|this, cx| this.update_agent_tools(cx)),
        ];
        Self {
            browser,
            workspace,
            project: None,
            agent_tools_server: None,
            address_bar,
            page_focus_handle: cx.focus_handle(),
            fs,
            page_bounds: Rc::new(Cell::new(None)),
            shown_frame: None,
            error: None,
            dev_server_search: DevServerSearch::NotStarted,
            is_zoomed: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn browser(&self) -> &Entity<Browser> {
        &self.browser
    }

    fn serve_tools_to_agents(&mut self, project: Entity<Project>, cx: &mut Context<Self>) {
        self.project = Some(project);
        self.update_agent_tools(cx);
    }

    /// Offers the browser tools to the project's agents while `browser.agent_tools` is on and
    /// the project is on this machine, where the browser runs.
    fn update_agent_tools(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let wanted = BrowserSettings::get_global(cx).agent_tools && project.read(cx).is_local();
        let store = project.read(cx).context_server_store();
        match (wanted, self.agent_tools_server.is_some()) {
            (true, false) => match McpServer::new(AGENT_TOOLS_SERVER_ID, cx) {
                Ok(mut server) => {
                    browser::add_browser_tools(&mut server, self.browser.downgrade());
                    let settings = ContextServerSettings::Http {
                        enabled: true,
                        url: server.url().to_string(),
                        headers: [("Authorization".to_string(), server.authorization_header())]
                            .into_iter()
                            .collect(),
                        timeout: None,
                        oauth: None,
                    };
                    store.update(cx, |store, cx| {
                        store.set_built_in_server(AGENT_TOOLS_SERVER_ID.into(), settings, cx)
                    });
                    self.agent_tools_server = Some(server);
                }
                Err(error) => {
                    self.error = Some(format!("Agents cannot use the browser: {error:#}").into());
                    cx.notify();
                }
            },
            (false, true) => {
                store.update(cx, |store, cx| {
                    store.remove_built_in_server(AGENT_TOOLS_SERVER_ID, cx)
                });
                self.agent_tools_server = None;
            }
            _ => {}
        }
    }

    /// Keeps the address bar on the active tab's URL, except while the user is typing in it.
    fn show_active_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.address_bar.focus_handle(cx).is_focused(window) {
            return;
        }
        let url = self
            .browser
            .read(cx)
            .active_tab()
            .map(|tab| tab.url.clone())
            .unwrap_or_default();
        let url = if url == "about:blank" {
            String::new()
        } else {
            url
        };
        if self.address_bar.read(cx).text(cx) != url {
            self.address_bar
                .update(cx, |editor, cx| editor.set_text(url, window, cx));
        }
    }

    fn open_address(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let address = self.address_bar.read(cx).text(cx);
        if address.trim().is_empty() {
            return;
        }
        let task = self.browser.update(cx, |browser, cx| {
            if browser.active_tab().is_some() {
                browser.navigate(&address, cx)
            } else {
                browser.new_tab(Some(address), cx)
            }
        });
        window.focus(&self.page_focus_handle, cx);
        self.report_errors(task, cx);
    }

    fn send_picked_element(
        &mut self,
        backend_node_id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let description = self.browser.update(cx, |browser, cx| {
            browser.describe_element(backend_node_id, cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = description.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(description) => window.dispatch_action(
                    Box::new(zed_actions::agent::AddBrowserElementToThread { description }),
                    cx,
                ),
                Err(error) => {
                    this.error =
                        Some(format!("Could not read the picked element: {error:#}").into());
                    cx.notify();
                }
            })
        })
        .detach_and_log_err(cx);
    }

    /// Looks for web servers running from the project's folders; opens the only one found.
    fn detect_dev_servers(&mut self, cx: &mut Context<Self>) {
        let project_dirs: Vec<PathBuf> = self
            .project
            .iter()
            .flat_map(|project| project.read(cx).visible_worktrees(cx))
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
            .collect();
        let find =
            cx.background_spawn(async move { dev_servers::find_dev_servers(&project_dirs).await });
        let search = cx.spawn(async move |this, cx| {
            let result = find.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(servers) => {
                        if let [server] = servers.as_slice() {
                            this.open_url(server.url.clone(), cx);
                        }
                        this.dev_server_search = DevServerSearch::Done(servers);
                    }
                    Err(error) => {
                        this.error = Some(format!("{error:#}").into());
                        this.dev_server_search = DevServerSearch::NotStarted;
                    }
                }
                cx.notify();
            })
            .log_err();
        });
        self.dev_server_search = DevServerSearch::Searching { _search: search };
        cx.notify();
    }

    fn open_url(&mut self, url: String, cx: &mut Context<Self>) {
        let task = self
            .browser
            .update(cx, |browser, cx| browser.new_tab(Some(url), cx));
        self.report_errors(task, cx);
    }

    /// Shows a failed action's reason above the page until the next action succeeds.
    fn report_errors(&mut self, task: Task<Result<()>>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.error = result.err().map(|error| format!("{error:#}").into());
                cx.notify();
            })
        })
        .detach();
    }

    fn page_position(&self, position: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.page_bounds.get()?;
        let local = position - bounds.origin;
        Some((f32::from(local.x), f32::from(local.y)))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.page_focus_handle, cx);
        self.send_mouse(
            MouseAction::Pressed,
            event.position,
            Some(event.button),
            event.click_count,
            event.modifiers,
            cx,
        );
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.send_mouse(
            MouseAction::Released,
            event.position,
            Some(event.button),
            event.click_count,
            event.modifiers,
            cx,
        );
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let inside = self
            .page_bounds
            .get()
            .is_some_and(|bounds| bounds.contains(&event.position));
        // Keep following a drag that leaves the page, so selections and sliders end cleanly.
        if inside || event.pressed_button.is_some() {
            self.send_mouse(
                MouseAction::Moved,
                event.position,
                event.pressed_button,
                0,
                event.modifiers,
                cx,
            );
        }
    }

    fn send_mouse(
        &mut self,
        action: MouseAction,
        position: gpui::Point<Pixels>,
        button: Option<MouseButton>,
        click_count: usize,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some((x, y)) = self.page_position(position) else {
            return;
        };
        let button = match button {
            Some(MouseButton::Left) => browser::MouseButton::Left,
            Some(MouseButton::Right) => browser::MouseButton::Right,
            Some(MouseButton::Middle) => browser::MouseButton::Middle,
            _ => browser::MouseButton::None,
        };
        let task = self.browser.update(cx, |browser, cx| {
            browser.dispatch_mouse(
                action,
                x,
                y,
                button,
                click_count,
                key_modifiers(modifiers),
                cx,
            )
        });
        task.detach_and_log_err(cx);
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((x, y)) = self.page_position(event.position) else {
            return;
        };
        // zedd's deltas move the content; Chrome's move the viewport.
        let delta = event.delta.pixel_delta(SCROLL_LINE_HEIGHT);
        let task = self.browser.update(cx, |browser, cx| {
            browser.dispatch_wheel(x, y, -f32::from(delta.x), -f32::from(delta.y), cx)
        });
        task.detach_and_log_err(cx);
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let shortcut = modifiers.platform || modifiers.control;
        if modifiers.platform && !modifiers.shift && keystroke.key == "c" {
            self.copy_selection(cx);
        } else if modifiers.platform && !modifiers.shift && keystroke.key == "v" {
            // Headless Chrome has its own clipboard, so paste from zedd's.
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                let task = self
                    .browser
                    .update(cx, |browser, cx| browser.insert_text(&text, cx));
                task.detach_and_log_err(cx);
            }
        } else if let Some(text) = keystroke
            .key_char
            .as_ref()
            .filter(|text| !shortcut && !text.chars().any(char::is_control))
        {
            let task = self
                .browser
                .update(cx, |browser, cx| browser.insert_text(text, cx));
            task.detach_and_log_err(cx);
        } else if let Some(key) = dom_key(&keystroke.key) {
            let task = self.browser.update(cx, |browser, cx| {
                browser.press_key(&key, key_modifiers(modifiers), cx)
            });
            task.detach_and_log_err(cx);
        } else {
            return;
        }
        cx.stop_propagation();
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let selection = self.browser.update(cx, |browser, cx| {
            browser.send_to_active_tab(
                "Runtime.evaluate",
                serde_json::json!({ "expression": "getSelection().toString()", "returnByValue": true }),
                cx,
            )
        });
        cx.spawn(async move |_, cx| {
            let result = selection.await?;
            if let Some(text) = result["result"]["value"]
                .as_str()
                .filter(|text| !text.is_empty())
            {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text.to_string())));
            }
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let browser = self.browser.read(cx);
        let active = browser.active_tab_index();
        h_flex()
            .w_full()
            .gap_0p5()
            .px_1()
            .h_8()
            .overflow_x_hidden()
            .children(browser.tabs().iter().enumerate().map(|(index, tab)| {
                let title = [&tab.title, &tab.url]
                    .into_iter()
                    .find(|text| !text.is_empty() && *text != "about:blank")
                    .cloned()
                    .unwrap_or_else(|| "New Tab".to_string());
                h_flex()
                    .id(("browser-tab", index))
                    .min_w_0()
                    .max_w_48()
                    .h_6()
                    .pl_2()
                    .pr_0p5()
                    .gap_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(active == Some(index), |tab| {
                        tab.bg(cx.theme().colors().element_selected)
                    })
                    .hover(|tab| tab.bg(cx.theme().colors().element_hover))
                    .child(Label::new(title).size(LabelSize::Small).truncate())
                    .child(
                        IconButton::new(("close-browser-tab", index), IconName::Close)
                            .icon_size(IconSize::XSmall)
                            .tooltip(Tooltip::text("Close Tab"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let task = this
                                    .browser
                                    .update(cx, |browser, cx| browser.close_tab(index, cx));
                                this.report_errors(task, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.browser
                            .update(cx, |browser, cx| browser.activate_tab(index, cx));
                    }))
            }))
            .child(
                IconButton::new("new-browser-tab", IconName::Plus)
                    .icon_size(IconSize::Small)
                    .tooltip(Tooltip::text("New Tab"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let task = this
                            .browser
                            .update(cx, |browser, cx| browser.new_tab(None, cx));
                        this.report_errors(task, cx);
                        this.address_bar.update(cx, |editor, cx| {
                            editor.set_text("", window, cx);
                        });
                        window.focus(&this.address_bar.focus_handle(cx), cx);
                    })),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_tab = self.browser.read(cx).active_tab().is_some();
        let navigation_button =
            |id: &'static str,
             icon: IconName,
             tooltip: &'static str,
             action: fn(&mut Browser, &mut Context<Browser>) -> Task<Result<()>>| {
                IconButton::new(id, icon)
                    .icon_size(IconSize::Small)
                    .disabled(!has_tab)
                    .tooltip(Tooltip::text(tooltip))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let task = this.browser.update(cx, action);
                        this.report_errors(task, cx);
                    }))
            };
        h_flex()
            .w_full()
            .gap_1()
            .px_1()
            .pb_1()
            .child(navigation_button(
                "browser-back",
                IconName::ArrowLeft,
                "Back",
                Browser::go_back,
            ))
            .child(navigation_button(
                "browser-forward",
                IconName::ArrowRight,
                "Forward",
                Browser::go_forward,
            ))
            .child(navigation_button(
                "browser-reload",
                IconName::RotateCw,
                "Reload",
                Browser::reload,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .bg(cx.theme().colors().editor_background)
                    .on_action(cx.listener(Self::open_address))
                    .child(self.address_bar.clone()),
            )
            .child(
                IconButton::new("browser-pick-element", IconName::Crosshair)
                    .icon_size(IconSize::Small)
                    .disabled(!has_tab)
                    .toggle_state(self.browser.read(cx).is_picking_element())
                    .tooltip(Tooltip::text("Pick an Element for the Agent"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let task = this.browser.update(cx, |browser, cx| {
                            let picking = !browser.is_picking_element();
                            browser.set_picking_element(picking, cx)
                        });
                        this.report_errors(task, cx);
                        window.focus(&this.page_focus_handle, cx);
                    })),
            )
    }

    fn render_dialog(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let dialog = self.browser.read(cx).active_tab()?.dialog.clone()?;
        let answer = |accept: bool| {
            cx.listener(move |this: &mut Self, _, _, cx| {
                let task = this
                    .browser
                    .update(cx, |browser, cx| browser.handle_dialog(accept, None, cx));
                this.report_errors(task, cx);
            })
        };
        Some(
            h_flex()
                .w_full()
                .gap_2()
                .px_2()
                .py_1()
                .bg(cx.theme().colors().surface_background)
                .border_y_1()
                .border_color(cx.theme().colors().border)
                .child(Label::new(dialog.message).size(LabelSize::Small).flex_1())
                .child(Button::new("browser-dialog-accept", "OK").on_click(answer(true)))
                .when(dialog.kind != "alert", |this| {
                    this.child(
                        Button::new("browser-dialog-dismiss", "Cancel").on_click(answer(false)),
                    )
                }),
        )
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self
            .browser
            .read(cx)
            .active_tab()
            .and_then(|tab| tab.frame.clone());
        if self.shown_frame != frame
            && let Some(previous) = std::mem::replace(&mut self.shown_frame, frame.clone())
        {
            window.drop_image(previous).log_err();
        }
        let has_tab = self.browser.read(cx).active_tab().is_some();
        let page_bounds = self.page_bounds.clone();
        let browser = self.browser.clone();
        div()
            .id("browser-page")
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .track_focus(&self.page_focus_handle)
            .key_context("BrowserPage")
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        page_bounds.set(Some(bounds));
                        let viewport = Viewport {
                            width: f32::from(bounds.size.width),
                            height: f32::from(bounds.size.height),
                            scale_factor: window.scale_factor(),
                        };
                        browser.update(cx, |browser, cx| browser.set_viewport(viewport, cx));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .when_some(frame, |page, frame| {
                page.child(
                    img(frame)
                        .absolute()
                        .size_full()
                        .object_fit(ObjectFit::Fill),
                )
            })
            .when(!has_tab, |page| page.child(self.render_empty_state(cx)))
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .absolute()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .px_6()
            .child(
                Icon::new(IconName::ToolWeb)
                    .size(IconSize::XLarge)
                    .color(Color::Muted),
            )
            .child(Label::new("Browse with your agent"))
            .child(
                Label::new(
                    "Type a URL or ask an agent to open a site. Agents can read, click, and type.",
                )
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .child(self.render_dev_servers(cx))
            .bg(cx.theme().colors().editor_background)
    }
}

impl BrowserPanel {
    fn render_dev_servers(&self, cx: &mut Context<Self>) -> AnyElement {
        let detect = |label: &'static str| {
            Button::new("detect-dev-server", label)
                .label_size(LabelSize::Small)
                .on_click(cx.listener(|this, _, _, cx| this.detect_dev_servers(cx)))
        };
        match &self.dev_server_search {
            DevServerSearch::NotStarted => h_flex()
                .gap_1()
                .child(
                    Label::new("Preview your app?")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(detect("Detect dev server"))
                .into_any_element(),
            DevServerSearch::Searching { .. } => Label::new("Looking for dev servers…")
                .size(LabelSize::Small)
                .color(Color::Muted)
                .into_any_element(),
            DevServerSearch::Done(servers) if servers.is_empty() => h_flex()
                .gap_1()
                .child(
                    Label::new("No dev server is running from this project's folders.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(detect("Try again"))
                .into_any_element(),
            DevServerSearch::Done(servers) => v_flex()
                .items_center()
                .gap_0p5()
                .children(servers.iter().enumerate().map(|(index, server)| {
                    let url = server.url.clone();
                    Button::new(
                        ("dev-server", index),
                        format!("{} ({})", server.url, server.command),
                    )
                    .label_size(LabelSize::Small)
                    .on_click(cx.listener(move |this, _, _, cx| this.open_url(url.clone(), cx)))
                }))
                .into_any_element(),
        }
    }
}

fn key_modifiers(modifiers: Modifiers) -> KeyModifiers {
    KeyModifiers {
        alt: modifiers.alt,
        control: modifiers.control,
        meta: modifiers.platform,
        shift: modifiers.shift,
    }
}

/// The DOM key name for a zedd key name, for keys that do not type text.
fn dom_key(key: &str) -> Option<String> {
    Some(
        match key {
            "enter" => "Enter",
            "tab" => "Tab",
            "backspace" => "Backspace",
            "delete" => "Delete",
            "escape" => "Escape",
            "space" => " ",
            "left" => "ArrowLeft",
            "right" => "ArrowRight",
            "up" => "ArrowUp",
            "down" => "ArrowDown",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            key if key.chars().count() == 1 => key,
            _ => return None,
        }
        .to_string(),
    )
}

impl Render for BrowserPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("BrowserPanel")
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(self.render_tabs(cx))
            .child(self.render_toolbar(cx))
            .when_some(self.error.clone(), |panel, error| {
                panel.child(
                    h_flex()
                        .w_full()
                        .px_2()
                        .py_1()
                        .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
                )
            })
            .children(self.render_dialog(cx))
            .child(self.render_page(window, cx))
    }
}

impl EventEmitter<PanelEvent> for BrowserPanel {}

impl Focusable for BrowserPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.browser.read(cx).active_tab().is_some() {
            self.page_focus_handle.clone()
        } else {
            self.address_bar.focus_handle(cx)
        }
    }
}

impl Panel for BrowserPanel {
    fn persistent_name() -> &'static str {
        "BrowserPanel"
    }

    fn panel_key() -> &'static str {
        BROWSER_PANEL_KEY
    }

    fn position(&self, _: &Window, cx: &App) -> DockPosition {
        match BrowserSettings::get_global(cx).dock {
            DockSide::Left => DockPosition::Left,
            DockSide::Right => DockPosition::Right,
        }
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        settings::update_settings_file(self.fs.clone(), cx, move |settings, _| {
            settings.browser.get_or_insert_default().dock = Some(match position {
                DockPosition::Left => DockSide::Left,
                DockPosition::Bottom | DockPosition::Right => DockSide::Right,
            });
        });
    }

    fn default_size(&self, _: &Window, cx: &App) -> Pixels {
        BrowserSettings::get_global(cx).default_width
    }

    fn icon(&self, _: &Window, cx: &App) -> Option<IconName> {
        BrowserSettings::get_global(cx)
            .button
            .then_some(IconName::ToolWeb)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Browser Panel")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        8
    }

    fn hide_button_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        Some(workspace::HideStatusItem::new(|settings| {
            settings.browser.get_or_insert_default().button = Some(false);
        }))
    }

    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.browser
            .update(cx, |browser, cx| browser.set_visible(active, cx));
    }

    fn is_zoomed(&self, _: &Window, _: &App) -> bool {
        self.is_zoomed
    }

    fn set_zoomed(&mut self, zoomed: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.is_zoomed = zoomed;
        cx.notify();
    }
}

#[cfg(test)]
mod browser_panel_tests;
