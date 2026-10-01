//! Browser tools for agents, served over MCP. Every tool acts on the active tab of the browser
//! panel, so the user sees what the agent does.

use crate::{Browser, KeyModifiers, MouseAction, MouseButton};
use anyhow::{Context as _, Result, anyhow};
use collections::HashMap;
use context_server::listener::{McpServer, McpServerTool, ToolResponse};
use context_server::types::{ToolAnnotations, ToolResponseContent};
use gpui::{AsyncApp, WeakEntity};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write as _, time::Duration};

/// The longest snapshot an agent receives, in characters.
const MAX_SNAPSHOT_CHARS: usize = 40_000;
/// How long navigation waits for the page to finish loading.
const LOAD_TIMEOUT: Duration = Duration::from_secs(15);

/// Roles whose name already says what their children would.
const ROLES_NAMED_BY_THEIR_TEXT: &[&str] = &[
    "button",
    "link",
    "heading",
    "menuitem",
    "option",
    "tab",
    "checkbox",
    "radio",
    "switch",
    "treeitem",
    "cell",
    "columnheader",
    "rowheader",
    "listitem",
    "label",
    "LabelText",
];
/// Roles that only group other nodes.
const WRAPPER_ROLES: &[&str] = &["generic", "none", "presentation", "RootWebArea", "group"];

pub fn add_browser_tools(server: &mut McpServer, browser: WeakEntity<Browser>) {
    server.add_tool(NavigateTool(browser.clone()));
    server.add_tool(SnapshotTool(browser.clone()));
    server.add_tool(ClickTool(browser.clone()));
    server.add_tool(TypeTool(browser.clone()));
    server.add_tool(PressKeyTool(browser.clone()));
    server.add_tool(ScreenshotTool(browser.clone()));
    server.add_tool(ConsoleTool(browser.clone()));
    server.add_tool(EvaluateTool(browser.clone()));
    server.add_tool(TabsTool(browser.clone()));
    server.add_tool(HandleDialogTool(browser));
}

fn text(text: impl Into<String>) -> ToolResponse<()> {
    ToolResponse {
        content: vec![ToolResponseContent::Text { text: text.into() }],
        structured_content: (),
    }
}

fn read_only() -> ToolAnnotations {
    ToolAnnotations {
        title: None,
        read_only_hint: Some(true),
        destructive_hint: None,
        idempotent_hint: None,
        open_world_hint: None,
    }
}

/// Shows the browser to the user and refuses to act behind an open dialog, which blocks the page.
fn begin(browser: &WeakEntity<Browser>, cx: &mut AsyncApp) -> Result<()> {
    browser.update(cx, |browser, cx| {
        browser.note_agent_use(cx);
        if let Some(dialog) = browser.active_tab().and_then(|tab| tab.dialog.as_ref()) {
            anyhow::bail!(
                "The page is showing a JavaScript {} dialog: \"{}\". Answer it with browser_handle_dialog first.",
                dialog.kind,
                dialog.message
            );
        }
        Ok(())
    })?
}

async fn send(
    browser: &WeakEntity<Browser>,
    method: &'static str,
    params: Value,
    cx: &mut AsyncApp,
) -> Result<Value> {
    browser
        .update(cx, |browser, cx| {
            browser.send_to_active_tab(method, params, cx)
        })?
        .await
}

async fn evaluate(
    browser: &WeakEntity<Browser>,
    expression: &str,
    cx: &mut AsyncApp,
) -> Result<Value> {
    let result = send(
        browser,
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
        cx,
    )
    .await?;
    if let Some(details) = result.get("exceptionDetails") {
        let message = details["exception"]["description"]
            .as_str()
            .or(details["text"].as_str())
            .unwrap_or("the script threw an exception");
        return Err(anyhow!("{message}"));
    }
    Ok(result["result"]["value"].clone())
}

async fn page_summary(browser: &WeakEntity<Browser>, cx: &mut AsyncApp) -> Result<String> {
    let page = evaluate(
        browser,
        "({ url: location.href, title: document.title })",
        cx,
    )
    .await?;
    Ok(format!(
        "Page: {} ({})",
        page["title"].as_str().unwrap_or_default(),
        page["url"].as_str().unwrap_or_default()
    ))
}

async fn wait_until_loaded(browser: &WeakEntity<Browser>, cx: &mut AsyncApp) {
    let started = std::time::Instant::now();
    while started.elapsed() < LOAD_TIMEOUT {
        match evaluate(browser, "document.readyState", cx).await {
            Ok(state) if state == "complete" => return,
            _ => {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await
            }
        }
    }
}

fn element(browser: &WeakEntity<Browser>, element_ref: &str, cx: &mut AsyncApp) -> Result<i64> {
    browser
        .read_with(cx, |browser, _| browser.element_for_ref(element_ref))?
        .with_context(|| {
            format!(
                "No element {element_ref} on this page. Take a new browser_snapshot; refs reset \
                 when the page changes."
            )
        })
}

/// Opens a URL in the browser panel's active tab, or goes back, forward, or reloads. Opens the
/// panel and a tab when needed, and waits for the page to load.
#[derive(Deserialize, JsonSchema)]
struct NavigateInput {
    /// A URL such as `localhost:3000` or `https://example.com`, or `back`, `forward`, or
    /// `reload`.
    url: String,
}

#[derive(Clone)]
struct NavigateTool(WeakEntity<Browser>);

impl McpServerTool for NavigateTool {
    type Input = NavigateInput;
    type Output = ();
    const NAME: &'static str = "browser_navigate";

    async fn run(&self, input: NavigateInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        self.0
            .update(cx, |browser, cx| match input.url.as_str() {
                "back" => browser.go_back(cx),
                "forward" => browser.go_forward(cx),
                "reload" => browser.reload(cx),
                url if browser.active_tab().is_none() => browser.new_tab(Some(url.to_string()), cx),
                url => browser.navigate(url, cx),
            })?
            .await?;
        wait_until_loaded(&self.0, cx).await;
        Ok(text(format!(
            "{}\nTake browser_snapshot to read the page.",
            page_summary(&self.0, cx).await?
        )))
    }
}

/// Reads the active tab as an accessibility tree: each element's role, name, and state, with a
/// ref such as `e12` that browser_click and browser_type accept. Take a new snapshot after the
/// page changes.
#[derive(Deserialize, JsonSchema)]
struct SnapshotInput {}

#[derive(Clone)]
struct SnapshotTool(WeakEntity<Browser>);

impl McpServerTool for SnapshotTool {
    type Input = SnapshotInput;
    type Output = ();
    const NAME: &'static str = "browser_snapshot";

    fn annotations(&self) -> ToolAnnotations {
        read_only()
    }

    async fn run(&self, _: SnapshotInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        let tree = send(&self.0, "Accessibility.getFullAXTree", json!({}), cx).await?;
        let nodes = tree["nodes"].as_array().cloned().unwrap_or_default();
        let (snapshot, element_refs) = render_snapshot(&nodes);
        self.0
            .update(cx, |browser, _| browser.set_element_refs(element_refs))?;
        Ok(text(format!(
            "{}\n{snapshot}",
            page_summary(&self.0, cx).await?
        )))
    }
}

/// Clicks an element from the latest browser_snapshot, as a user would with the mouse.
#[derive(Deserialize, JsonSchema)]
struct ClickInput {
    /// The element's ref from browser_snapshot, such as `e12`.
    element_ref: String,
    /// Whether to double-click.
    #[serde(default)]
    double_click: bool,
}

#[derive(Clone)]
struct ClickTool(WeakEntity<Browser>);

impl McpServerTool for ClickTool {
    type Input = ClickInput;
    type Output = ();
    const NAME: &'static str = "browser_click";

    async fn run(&self, input: ClickInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        let backend_node_id = element(&self.0, &input.element_ref, cx)?;
        send(
            &self.0,
            "DOM.scrollIntoViewIfNeeded",
            json!({ "backendNodeId": backend_node_id }),
            cx,
        )
        .await?;
        let quads = send(
            &self.0,
            "DOM.getContentQuads",
            json!({ "backendNodeId": backend_node_id }),
            cx,
        )
        .await?;
        let (x, y) = quads["quads"]
            .get(0)
            .and_then(quad_center)
            .with_context(|| {
                format!(
                    "Element {} has no visible area to click.",
                    input.element_ref
                )
            })?;

        let clicks = if input.double_click { 2 } else { 1 };
        let mut steps = vec![(MouseAction::Moved, MouseButton::None, 0)];
        for click_count in 1..=clicks {
            steps.push((MouseAction::Pressed, MouseButton::Left, click_count));
            steps.push((MouseAction::Released, MouseButton::Left, click_count));
        }
        for (action, button, click_count) in steps {
            self.0
                .update(cx, |browser, cx| {
                    browser.dispatch_mouse(
                        action,
                        x,
                        y,
                        button,
                        click_count,
                        KeyModifiers::default(),
                        cx,
                    )
                })?
                .await?;
        }
        Ok(text(format!("Clicked {}.", input.element_ref)))
    }
}

/// Types into a text field from the latest browser_snapshot, replacing its contents.
#[derive(Deserialize, JsonSchema)]
struct TypeInput {
    /// The field's ref from browser_snapshot, such as `e7`.
    element_ref: String,
    /// The text to type.
    text: String,
    /// Whether to press Enter afterwards, for example to submit a form.
    #[serde(default)]
    submit: bool,
}

#[derive(Clone)]
struct TypeTool(WeakEntity<Browser>);

impl McpServerTool for TypeTool {
    type Input = TypeInput;
    type Output = ();
    const NAME: &'static str = "browser_type";

    async fn run(&self, input: TypeInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        let backend_node_id = element(&self.0, &input.element_ref, cx)?;
        send(
            &self.0,
            "DOM.focus",
            json!({ "backendNodeId": backend_node_id }),
            cx,
        )
        .await?;
        let node = send(
            &self.0,
            "DOM.resolveNode",
            json!({ "backendNodeId": backend_node_id }),
            cx,
        )
        .await?;
        // Selecting the current contents makes the typed text replace them.
        send(
            &self.0,
            "Runtime.callFunctionOn",
            json!({
                "objectId": node["object"]["objectId"],
                "functionDeclaration": "function() { if (this.select) { this.select(); } else if (this.isContentEditable) { document.execCommand('selectAll'); } }",
            }),
            cx,
        )
        .await?;
        let text_to_type = input.text.clone();
        self.0
            .update(cx, |browser, cx| browser.insert_text(&text_to_type, cx))?
            .await?;
        if input.submit {
            self.0
                .update(cx, |browser, cx| {
                    browser.press_key("Enter", KeyModifiers::default(), cx)
                })?
                .await?;
        }
        Ok(text(format!(
            "Typed into {}{}.",
            input.element_ref,
            if input.submit {
                " and pressed Enter"
            } else {
                ""
            }
        )))
    }
}

/// Presses a key on the page, such as `Enter`, `Tab`, `Escape`, `ArrowDown`, `PageDown`, or a
/// character, with optional modifiers joined by `+`, such as `Shift+Tab` or `Meta+a`.
#[derive(Deserialize, JsonSchema)]
struct PressKeyInput {
    key: String,
}

#[derive(Clone)]
struct PressKeyTool(WeakEntity<Browser>);

impl McpServerTool for PressKeyTool {
    type Input = PressKeyInput;
    type Output = ();
    const NAME: &'static str = "browser_press_key";

    async fn run(&self, input: PressKeyInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        let (key, modifiers) = parse_key_combination(&input.key)?;
        self.0
            .update(cx, |browser, cx| browser.press_key(&key, modifiers, cx))?
            .await?;
        Ok(text(format!("Pressed {}.", input.key)))
    }
}

/// Takes a screenshot of what the active tab shows.
#[derive(Deserialize, JsonSchema)]
struct ScreenshotInput {}

#[derive(Clone)]
struct ScreenshotTool(WeakEntity<Browser>);

impl McpServerTool for ScreenshotTool {
    type Input = ScreenshotInput;
    type Output = ();
    const NAME: &'static str = "browser_screenshot";

    fn annotations(&self) -> ToolAnnotations {
        read_only()
    }

    async fn run(&self, _: ScreenshotInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        let metrics = send(&self.0, "Page.getLayoutMetrics", json!({}), cx).await?;
        let viewport = &metrics["cssVisualViewport"];
        let pixel_ratio = evaluate(&self.0, "devicePixelRatio", cx)
            .await?
            .as_f64()
            .unwrap_or(1.0);
        // One image pixel per CSS pixel keeps screenshots small on high-density displays.
        let screenshot = send(
            &self.0,
            "Page.captureScreenshot",
            json!({
                "format": "png",
                "clip": {
                    "x": viewport["pageX"], "y": viewport["pageY"],
                    "width": viewport["clientWidth"], "height": viewport["clientHeight"],
                    "scale": 1.0 / pixel_ratio,
                },
            }),
            cx,
        )
        .await?;
        let data = screenshot["data"]
            .as_str()
            .context("Page.captureScreenshot: no image")?
            .to_string();
        Ok(ToolResponse {
            content: vec![ToolResponseContent::Image {
                data,
                mime_type: "image/png".to_string(),
            }],
            structured_content: (),
        })
    }
}

/// Reads the active tab's console messages and uncaught errors, oldest first.
#[derive(Deserialize, JsonSchema)]
struct ConsoleInput {
    /// Whether to clear the messages after reading them.
    #[serde(default)]
    clear: bool,
}

#[derive(Clone)]
struct ConsoleTool(WeakEntity<Browser>);

impl McpServerTool for ConsoleTool {
    type Input = ConsoleInput;
    type Output = ();
    const NAME: &'static str = "browser_console";

    fn annotations(&self) -> ToolAnnotations {
        read_only()
    }

    async fn run(&self, input: ConsoleInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        let messages = self.0.update(cx, |browser, _| {
            let messages = browser.console_messages();
            if input.clear {
                browser.clear_console();
            }
            messages
        })?;
        if messages.is_empty() {
            return Ok(text("The console is empty."));
        }
        let mut output = String::new();
        for message in messages {
            writeln!(output, "[{}] {}", message.level, message.text).ok();
        }
        Ok(text(output))
    }
}

/// Runs a JavaScript expression in the active tab and returns its value as JSON. Promises are
/// awaited.
#[derive(Deserialize, JsonSchema)]
struct EvaluateInput {
    expression: String,
}

#[derive(Clone)]
struct EvaluateTool(WeakEntity<Browser>);

impl McpServerTool for EvaluateTool {
    type Input = EvaluateInput;
    type Output = ();
    const NAME: &'static str = "browser_evaluate";

    async fn run(&self, input: EvaluateInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        begin(&self.0, cx)?;
        let value = evaluate(&self.0, &input.expression, cx).await?;
        Ok(text(if value.is_null() {
            "undefined".to_string()
        } else {
            serde_json::to_string_pretty(&value)?
        }))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TabsAction {
    List,
    New,
    Select,
    Close,
}

/// Lists, opens, selects, or closes the browser panel's tabs. Other browser tools act on the
/// selected tab.
#[derive(Deserialize, JsonSchema)]
struct TabsInput {
    action: TabsAction,
    /// The tab's index from `list`, for `select` and `close`.
    index: Option<usize>,
    /// The URL to open, for `new`.
    url: Option<String>,
}

#[derive(Clone)]
struct TabsTool(WeakEntity<Browser>);

impl McpServerTool for TabsTool {
    type Input = TabsInput;
    type Output = ();
    const NAME: &'static str = "browser_tabs";

    async fn run(&self, input: TabsInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        self.0
            .update(cx, |browser, cx| browser.note_agent_use(cx))?;
        let index = || input.index.context("`index` is required for this action");
        match input.action {
            TabsAction::List => {}
            TabsAction::New => {
                let url = input.url.clone();
                self.0
                    .update(cx, |browser, cx| browser.new_tab(url, cx))?
                    .await?;
                wait_until_loaded(&self.0, cx).await;
            }
            TabsAction::Select => {
                let index = index()?;
                self.0.update(cx, |browser, cx| {
                    anyhow::ensure!(index < browser.tabs().len(), "there is no tab {index}");
                    browser.activate_tab(index, cx);
                    Ok(())
                })??;
            }
            TabsAction::Close => {
                let index = index()?;
                self.0
                    .update(cx, |browser, cx| {
                        anyhow::ensure!(index < browser.tabs().len(), "there is no tab {index}");
                        Ok(browser.close_tab(index, cx))
                    })??
                    .await?;
            }
        }
        let listing = self.0.read_with(cx, |browser, _| {
            let mut listing = String::new();
            for (index, tab) in browser.tabs().iter().enumerate() {
                let marker = if browser.active_tab_index() == Some(index) {
                    "*"
                } else {
                    " "
                };
                writeln!(listing, "{marker}{index}: {} ({})", tab.title, tab.url).ok();
            }
            listing
        })?;
        Ok(text(if listing.is_empty() {
            "No tabs are open.".to_string()
        } else {
            listing
        }))
    }
}

/// Answers the active tab's open `alert`, `confirm`, or `prompt` dialog.
#[derive(Deserialize, JsonSchema)]
struct HandleDialogInput {
    /// Whether to accept (OK) or dismiss (Cancel) the dialog.
    accept: bool,
    /// The text to enter, for a `prompt` dialog.
    prompt_text: Option<String>,
}

#[derive(Clone)]
struct HandleDialogTool(WeakEntity<Browser>);

impl McpServerTool for HandleDialogTool {
    type Input = HandleDialogInput;
    type Output = ();
    const NAME: &'static str = "browser_handle_dialog";

    async fn run(&self, input: HandleDialogInput, cx: &mut AsyncApp) -> Result<ToolResponse<()>> {
        self.0
            .update(cx, |browser, cx| browser.note_agent_use(cx))?;
        self.0
            .update(cx, |browser, cx| {
                browser.handle_dialog(input.accept, input.prompt_text, cx)
            })?
            .await?;
        Ok(text(if input.accept {
            "Accepted the dialog."
        } else {
            "Dismissed the dialog."
        }))
    }
}

/// The middle of a CDP quad, given as four corner points `[x1, y1, ..., x4, y4]`.
fn quad_center(quad: &Value) -> Option<(f32, f32)> {
    let points: Vec<f64> = quad.as_array()?.iter().filter_map(Value::as_f64).collect();
    if points.len() != 8 {
        return None;
    }
    let x = (points[0] + points[2] + points[4] + points[6]) / 4.0;
    let y = (points[1] + points[3] + points[5] + points[7]) / 4.0;
    Some((x as f32, y as f32))
}

fn parse_key_combination(combination: &str) -> Result<(String, KeyModifiers)> {
    let mut parts: Vec<&str> = combination.split('+').collect();
    // `+` itself is a key.
    if combination.ends_with("++") || combination == "+" {
        parts.retain(|part| !part.is_empty());
        parts.push("+");
    }
    let key = parts
        .pop()
        .filter(|key| !key.is_empty())
        .context("no key given")?;
    let mut modifiers = KeyModifiers::default();
    for modifier in parts {
        match modifier {
            "Alt" | "Option" => modifiers.alt = true,
            "Control" | "Ctrl" => modifiers.control = true,
            "Meta" | "Cmd" | "Command" => modifiers.meta = true,
            "Shift" => modifiers.shift = true,
            other => anyhow::bail!("unknown modifier `{other}`; use Alt, Control, Meta, or Shift"),
        }
    }
    Ok((key.to_string(), modifiers))
}

/// Writes the accessibility tree as an indented list, one element per line, and assigns refs to
/// the elements an agent can act on.
fn render_snapshot(nodes: &[Value]) -> (String, HashMap<String, i64>) {
    let by_id: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|node| Some((node["nodeId"].as_str()?, node)))
        .collect();
    let mut output = String::new();
    let mut element_refs = HashMap::default();
    if let Some(root) = nodes.first() {
        render_node(root, 0, &by_id, &mut output, &mut element_refs);
    }
    if output.len() > MAX_SNAPSHOT_CHARS {
        let cut = output.floor_char_boundary(MAX_SNAPSHOT_CHARS);
        output.truncate(cut);
        output.push_str(
            "\n[The snapshot was cut short. Scroll with browser_press_key PageDown, or read parts \
             of the page with browser_evaluate.]",
        );
    }
    (output, element_refs)
}

fn render_node(
    node: &Value,
    depth: usize,
    by_id: &HashMap<&str, &Value>,
    output: &mut String,
    element_refs: &mut HashMap<String, i64>,
) {
    let role = node["role"]["value"].as_str().unwrap_or_default();
    let name = node["name"]["value"].as_str().unwrap_or_default().trim();
    let render_children =
        |depth: usize, output: &mut String, element_refs: &mut HashMap<String, i64>| {
            for child_id in node["childIds"].as_array().into_iter().flatten() {
                if let Some(child) = child_id.as_str().and_then(|id| by_id.get(id)) {
                    render_node(child, depth, by_id, output, element_refs);
                }
            }
        };
    if output.len() > MAX_SNAPSHOT_CHARS || matches!(role, "InlineTextBox" | "LineBreak") {
        return;
    }
    if node["ignored"].as_bool() == Some(true) || WRAPPER_ROLES.contains(&role) {
        render_children(depth, output, element_refs);
        return;
    }
    let indent = "  ".repeat(depth);
    if role == "StaticText" {
        if !name.is_empty() {
            writeln!(output, "{indent}- text: {name:?}").ok();
        }
        return;
    }

    write!(output, "{indent}- {role}").ok();
    if !name.is_empty() {
        write!(output, " {name:?}").ok();
    }
    for property in node["properties"].as_array().into_iter().flatten() {
        let value = &property["value"]["value"];
        match (property["name"].as_str().unwrap_or_default(), value) {
            ("level", level) if level.is_number() => write!(output, " [level={level}]"),
            (state @ ("checked" | "pressed" | "selected" | "expanded"), Value::Bool(true)) => {
                write!(output, " [{state}]")
            }
            ("checked", Value::String(mixed)) if mixed == "mixed" => {
                write!(output, " [checked=mixed]")
            }
            (state @ ("disabled" | "focused" | "required"), Value::Bool(true)) => {
                write!(output, " [{state}]")
            }
            _ => Ok(()),
        }
        .ok();
    }
    if let Some(backend_node_id) = node["backendDOMNodeId"].as_i64() {
        let element_ref = format!("e{}", element_refs.len() + 1);
        write!(output, " [ref={element_ref}]").ok();
        element_refs.insert(element_ref, backend_node_id);
    }
    match node["value"]["value"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        Some(value) => writeln!(output, ": {value:?}"),
        None => writeln!(output),
    }
    .ok();
    if !(ROLES_NAMED_BY_THEIR_TEXT.contains(&role) && !name.is_empty()) {
        render_children(depth + 1, output, element_refs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeBrowser, fake_connection};
    use gpui::{AppContext as _, Entity, Task, TestAppContext};
    use std::rc::Rc;

    fn ax_node(id: &str, role: &str, name: &str, children: &[&str], backend: Option<i64>) -> Value {
        let mut node = json!({
            "nodeId": id,
            "ignored": false,
            "role": { "value": role },
            "name": { "value": name },
            "childIds": children,
        });
        if let Some(backend) = backend {
            node["backendDOMNodeId"] = backend.into();
        }
        node
    }

    fn sample_tree() -> Vec<Value> {
        let mut heading = ax_node("3", "heading", "Sign in", &["4"], Some(30));
        heading["properties"] = json!([{ "name": "level", "value": { "value": 1 } }]);
        let mut email = ax_node("5", "textbox", "Email", &[], Some(50));
        email["value"] = json!({ "value": "me@example.com" });
        email["properties"] = json!([{ "name": "focused", "value": { "value": true } }]);
        let mut hidden = ax_node("8", "generic", "", &["9"], None);
        hidden["ignored"] = true.into();
        vec![
            ax_node("1", "RootWebArea", "Login", &["2"], Some(10)),
            ax_node("2", "generic", "", &["3", "5", "6", "8"], Some(20)),
            heading,
            ax_node("4", "StaticText", "Sign in", &[], Some(40)),
            email,
            ax_node("6", "button", "Continue", &["7"], Some(60)),
            ax_node("7", "StaticText", "Continue", &[], Some(70)),
            hidden,
            ax_node("9", "paragraph", "", &["10"], Some(90)),
            ax_node("10", "StaticText", "Forgot it?", &[], Some(100)),
        ]
    }

    #[test]
    fn test_snapshot_lists_elements_with_refs_and_skips_wrappers() {
        let (snapshot, element_refs) = render_snapshot(&sample_tree());
        assert_eq!(
            snapshot,
            concat!(
                "- heading \"Sign in\" [level=1] [ref=e1]\n",
                "- textbox \"Email\" [focused] [ref=e2]: \"me@example.com\"\n",
                "- button \"Continue\" [ref=e3]\n",
                "- paragraph [ref=e4]\n",
                "  - text: \"Forgot it?\"\n",
            )
        );
        assert_eq!(element_refs["e2"], 50);
        assert_eq!(element_refs["e3"], 60);
    }

    #[test]
    fn test_key_combinations_split_into_key_and_modifiers() {
        let (key, modifiers) = parse_key_combination("Shift+Tab").unwrap();
        assert_eq!(key, "Tab");
        assert!(modifiers.shift && !modifiers.meta);
        let (key, modifiers) = parse_key_combination("Meta++").unwrap();
        assert_eq!(key, "+");
        assert!(modifiers.meta);
        assert_eq!(
            parse_key_combination("Hyper+a").unwrap_err().to_string(),
            "unknown modifier `Hyper`; use Alt, Control, Meta, or Shift"
        );
    }

    async fn expect(chrome: &FakeBrowser, method: &str, result: Value) -> Value {
        let command = chrome.next_command().await;
        assert_eq!(command["method"], method, "unexpected command {command}");
        chrome.reply(&command, result).await;
        command
    }

    async fn browser_with_tab(cx: &mut TestAppContext) -> (Entity<Browser>, FakeBrowser) {
        let (connection, chrome) = fake_connection(cx);
        let browser =
            cx.new(|_| Browser::new(Rc::new(move |_| Task::ready(Ok(connection.clone())))));
        let opened = browser.update(cx, |browser, cx| browser.new_tab(None, cx));
        expect(&chrome, "Target.setDiscoverTargets", json!({})).await;
        expect(
            &chrome,
            "Target.createTarget",
            json!({ "targetId": "tab-1" }),
        )
        .await;
        expect(
            &chrome,
            "Target.attachToTarget",
            json!({ "sessionId": "session-1" }),
        )
        .await;
        expect(&chrome, "Page.enable", json!({})).await;
        expect(&chrome, "Runtime.enable", json!({})).await;
        opened.await.unwrap();
        (browser, chrome)
    }

    fn reply_text(response: ToolResponse<()>) -> String {
        match &response.content[..] {
            [ToolResponseContent::Text { text }] => text.clone(),
            _ => panic!("expected one text reply"),
        }
    }

    #[gpui::test]
    async fn test_agent_clicks_an_element_named_by_its_snapshot(cx: &mut TestAppContext) {
        let (browser, chrome) = browser_with_tab(cx).await;
        let weak = browser.downgrade();

        let snapshot = cx.spawn({
            let weak = weak.clone();
            async move |mut cx| SnapshotTool(weak).run(SnapshotInput {}, &mut cx).await
        });
        expect(
            &chrome,
            "Accessibility.getFullAXTree",
            json!({ "nodes": sample_tree() }),
        )
        .await;
        expect(
            &chrome,
            "Runtime.evaluate",
            json!({ "result": { "value": { "url": "http://localhost:3000/login", "title": "Login" } } }),
        )
        .await;
        let snapshot = reply_text(snapshot.await.unwrap());
        assert!(snapshot.starts_with("Page: Login (http://localhost:3000/login)\n- heading"));

        let click = cx.spawn({
            let weak = weak.clone();
            async move |mut cx| {
                ClickTool(weak)
                    .run(
                        ClickInput {
                            element_ref: "e3".to_string(),
                            double_click: false,
                        },
                        &mut cx,
                    )
                    .await
            }
        });
        let scroll = expect(&chrome, "DOM.scrollIntoViewIfNeeded", json!({})).await;
        assert_eq!(scroll["params"]["backendNodeId"], 60);
        expect(
            &chrome,
            "DOM.getContentQuads",
            json!({ "quads": [[10, 20, 30, 20, 30, 40, 10, 40]] }),
        )
        .await;
        let mut mouse = Vec::new();
        for _ in 0..3 {
            let event = expect(&chrome, "Input.dispatchMouseEvent", json!({})).await;
            mouse.push((
                event["params"]["type"].clone(),
                event["params"]["x"].clone(),
                event["params"]["y"].clone(),
            ));
        }
        assert_eq!(
            mouse,
            vec![
                (json!("mouseMoved"), json!(20.0), json!(30.0)),
                (json!("mousePressed"), json!(20.0), json!(30.0)),
                (json!("mouseReleased"), json!(20.0), json!(30.0)),
            ]
        );
        assert_eq!(reply_text(click.await.unwrap()), "Clicked e3.");

        // A new page invalidates the refs.
        chrome
            .emit(
                Some("session-1"),
                "Page.frameNavigated",
                json!({ "frame": { "id": "tab-1", "url": "http://localhost:3000/home" } }),
            )
            .await;
        cx.run_until_parked();
        let stale = cx
            .spawn(async move |mut cx| {
                ClickTool(weak)
                    .run(
                        ClickInput {
                            element_ref: "e3".to_string(),
                            double_click: false,
                        },
                        &mut cx,
                    )
                    .await
            })
            .await;
        assert_eq!(
            stale.unwrap_err().to_string(),
            "No element e3 on this page. Take a new browser_snapshot; refs reset when the page changes."
        );
    }

    /// Drives the installed Chrome: `cargo test -p browser -- --ignored`.
    #[gpui::test]
    #[ignore]
    async fn test_tools_fill_and_submit_a_form_in_real_chrome(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let profile = tempfile::tempdir().unwrap();
        let chrome = crate::chrome::launch(
            &crate::find_chrome(None).unwrap(),
            profile.path(),
            &cx.executor(),
        )
        .unwrap();
        let connection = chrome.connection.clone();
        let browser =
            cx.new(|_| Browser::new(Rc::new(move |_| Task::ready(Ok(connection.clone())))));
        let weak = browser.downgrade();
        let mut async_cx = cx.to_async();

        let page = "data:text/html,<title>Form</title><label>Name <input id=name value=old></label>\
            <button onclick=\"document.title = 'Hello ' + document.getElementById('name').value\">Greet</button>";
        let opened = NavigateTool(weak.clone())
            .run(
                NavigateInput {
                    url: page.to_string(),
                },
                &mut async_cx,
            )
            .await
            .unwrap();
        assert!(reply_text(opened).starts_with("Page: Form"));

        let snapshot = reply_text(
            SnapshotTool(weak.clone())
                .run(SnapshotInput {}, &mut async_cx)
                .await
                .unwrap(),
        );
        let ref_for = |role: &str| {
            let line = snapshot
                .lines()
                .find(|line| line.trim_start().starts_with(&format!("- {role}")))
                .unwrap_or_else(|| panic!("no {role} in {snapshot}"));
            line.split("[ref=")
                .nth(1)
                .unwrap()
                .split(']')
                .next()
                .unwrap()
                .to_string()
        };
        let (field, button) = (ref_for("textbox"), ref_for("button"));

        TypeTool(weak.clone())
            .run(
                TypeInput {
                    element_ref: field,
                    text: "zedd".to_string(),
                    submit: false,
                },
                &mut async_cx,
            )
            .await
            .unwrap();
        ClickTool(weak.clone())
            .run(
                ClickInput {
                    element_ref: button,
                    double_click: false,
                },
                &mut async_cx,
            )
            .await
            .unwrap();
        let title = reply_text(
            EvaluateTool(weak.clone())
                .run(
                    EvaluateInput {
                        expression: "document.title".to_string(),
                    },
                    &mut async_cx,
                )
                .await
                .unwrap(),
        );
        assert_eq!(
            title, "\"Hello zedd\"",
            "typing replaced the old value and the click ran"
        );

        let screenshot = ScreenshotTool(weak)
            .run(ScreenshotInput {}, &mut async_cx)
            .await
            .unwrap();
        assert!(
            matches!(&screenshot.content[..], [ToolResponseContent::Image { mime_type, .. }] if mime_type == "image/png")
        );
        drop(chrome);
    }

    #[gpui::test]
    async fn test_tools_refuse_to_act_behind_an_open_dialog(cx: &mut TestAppContext) {
        let (browser, chrome) = browser_with_tab(cx).await;
        chrome
            .emit(
                Some("session-1"),
                "Page.javascriptDialogOpening",
                json!({ "type": "alert", "message": "Saved", "defaultPrompt": "" }),
            )
            .await;
        cx.run_until_parked();

        let weak = browser.downgrade();
        let blocked = cx
            .spawn(async move |mut cx| SnapshotTool(weak).run(SnapshotInput {}, &mut cx).await)
            .await;
        assert_eq!(
            blocked.unwrap_err().to_string(),
            "The page is showing a JavaScript alert dialog: \"Saved\". Answer it with browser_handle_dialog first."
        );
    }
}
