//! A workspace's browser: its tabs in the shared headless Chrome, what each tab shows, and the
//! input the user and agents send to them.

mod cdp;
mod chrome;
mod tools;

#[cfg(any(test, feature = "test-support"))]
pub use cdp::test_support;
pub use cdp::{CdpConnection, CdpEvent};
pub use chrome::{Chrome, connect_to_shared_chrome, find_chrome, shared_chrome};
pub use tools::add_browser_tools;

use anyhow::{Context as _, Result, anyhow};
use base64::Engine as _;
use collections::{HashMap, VecDeque};
use futures::channel::oneshot;
use gpui::{
    App, AppContext as _, Context, EventEmitter, RenderImage, SharedString, Task, TaskExt as _,
};
use serde_json::{Value, json};
use std::{rc::Rc, sync::Arc, time::Duration};

/// Returns the element's opening tag, its text, and a short CSS selector path.
const DESCRIBE_ELEMENT_JS: &str = r#"function() {
    const tag = this.tagName.toLowerCase();
    const opening = this.cloneNode(false).outerHTML.replace(new RegExp('</' + tag + '>$'), '');
    const text = (this.innerText || this.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 200);
    const path = [];
    for (let node = this; node && node.nodeType === 1 && path.length < 5; node = node.parentElement) {
        if (node.id) { path.unshift(node.tagName.toLowerCase() + '#' + CSS.escape(node.id)); break; }
        const classes = [...node.classList].slice(0, 2).map(name => '.' + CSS.escape(name)).join('');
        path.unshift(node.tagName.toLowerCase() + classes);
    }
    return { url: location.href, markup: (opening + text + '</' + tag + '>').slice(0, 600), selector: path.join(' > ') };
}"#;

/// How many console messages each tab keeps.
const CONSOLE_HISTORY: usize = 500;

pub type Connector = Rc<dyn Fn(&mut App) -> Task<Result<Arc<CdpConnection>>>>;

pub struct Browser {
    connector: Connector,
    connection: Option<Arc<CdpConnection>>,
    tabs: Vec<BrowserTab>,
    active_tab: Option<usize>,
    viewport: Option<Viewport>,
    visible: bool,
    picking_element: bool,
    _events: Option<Task<()>>,
}

pub struct BrowserTab {
    pub target_id: String,
    session_id: String,
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// The latest frame of the page, when the tab is being shown.
    pub frame: Option<Arc<RenderImage>>,
    pub dialog: Option<JavaScriptDialog>,
    console: VecDeque<ConsoleMessage>,
    load_waiters: Vec<oneshot::Sender<()>>,
    /// The elements an agent's latest snapshot named, by ref; a new page clears them.
    element_refs: HashMap<String, i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JavaScriptDialog {
    pub kind: SharedString,
    pub message: SharedString,
    pub default_prompt: SharedString,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleMessage {
    pub level: SharedString,
    pub text: String,
}

/// The page area in CSS pixels and the display's pixels per CSS pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
    pub scale_factor: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseAction {
    Pressed,
    Released,
    Moved,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseButton {
    None,
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyModifiers {
    pub alt: bool,
    pub control: bool,
    pub meta: bool,
    pub shift: bool,
}

impl KeyModifiers {
    fn cdp_bits(self) -> u8 {
        u8::from(self.alt)
            | u8::from(self.control) << 1
            | u8::from(self.meta) << 2
            | u8::from(self.shift) << 3
    }
}

pub enum BrowserEvent {
    TabsChanged,
    /// An agent used the browser, so the user should see it.
    UsedByAgent,
    /// The user picked this DOM node while picking an element.
    ElementPicked {
        backend_node_id: i64,
    },
}

impl EventEmitter<BrowserEvent> for Browser {}

impl Browser {
    /// A browser with no tabs; `connector` starts or reaches Chrome on first use.
    pub fn new(connector: Connector) -> Self {
        Self {
            connector,
            connection: None,
            tabs: Vec::new(),
            active_tab: None,
            viewport: None,
            visible: false,
            picking_element: false,
            _events: None,
        }
    }

    pub fn tabs(&self) -> &[BrowserTab] {
        &self.tabs
    }

    pub fn active_tab_index(&self) -> Option<usize> {
        self.active_tab
    }

    pub fn active_tab(&self) -> Option<&BrowserTab> {
        self.tabs.get(self.active_tab?)
    }

    fn connect(&mut self, cx: &mut Context<Self>) -> Task<Result<Arc<CdpConnection>>> {
        if let Some(connection) = &self.connection {
            if !connection.is_closed() {
                return Task::ready(Ok(connection.clone()));
            }
            // Chrome exited, taking every tab with it.
            self.connection = None;
            self._events = None;
            self.tabs.clear();
            self.active_tab = None;
            cx.emit(BrowserEvent::TabsChanged);
            cx.notify();
        }
        let connect = (self.connector)(cx);
        cx.spawn(async move |this, cx| {
            let connection = connect.await?;
            let first_connection = this.update(cx, |this, cx| {
                if this.connection.is_some() {
                    return false;
                }
                let events = connection.subscribe();
                this.connection = Some(connection.clone());
                this._events = Some(cx.spawn(async move |this, cx| {
                    while let Ok(event) = events.recv().await {
                        if this
                            .update(cx, |this, cx| this.handle_event(event, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                }));
                true
            })?;
            if first_connection {
                connection
                    .send(
                        None,
                        "Target.setDiscoverTargets",
                        json!({ "discover": true }),
                    )
                    .await?;
            }
            Ok(connection)
        })
    }

    /// Opens a tab, shows it, and loads `url` in it when given.
    pub fn new_tab(&mut self, url: Option<String>, cx: &mut Context<Self>) -> Task<Result<()>> {
        let connect = self.connect(cx);
        cx.spawn(async move |this, cx| {
            let connection = connect.await?;
            let target = connection
                .send(None, "Target.createTarget", json!({ "url": "about:blank" }))
                .await?;
            let target_id = target["targetId"]
                .as_str()
                .context("Target.createTarget: no targetId")?
                .to_string();
            Self::adopt_target(this.clone(), connection, target_id, cx).await?;
            if let Some(url) = url {
                this.update(cx, |this, cx| this.navigate(&url, cx))?.await?;
            }
            Ok(())
        })
    }

    /// Takes over a page target: attaches to it, turns on the events zedd reads, and makes it
    /// the active tab.
    async fn adopt_target(
        this: gpui::WeakEntity<Self>,
        connection: Arc<CdpConnection>,
        target_id: String,
        cx: &mut gpui::AsyncApp,
    ) -> Result<()> {
        let attached = connection
            .send(
                None,
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
            )
            .await?;
        let session_id = attached["sessionId"]
            .as_str()
            .context("Target.attachToTarget: no sessionId")?
            .to_string();
        for method in ["Page.enable", "Runtime.enable"] {
            connection
                .send(Some(&session_id), method, json!({}))
                .await?;
        }
        // Size the page before it is shown, so its first frames already fit.
        if let Some(viewport) = this.read_with(cx, |this, _| this.viewport)? {
            apply_viewport(&connection, &session_id, viewport).await?;
        }
        this.update(cx, |this, cx| {
            this.tabs.push(BrowserTab {
                target_id,
                session_id: session_id.clone(),
                url: String::new(),
                title: String::new(),
                loading: false,
                frame: None,
                dialog: None,
                console: VecDeque::new(),
                load_waiters: Vec::new(),
                element_refs: HashMap::default(),
            });
            this.activate_tab(this.tabs.len() - 1, cx);
        })
    }

    pub fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() || self.active_tab == Some(index) {
            return;
        }
        self.stop_screencast(cx);
        self.active_tab = Some(index);
        self.start_screencast(cx);
        cx.emit(BrowserEvent::TabsChanged);
        cx.notify();
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) -> Task<Result<()>> {
        let (Some(connection), Some(tab)) = (self.connection.clone(), self.tabs.get(index)) else {
            return Task::ready(Ok(()));
        };
        let target_id = tab.target_id.clone();
        self.remove_tab(&target_id, cx);
        cx.background_spawn(async move {
            connection
                .send(None, "Target.closeTarget", json!({ "targetId": target_id }))
                .await?;
            Ok(())
        })
    }

    fn remove_tab(&mut self, target_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.target_id == target_id) else {
            return;
        };
        self.tabs.remove(index);
        match self.active_tab {
            _ if self.tabs.is_empty() => self.active_tab = None,
            Some(active) if active > index => self.active_tab = Some(active - 1),
            Some(active) if active == index => {
                self.active_tab = Some(index.min(self.tabs.len() - 1));
                self.start_screencast(cx);
            }
            _ => {}
        }
        cx.emit(BrowserEvent::TabsChanged);
        cx.notify();
    }

    /// Sends a command to the active tab, opening a blank tab first when there is none.
    pub fn send_to_active_tab(
        &mut self,
        method: &'static str,
        params: Value,
        cx: &mut Context<Self>,
    ) -> Task<Result<Value>> {
        let ensure_tab = if self.active_tab.is_none() {
            self.new_tab(None, cx)
        } else {
            Task::ready(Ok(()))
        };
        cx.spawn(async move |this, cx| {
            ensure_tab.await?;
            let (connection, session_id) = this.read_with(cx, |this, _| {
                let tab = this.active_tab().context("the browser has no open tab")?;
                let connection = this
                    .connection
                    .clone()
                    .context("the browser is not running")?;
                anyhow::Ok((connection, tab.session_id.clone()))
            })??;
            connection.send(Some(&session_id), method, params).await
        })
    }

    pub fn navigate(&mut self, url: &str, cx: &mut Context<Self>) -> Task<Result<()>> {
        let url = normalize_url(url);
        let navigate = self.send_to_active_tab("Page.navigate", json!({ "url": url }), cx);
        cx.background_spawn(async move {
            let result = navigate.await?;
            if let Some(error) = result.get("errorText").and_then(Value::as_str) {
                anyhow::bail!("could not open {url}: {error}");
            }
            Ok(())
        })
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.go_through_history(-1, cx)
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.go_through_history(1, cx)
    }

    fn go_through_history(&mut self, step: i64, cx: &mut Context<Self>) -> Task<Result<()>> {
        let history = self.send_to_active_tab("Page.getNavigationHistory", json!({}), cx);
        cx.spawn(async move |this, cx| {
            let history = history.await?;
            let index = history["currentIndex"].as_i64().unwrap_or(0) + step;
            let Some(entry) = usize::try_from(index)
                .ok()
                .and_then(|index| history["entries"].get(index))
            else {
                anyhow::bail!(
                    "there is no page to go {}",
                    if step < 0 { "back to" } else { "forward to" }
                );
            };
            let entry_id = entry["id"].clone();
            this.update(cx, |this, cx| {
                this.send_to_active_tab(
                    "Page.navigateToHistoryEntry",
                    json!({ "entryId": entry_id }),
                    cx,
                )
            })?
            .await?;
            Ok(())
        })
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) -> Task<Result<()>> {
        let reload = self.send_to_active_tab("Page.reload", json!({}), cx);
        cx.background_spawn(async move { reload.await.map(|_| ()) })
    }

    /// Resolves when the active tab finishes loading, or after `timeout`.
    pub fn wait_for_load(&mut self, timeout: Duration, cx: &mut Context<Self>) -> Task<()> {
        let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) else {
            return Task::ready(());
        };
        if !tab.loading {
            return Task::ready(());
        }
        let (loaded_tx, loaded_rx) = oneshot::channel();
        tab.load_waiters.push(loaded_tx);
        let timer = cx.background_executor().timer(timeout);
        cx.background_spawn(async move {
            futures::future::select(loaded_rx, timer).await;
        })
    }

    /// Sets the page area's size; every tab lays out to it.
    pub fn set_viewport(&mut self, viewport: Viewport, cx: &mut Context<Self>) {
        if self.viewport == Some(viewport) {
            return;
        }
        self.viewport = Some(viewport);
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let sessions: Vec<String> = self.tabs.iter().map(|tab| tab.session_id.clone()).collect();
        cx.spawn(async move |this, cx| {
            for session_id in sessions {
                apply_viewport(&connection, &session_id, viewport)
                    .await
                    .context("resizing the page")?;
            }
            this.update(cx, |this, cx| this.start_screencast(cx))
        })
        .detach_and_log_err(cx);
    }

    /// Frames are streamed only while the browser is on screen.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        if visible {
            self.visible = true;
            self.start_screencast(cx);
        } else {
            self.stop_screencast(cx);
            self.visible = false;
        }
    }

    fn start_screencast(&mut self, cx: &mut Context<Self>) {
        let (true, Some(viewport)) = (self.visible, self.viewport) else {
            return;
        };
        self.send_to_shown_tab(
            "Page.startScreencast",
            json!({
                "format": "jpeg",
                "quality": 80,
                "maxWidth": (viewport.width * viewport.scale_factor).round() as i64,
                "maxHeight": (viewport.height * viewport.scale_factor).round() as i64,
            }),
            cx,
        );
    }

    fn stop_screencast(&mut self, cx: &mut Context<Self>) {
        if self.visible {
            self.send_to_shown_tab("Page.stopScreencast", json!({}), cx);
        }
    }

    /// Sends a command to the active tab only if it already exists; failures are logged.
    fn send_to_shown_tab(&self, method: &'static str, params: Value, cx: &mut Context<Self>) {
        let (Some(connection), Some(tab)) = (self.connection.clone(), self.active_tab()) else {
            return;
        };
        let session_id = tab.session_id.clone();
        cx.background_spawn(
            async move { connection.send(Some(&session_id), method, params).await },
        )
        .detach_and_log_err(cx);
    }

    pub fn dispatch_mouse(
        &mut self,
        action: MouseAction,
        x: f32,
        y: f32,
        button: MouseButton,
        click_count: usize,
        modifiers: KeyModifiers,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let kind = match action {
            MouseAction::Pressed => "mousePressed",
            MouseAction::Released => "mouseReleased",
            MouseAction::Moved => "mouseMoved",
        };
        let button = match button {
            MouseButton::None => "none",
            MouseButton::Left => "left",
            MouseButton::Middle => "middle",
            MouseButton::Right => "right",
        };
        self.send_input(
            "Input.dispatchMouseEvent",
            json!({
                "type": kind, "x": x, "y": y, "button": button,
                "clickCount": click_count, "modifiers": modifiers.cdp_bits(),
            }),
            cx,
        )
    }

    pub fn dispatch_wheel(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.send_input(
            "Input.dispatchMouseEvent",
            json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": delta_x, "deltaY": delta_y }),
            cx,
        )
    }

    /// Types `text` into the focused element, as an input method would.
    pub fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.send_input("Input.insertText", json!({ "text": text }), cx)
    }

    /// Presses and releases `key`, a DOM key name such as `Enter`, `ArrowDown` or `a`.
    pub fn press_key(
        &mut self,
        key: &str,
        modifiers: KeyModifiers,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(definition) = key_definition(key) else {
            return Task::ready(Err(anyhow!("unknown key `{key}`")));
        };
        let mut key_down = json!({
            "type": if definition.text.is_some() && !modifiers.meta && !modifiers.control { "keyDown" } else { "rawKeyDown" },
            "key": definition.key,
            "code": definition.code,
            "windowsVirtualKeyCode": definition.key_code,
            "modifiers": modifiers.cdp_bits(),
        });
        if let Some(text) = definition.text.as_deref()
            && !modifiers.meta
            && !modifiers.control
        {
            key_down["text"] = text.into();
        }
        // Headless Chrome has no menu bar to turn macOS editing shortcuts into commands.
        if let Some(command) = editing_command(&definition.key, modifiers) {
            key_down["commands"] = json!([command]);
        }
        let mut key_up = key_down.clone();
        key_up["type"] = "keyUp".into();
        key_up
            .as_object_mut()
            .map(|key_up| key_up.remove("commands"));
        cx.spawn(async move |this, cx| {
            this.update(cx, |this, cx| {
                this.send_input("Input.dispatchKeyEvent", key_down, cx)
            })?
            .await?;
            this.update(cx, |this, cx| {
                this.send_input("Input.dispatchKeyEvent", key_up, cx)
            })?
            .await
        })
    }

    fn send_input(
        &mut self,
        method: &'static str,
        params: Value,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let send = self.send_to_active_tab(method, params, cx);
        cx.background_spawn(async move { send.await.map(|_| ()) })
    }

    /// Answers the active tab's open `alert`, `confirm` or `prompt`.
    pub fn handle_dialog(
        &mut self,
        accept: bool,
        prompt_text: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        if self.active_tab().is_none_or(|tab| tab.dialog.is_none()) {
            return Task::ready(Err(anyhow!("the page has no open dialog")));
        }
        let mut params = json!({ "accept": accept });
        if let Some(prompt_text) = prompt_text {
            params["promptText"] = prompt_text.into();
        }
        self.send_input("Page.handleJavaScriptDialog", params, cx)
    }

    pub fn is_picking_element(&self) -> bool {
        self.picking_element
    }

    /// While picking, Chrome highlights the element under the mouse, and a click picks it
    /// instead of clicking it.
    pub fn set_picking_element(
        &mut self,
        picking: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        if self.picking_element == picking {
            return Task::ready(Ok(()));
        }
        let (Some(connection), Some(tab)) = (self.connection.clone(), self.active_tab()) else {
            return Task::ready(Err(anyhow!("the browser has no open tab")));
        };
        let session_id = tab.session_id.clone();
        self.picking_element = picking;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = async {
                if picking {
                    // Chrome's highlighter belongs to its DOM inspector, which must be on first.
                    connection
                        .send(Some(&session_id), "DOM.enable", json!({}))
                        .await?;
                    connection
                        .send(Some(&session_id), "Overlay.enable", json!({}))
                        .await?;
                    connection
                        .send(
                            Some(&session_id),
                            "Overlay.setInspectMode",
                            json!({
                                "mode": "searchForNode",
                                "highlightConfig": {
                                    "showInfo": true,
                                    "contentColor": { "r": 111, "g": 168, "b": 220, "a": 0.5 },
                                    "borderColor": { "r": 111, "g": 168, "b": 220, "a": 1.0 },
                                },
                            }),
                        )
                        .await?;
                } else {
                    connection
                        .send(
                            Some(&session_id),
                            "Overlay.setInspectMode",
                            json!({ "mode": "none", "highlightConfig": {} }),
                        )
                        .await?;
                }
                anyhow::Ok(())
            }
            .await;
            if result.is_err() {
                this.update(cx, |this, cx| {
                    this.picking_element = false;
                    cx.notify();
                })?;
            }
            result
        })
    }

    /// A description of a DOM node for an agent: the page, the element's markup, and a selector.
    pub fn describe_element(
        &mut self,
        backend_node_id: i64,
        cx: &mut Context<Self>,
    ) -> Task<Result<String>> {
        let node = self.send_to_active_tab(
            "DOM.resolveNode",
            json!({ "backendNodeId": backend_node_id }),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let node = node.await?;
            let described = this
                .update(cx, |this, cx| {
                    this.send_to_active_tab(
                        "Runtime.callFunctionOn",
                        json!({
                            "objectId": node["object"]["objectId"],
                            "functionDeclaration": DESCRIBE_ELEMENT_JS,
                            "returnByValue": true,
                        }),
                        cx,
                    )
                })?
                .await?;
            let element = &described["result"]["value"];
            Ok(format!(
                "Element picked in the browser at {}:\n```html\n{}\n```\nSelector: `{}`\n",
                element["url"].as_str().unwrap_or_default(),
                element["markup"].as_str().unwrap_or_default(),
                element["selector"].as_str().unwrap_or_default(),
            ))
        })
    }

    pub fn note_agent_use(&mut self, cx: &mut Context<Self>) {
        cx.emit(BrowserEvent::UsedByAgent);
    }

    /// The DOM node an agent's snapshot named `element_ref` on the active tab's current page.
    pub fn element_for_ref(&self, element_ref: &str) -> Option<i64> {
        self.active_tab()?.element_refs.get(element_ref).copied()
    }

    pub fn set_element_refs(&mut self, element_refs: HashMap<String, i64>) {
        if let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) {
            tab.element_refs = element_refs;
        }
    }

    /// The active tab's console messages, oldest first.
    pub fn console_messages(&self) -> Vec<ConsoleMessage> {
        self.active_tab()
            .map(|tab| tab.console.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear_console(&mut self) {
        if let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) {
            tab.console.clear();
        }
    }

    fn handle_event(&mut self, event: CdpEvent, cx: &mut Context<Self>) {
        match event.session_id.as_deref() {
            None => self.handle_browser_event(&event.method, &event.params, cx),
            Some(session_id) => {
                if let Some(index) = self
                    .tabs
                    .iter()
                    .position(|tab| tab.session_id == session_id)
                {
                    self.handle_tab_event(index, &event.method, event.params, cx);
                }
            }
        }
    }

    fn handle_browser_event(&mut self, method: &str, params: &Value, cx: &mut Context<Self>) {
        let info = &params["targetInfo"];
        match method {
            "Target.targetInfoChanged" => {
                let target_id = info["targetId"].as_str().unwrap_or_default();
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.target_id == target_id) {
                    tab.url = info["url"].as_str().unwrap_or_default().to_string();
                    tab.title = info["title"].as_str().unwrap_or_default().to_string();
                    cx.emit(BrowserEvent::TabsChanged);
                    cx.notify();
                }
            }
            // A page opened by one of this browser's tabs, like a sign-in popup, becomes a tab.
            "Target.targetCreated" if info["type"] == "page" => {
                let opener = info["openerId"].as_str().unwrap_or_default();
                if let (true, Some(connection), Some(target_id)) = (
                    self.tabs.iter().any(|tab| tab.target_id == opener),
                    self.connection.clone(),
                    info["targetId"].as_str().map(str::to_string),
                ) {
                    cx.spawn(async move |this, cx| {
                        Self::adopt_target(this, connection, target_id, cx).await
                    })
                    .detach_and_log_err(cx);
                }
            }
            "Target.targetDestroyed" => {
                if let Some(target_id) = params["targetId"].as_str() {
                    self.remove_tab(target_id, cx);
                }
            }
            _ => {}
        }
    }

    fn handle_tab_event(
        &mut self,
        index: usize,
        method: &str,
        params: Value,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[index];
        let is_main_frame = params["frameId"].as_str() == Some(&tab.target_id);
        match method {
            "Page.frameStartedLoading" if is_main_frame => tab.loading = true,
            "Page.frameNavigated"
                if params["frame"]["id"].as_str() == Some(&tab.target_id)
                    && params["frame"].get("parentId").is_none() =>
            {
                tab.element_refs.clear();
            }
            "Page.frameStoppedLoading" if is_main_frame => {
                tab.loading = false;
                for waiter in tab.load_waiters.drain(..) {
                    waiter.send(()).ok();
                }
            }
            "Page.javascriptDialogOpening" => {
                tab.dialog = Some(JavaScriptDialog {
                    kind: params["type"]
                        .as_str()
                        .unwrap_or("alert")
                        .to_string()
                        .into(),
                    message: params["message"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string()
                        .into(),
                    default_prompt: params["defaultPrompt"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string()
                        .into(),
                });
            }
            "Page.javascriptDialogClosed" => tab.dialog = None,
            "Runtime.consoleAPICalled" => {
                let text = params["args"]
                    .as_array()
                    .map(|args| {
                        args.iter()
                            .map(remote_object_text)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                let level = params["type"].as_str().unwrap_or("log").to_string();
                tab.push_console(level, text);
            }
            "Runtime.exceptionThrown" => {
                let details = &params["exceptionDetails"];
                let text = details["exception"]["description"]
                    .as_str()
                    .or(details["text"].as_str())
                    .unwrap_or("Uncaught exception")
                    .to_string();
                tab.push_console("error".to_string(), text);
            }
            "Page.screencastFrame" => {
                self.receive_frame(index, params, cx);
                return;
            }
            "Overlay.inspectNodeRequested" => {
                if let Some(backend_node_id) = params["backendNodeId"].as_i64() {
                    self.set_picking_element(false, cx).detach_and_log_err(cx);
                    cx.emit(BrowserEvent::ElementPicked { backend_node_id });
                }
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn receive_frame(&mut self, index: usize, params: Value, cx: &mut Context<Self>) {
        let target_id = self.tabs[index].target_id.clone();
        let screencast_session = params["sessionId"].clone();
        let data = params["data"].as_str().unwrap_or_default().to_string();
        let decode = cx.background_spawn(async move { decode_frame(&data) });
        cx.spawn(async move |this, cx| {
            let frame = decode.await;
            this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.target_id == target_id) {
                    match frame {
                        Ok(frame) => tab.frame = Some(frame),
                        Err(error) => log::error!("unreadable browser frame: {error:#}"),
                    }
                    cx.notify();
                }
                // Acknowledging after decoding keeps Chrome from sending frames faster than
                // zedd can show them.
                this.send_to_shown_tab(
                    "Page.screencastFrameAck",
                    json!({ "sessionId": screencast_session }),
                    cx,
                );
            })
        })
        .detach_and_log_err(cx);
    }
}

impl BrowserTab {
    fn push_console(&mut self, level: String, text: String) {
        if self.console.len() == CONSOLE_HISTORY {
            self.console.pop_front();
        }
        self.console.push_back(ConsoleMessage {
            level: level.into(),
            text,
        });
    }
}

async fn apply_viewport(
    connection: &CdpConnection,
    session_id: &str,
    viewport: Viewport,
) -> Result<()> {
    connection
        .send(
            Some(session_id),
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": viewport.width.round() as i64,
                "height": viewport.height.round() as i64,
                "deviceScaleFactor": viewport.scale_factor,
                "mobile": false,
            }),
        )
        .await?;
    Ok(())
}

fn decode_frame(base64_jpeg: &str) -> Result<Arc<RenderImage>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64_jpeg)?;
    let mut image =
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)?.into_rgba8();
    // GPUI draws BGRA.
    for pixel in image.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(smallvec::SmallVec::from_elem(
        image::Frame::new(image),
        1,
    ))))
}

/// How a console argument reads in DevTools: strings and numbers by value, objects by their
/// description.
fn remote_object_text(object: &Value) -> String {
    match &object["value"] {
        Value::String(text) => text.clone(),
        Value::Null => object["description"]
            .as_str()
            .or(object["type"].as_str())
            .unwrap_or_default()
            .to_string(),
        value => value.to_string(),
    }
}

/// Adds the scheme a browser's address bar would assume: `http` for this machine, `https`
/// otherwise.
pub fn normalize_url(input: &str) -> String {
    let input = input.trim();
    if input.contains("://")
        || ["about:", "data:", "file:", "chrome:", "javascript:"]
            .iter()
            .any(|scheme| input.starts_with(scheme))
    {
        return input.to_string();
    }
    let host = input.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once(':').map_or(host, |(host, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            host
        } else {
            input
        }
    });
    let is_local = host == "localhost"
        || host.ends_with(".localhost")
        || host == "[::1]"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified());
    format!("{}://{input}", if is_local { "http" } else { "https" })
}

struct KeyDefinition {
    key: String,
    code: String,
    key_code: u32,
    text: Option<String>,
}

/// What Chrome needs to synthesize `key`: its DOM key and code, its Windows key code (which
/// Chrome uses to decide default actions), and the text it types, if any.
fn key_definition(key: &str) -> Option<KeyDefinition> {
    let named = |key: &str, code: &str, key_code: u32, text: Option<&str>| KeyDefinition {
        key: key.to_string(),
        code: code.to_string(),
        key_code,
        text: text.map(str::to_string),
    };
    Some(match key {
        "Enter" => named("Enter", "Enter", 13, Some("\r")),
        "Tab" => named("Tab", "Tab", 9, None),
        "Backspace" => named("Backspace", "Backspace", 8, None),
        "Delete" => named("Delete", "Delete", 46, None),
        "Escape" => named("Escape", "Escape", 27, None),
        " " | "Space" => named(" ", "Space", 32, Some(" ")),
        "ArrowLeft" => named("ArrowLeft", "ArrowLeft", 37, None),
        "ArrowUp" => named("ArrowUp", "ArrowUp", 38, None),
        "ArrowRight" => named("ArrowRight", "ArrowRight", 39, None),
        "ArrowDown" => named("ArrowDown", "ArrowDown", 40, None),
        "Home" => named("Home", "Home", 36, None),
        "End" => named("End", "End", 35, None),
        "PageUp" => named("PageUp", "PageUp", 33, None),
        "PageDown" => named("PageDown", "PageDown", 34, None),
        _ => {
            let mut chars = key.chars();
            let character = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            let upper = character.to_ascii_uppercase();
            let code = if character.is_ascii_alphabetic() {
                format!("Key{upper}")
            } else if character.is_ascii_digit() {
                format!("Digit{character}")
            } else {
                String::new()
            };
            KeyDefinition {
                key: key.to_string(),
                code,
                key_code: if character.is_ascii_alphanumeric() {
                    upper as u32
                } else {
                    0
                },
                text: Some(key.to_string()),
            }
        }
    })
}

fn editing_command(key: &str, modifiers: KeyModifiers) -> Option<&'static str> {
    if !modifiers.meta || modifiers.control || modifiers.alt {
        return None;
    }
    Some(match (key.to_ascii_lowercase().as_str(), modifiers.shift) {
        ("a", false) => "selectAll",
        ("c", false) => "copy",
        ("x", false) => "cut",
        ("v", false) => "paste",
        ("z", false) => "undo",
        ("z", true) => "redo",
        _ => return None,
    })
}

#[cfg(test)]
mod browser_tests;
