# Built-in browser panel that agents can drive

- Date: 2026-10-01 17:15
- Status: approved (M: "go with your recommendation", "yes, go ahead"); done except M's visual check (latency, `<select>` popups, picker highlight)
- Repos: zedd   Branch: zedd-built-in-browser (from main, M asked to commit, push and open a PR)
- Folders: crates/context_server, crates/project, crates/agent_servers, crates/agent_ui, crates/zed_actions, crates/zed, assets/settings, new crates/browser and crates/browser_panel
- Files: crates/context_server/src/listener.rs, crates/project/src/context_server_store.rs, crates/agent_servers/src/acp.rs (test only), crates/agent_ui/src/agent_panel.rs, crates/zed_actions/src/lib.rs, crates/zed/src/zed.rs, crates/zed/Cargo.toml, Cargo.toml, assets/settings/default.json, crates/settings_content (browser settings), README.md, new crates
- Research: .claude/research/2026-10-01-1702_built-in-browser_research.md
- Subagents: none

## Context and problem

M wants a browser inside zedd like Claude desktop's browser pane (M's screenshot: a right-side panel with tabs, back/forward/reload, URL bar, element picker, an empty state "Type a URL or ask Claude to open a site", and "Detect dev server"). Agents (zedd's own agent and Claude Code) must be able to open pages, read them, click, type and take screenshots on the same page the user sees.

## Goals / Non-goals

Goals:
- A right-dock Browser panel per workspace: tabs, back/forward/reload, URL bar, page drawn by zedd, mouse/scroll/keyboard work on the page.
- Agents get browser tools automatically in local projects: zedd's agent through the project's MCP server list, Claude Code through ACP `mcpServers`.
- Tools act on the panel's active tab; an agent call opens the panel (without taking focus) so the user watches.
- Element picker: point at an element, and a description of it lands in the agent message box.
- Detect dev server: list local web servers started from this project and open one.
- Chrome exits when zedd exits, including a crash.

Non-goals: bundling Chrome; Linux/Windows support (they compile, the panel says the platform is unsupported); file uploads, downloads, DevTools UI, extensions; network-request log tool (can follow); remote (SSH) projects get no agent tools (the panel still works locally).

## What is true today

- No webview or CDP code exists [verified: grep for wry, WKWebView, chromiumoxide, webview2 in crates: none].
- `context_server::listener::McpServer` is an in-process MCP server over a Unix socket with typed tools (`McpServerTool`), `tools/list`, `tools/call` and custom request handlers [verified: crates/context_server/src/listener.rs:34-460]. It has no callers since 2025-08-28 (930189ed83), and the `--nc` stdio bridge that let agents reach it was removed in ef5da3ccc2 (2026-05-29) [verified: git log -S].
- zedd's MCP HTTP client sends POST only, accepts `application/json` replies, and accepts 202 for notifications [verified: crates/context_server/src/transport/http.rs:205-255].
- A project's MCP servers come from settings via `resolve_all_context_server_settings`, recomputed on settings and worktree changes [verified: crates/project/src/context_server_store.rs:462-505, 1730-1860]; `configured_server_ids` lists enabled ones [verified: :378].
- zedd's agent turns each running MCP server's tools into agent tools, including image results, with per-call authorization [verified: crates/agent/src/tools/context_server_registry.rs:304-428]; the default Write profile enables all MCP servers [verified: assets/settings/default.json:1298].
- `mcp_servers_for_project` sends each configured HTTP server to ACP agents at session new/load/resume [verified: crates/agent_servers/src/acp.rs:5467-5513, 1649, 1779, 1823]; Claude Code accepts HTTP MCP [verified: adapter `mcpCapabilities.http: true`].
- GPUI shows changing frames with `img(Arc<RenderImage>)` and needs `window.drop_image` for the old frame [verified: crates/livekit_client/src/remote_video_track_view.rs:87-101].
- Dock panels implement `workspace::dock::Panel` [verified: crates/workspace/src/dock.rs:37-100; example crates/debugger_ui/src/debugger_panel.rs:1514-1580] and are loaded in `initialize_panels` [verified: crates/zed/src/zed.rs:779-785].
- The agent message box can take inserted text [verified: crates/agent_ui/src/message_editor.rs:1948 `insert_text`]; actions route to it the way `AddSelectionToThread` does [verified: crates/agent_ui/src/agent_panel.rs:638-680].
- Process setup in a child before exec has a precedent [verified: crates/util/src/util.rs:437 `set_pre_exec_to_start_new_session`]; toolchain 1.98.1 has `std::io::pipe` [verified: rust-toolchain.toml].
- Headless Chrome 154 accepts CDP clicks as trusted, returns the accessibility tree, screencasts on change, and captures screenshots [verified: scratchpad cdp_probe].

## Options considered

Engine: see the research file (native WebKit view 5/10, CEF 7/10, headless Chrome drawn by zedd 8/10, external Chrome 4/10). Decided: headless Chrome drawn by zedd.

Connection to Chrome:

| | Debug port + WebSocket | `--remote-debugging-pipe` (fds 3 and 4) |
| --- | --- | --- |
| How | Chrome opens a localhost port; zedd reads `DevToolsActivePort`, connects a WebSocket | zedd passes two pipes; CDP messages are JSON separated by NUL bytes |
| If zedd crashes | Chrome keeps running and holds the profile lock; next launch fails | Chrome sees the pipe close and exits |
| Exposure | Any local process can drive the browser through the port | Only zedd holds the pipes |
| Dependencies | async-tungstenite client | std pipes, one `pre_exec` with `dup2` |
| Completeness | 6/10 | 9/10 |

RECOMMENDATION: pipe, because it ties Chrome's life to zedd's and exposes no port.

Agent tool path:

| | Native agent tools + separate MCP server | One MCP server registered as a project MCP server |
| --- | --- | --- |
| How | `AgentTool` impls for zedd's agent, and an MCP server for Claude Code | One MCP server; the project list feeds both zedd's agent and ACP agents |
| Code | Two adapters for one tool set | One |
| Permissions | Custom | zedd's existing MCP tool authorization; Claude Code's own permission modes |
| Completeness | 7/10 | 9/10 |

RECOMMENDATION: one MCP server, because the project MCP list already reaches both kinds of agent.

## Decision

Headless Chrome over a pipe, frames drawn in a GPUI dock panel, and one in-process MCP server over HTTP (127.0.0.1, random port, random bearer token) registered as a built-in server in each local project's MCP server store. The existing Unix-socket `McpServer` becomes an HTTP server (its socket transport is unreachable since `--nc` was removed). Rejected: WebKit view, CEF, external Chrome, playwright-mcp reuse (research file).

## Design

```
crates/context_server  listener::McpServer ── HTTP transport (tiny_http thread ─► GPUI task) <-- changed (was Unix socket)
crates/project         ContextServerStore ── built-in servers merged into settings            <-- changed
crates/browser  (new)  chrome.rs   find + launch Chrome, pipe transport, kill on drop
                       cdp.rs      request/response by id, sessions per tab, event stream
                       browser.rs  Browser entity: tabs, navigation, screencast frames, input, console, dialogs
                       snapshot.rs accessibility tree -> text with refs
                       tools.rs    MCP tools over a Browser entity
crates/browser_panel (new)  BrowserPanel: dock panel UI, hosts the MCP server, registers it, picker, dev servers
crates/agent_ui        handles zed_actions::agent::AddBrowserElementToThread                 <-- changed
```

- **One Chrome per zedd process** (a lazily started global), one `Browser` entity per workspace owning its own tabs (CDP targets). Profile: `<data_dir>/browser/profile`, persistent so logins survive restarts. A second zedd process using the same profile gets a clear error, not a fallback.
- **Viewport:** the page area's size in logical pixels becomes the CSS viewport (`Emulation.setDeviceMetricsOverride`) with the window's scale factor, so text is sharp and mouse positions map 1:1.
- **Frames:** only the active tab of a visible panel screencasts. JPEG frames decode on a background thread into a BGRA `RenderImage`; the previous frame is released with `drop_image`.
- **Input:** mouse down/up/move and wheel via `Input.dispatchMouseEvent`; printable text via `Input.insertText`; other keys (Enter, Tab, arrows, Backspace, shortcuts) via `Input.dispatchKeyEvent`.
- **Dialogs:** `alert/confirm/prompt` show a bar in the panel with Accept/Dismiss; agent tools report an open dialog and `browser_handle_dialog` answers it.
- **Tools** (Playwright/Cursor naming): `browser_navigate` (URL, or back/forward/reload), `browser_snapshot`, `browser_click`, `browser_type`, `browser_press_key`, `browser_screenshot`, `browser_console`, `browser_evaluate`, `browser_tabs`, `browser_handle_dialog`. Refs come from the accessibility tree and map to DOM nodes; refs reset on navigation, and a stale ref returns an error telling the agent to take a new snapshot.
- **MCP server security:** binds 127.0.0.1 only; rejects requests without the bearer token (401) and any request carrying an `Origin` header (blocks web pages, including the panel's own Chrome, from reaching it).
- **Settings** (`browser`): `dock` (right), `default_width`, `button`, `chrome_path` (optional), `agent_tools` (default true; false removes the MCP server).

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation | Noticed by |
| --- | --- | --- | --- | --- |
| No Chromium browser installed | Low on M's Mac | Panel unusable | Clear message naming `browser.chrome_path` and the browsers searched | Panel empty state |
| Screencast lag | Medium | Typing feels slow | Frames only for the visible active tab; ack-based flow control; measure in the debug build | Visual check |
| `<select>` popups not drawn in headless | Medium | User can't pick an option by mouse | Keyboard (arrow keys) still works; agents use refs; documented | Visual check |
| CDP experimental methods change | Low | Screencast or snapshot breaks after a Chrome update | Errors surface in the panel and tool results, not silently | Tool error text |
| Agents drive external sites | Medium | Unwanted actions | zedd's per-call MCP tool authorization; Claude Code permission modes; `agent_tools: false` | Permission prompts |
| Two zedd processes share the profile | Low | Second launch fails | Explicit error | Panel message |

## Acceptance criteria

- Given a running zedd, when an MCP client POSTs `tools/list` with the token, then it gets the browser tools; without the token it gets 401; with an `Origin` header it gets 403.
- Given a local project with `browser.agent_tools` true, when a Claude Code session starts, then its `mcpServers` include the browser server with the Authorization header; for a remote project it does not.
- Given zedd's agent in the Write profile, when the project loads, then `browser_*` tools are available.
- Given the panel, when the user types a URL and presses Enter, then the page loads and draws, and clicks and typing reach it.
- Given an agent calls `browser_snapshot` then `browser_click` on a ref, then the click happens on the page in the panel.
- Given zedd is killed with SIGKILL, then its Chrome exits.
- Given the picker, when the user clicks an element, then a description of it appears in the agent message box.
- Given a dev server started from the project, when the user clicks "Detect dev server", then its URL is offered and opens.

## Rollout and rollback

Ships in the next zedd build; `browser.agent_tools: false` removes agent access; hiding the panel button keeps it out of the way. Rollback: revert the branch; the profile folder under the data dir can be deleted.

## Parts

### Part 1: MCP over HTTP for zedd's in-process server
- [x] `McpServer::new(name, cx)` binds 127.0.0.1:0 with tiny_http on a thread, forwards each POST to a GPUI task, returns the JSON reply; notifications get 202 (done when: tests drive initialize, tools/list, tools/call over real HTTP).
- [x] Bearer token and Origin checks (done when: tests get 401 and 403).
- [x] Built-in `initialize` and `ping` replies; Unix socket transport removed (done when: crate builds, no callers break).

### Part 2: built-in MCP servers in the project store
- [x] `ContextServerStore::set_built_in_server(id, settings)` / `remove_built_in_server(id)`, merged into resolved settings for local projects only (done when: test sees the id in `configured_server_ids` and the server starts; remote store ignores it).
- [x] ACP sessions receive it (done when: test of `mcp_servers_for_project` shows an HTTP server with the Authorization header).

### Part 3: Chrome process and CDP connection (crates/browser)
- [x] Find Chrome (setting, then known app paths); launch headless with the pipe transport and the profile dir (done when: an integration test, ignored by default, launches Chrome and calls `Browser.getVersion`).
- [x] CDP client: ids, replies, errors, sessions, events (done when: unit tests over an in-memory pipe pass).
- [x] Chrome exits when zedd's end of the pipe closes (done when: the integration test drops the connection and the process exits within 5 s).

### Part 4: Browser model
- [x] Tabs: create, close, activate, popups become tabs; URL/title/loading from events (done when: fake-CDP tests pass).
- [x] Navigate, back, forward, reload; viewport; screencast for the active visible tab; frame decode (done when: fake-CDP tests check commands and that a frame becomes an image).
- [x] Input forwarding, console buffer, dialog state (done when: tests check the CDP commands sent).

### Part 5: Browser panel UI (crates/browser_panel)
- [x] Dock panel, settings, toggle action, registration in zed.rs (done when: panel opens from the dock button and command palette).
- [x] Tab strip, nav buttons, URL bar, page view, input handlers, empty state, no-Chrome message, dialog bar (done when: GPUI tests for URL submit and tab switching pass; visual check in the debug build).

### Part 6: Agent tools
- [x] Snapshot with refs; click, type, press key, navigate, screenshot, console, evaluate, tabs, dialog (done when: fake-CDP tests per tool pass, including stale ref and open dialog errors).
- [x] Panel hosts the MCP server, registers it in the project when `agent_tools` is on, opens itself on agent use (done when: test registers and unregisters on the setting; real run with Claude Code navigates and clicks).

### Part 7: Element picker
- [x] Picker button turns on inspect mode; the picked element's description goes to the agent message box via `zed_actions::agent::AddBrowserElementToThread` (done when: test checks the description text and the action; manual pick works).

### Part 8: Detect dev server
- [x] List listening localhost ports whose process runs inside a project folder, keep those answering HTTP, offer them in the empty state (done when: test with a local listener spawned in a temp project dir finds it; manual check).

### Part 9: Verification and docs
- [x] Suites for touched crates, clippy, rustfmt on changed lines (done when: all pass).
- [ ] Debug build: browse, type, scroll, agent session with Claude Code and zedd's agent, latency and `<select>` checks (done when: results recorded here).
- [x] README section and default.json comments (done when: written).

## Standardized review

- **Bottom line:** executable; no blocking findings; one must-verify (headless inspect-mode overlay drawing, Part 7) and one note (Part 8 is macOS/Linux `lsof`-based).
- Goal fidelity: pass (every element of M's screenshot maps to a part; non-goals named).
- Current-system accuracy: pass (each claim cited; listener has no callers, verified by git log).
- Scope and cohesion: pass (two new crates; changes elsewhere are one transport, one registration hook, one agent action).
- Architecture and dependency direction: pass (browser model knows nothing of workspace or agents; the panel depends on the model; agent_ui depends only on a zed_actions action; the project store learns "built-in servers", not "browser").
- Sequencing and dependencies: pass (transport and registration first, then Chrome, model, UI, tools, then picker and dev servers).
- Completeness: pass (settings, security, lifecycle, docs, rollback).
- Failure and recovery: pass (no Chrome, crash cleanup via pipe, profile lock error, stale refs, dialogs, CDP errors surfaced).
- Verification: pass with a gap: Chrome-dependent tests are ignored by default; the real-Chrome integration test and the debug-build check cover them.
- Feasibility and precision: must-verify: whether `Overlay.setInspectMode` highlights appear in headless screencast frames; fallback is a zedd-drawn highlight from `DOM.getBoxModel` on hover, decided in Part 7.
- Decision readiness: branch name is M's call (git is M's domain).

## Changes made

- Part 1: `context_server::listener::McpServer` now serves streamable HTTP on 127.0.0.1 (tiny_http thread per request, replies through a GPUI task), with a random bearer token, 403 for any `Origin` header, built-in `initialize`/`ping`, 202 for notifications. The Unix socket transport is gone (no callers). Tests: `test_agents_can_initialize_list_and_call_tools_over_http`, `test_requests_without_the_token_or_from_web_pages_are_refused`.
- Part 2: `ContextServerStore::set_built_in_server` / `remove_built_in_server`; built-in servers merge into resolved settings (settings win on an id clash) and are skipped for remote projects. Test `test_built_in_server_runs_like_a_configured_server`; all 22 store tests pass. Deviation: ACP delivery is not unit tested (would need a context_server dev-dependency in agent_servers); `mcp_servers_for_project` is unchanged and maps any running HTTP server; the real Claude Code run in Part 6 checks it.
- Part 3: new crate `browser`: `cdp.rs` (ids, replies, errors, sessions, event fan-out, fails all on exit), `chrome.rs` (find Chrome, launch with `--remote-debugging-pipe` on fds 3/4 via `pre_exec` + `F_DUPFD`, one shared Chrome per process, restart if it exited, clear error if the profile is in use). Real-Chrome test (ignored by default) passes: Chrome exits within 5 s when zedd's pipe ends close, with no kill.
- Part 4: `Browser` entity: tabs (create, attach, enable, size before showing, popups from own tabs become tabs, destroyed targets leave), navigate with Chrome's error text, history, reload, wait for load, viewport, screencast only for the visible active tab with ack after decode, JPEG to BGRA frames, mouse/wheel/text/keys (DOM key table, macOS editing commands), console (500 per tab), dialogs, reconnect after Chrome exits. 11 tests pass.

- Part 5: crate `browser_panel`: right-dock `BrowserPanel` (settings `browser.{button,dock,default_width,chrome_path,agent_tools}`), tab strip, back/forward/reload, address bar (Enter opens, `http` for localhost), page drawn from frames (old frames released), mouse/wheel/keys forwarded, cmd-c/cmd-v through zedd's clipboard, dialog bar, error line, empty state. Registered in zed.rs/main.rs; `browser_panel` added to the action namespace list. 3 GPUI tests.
- Part 6: 10 MCP tools (`browser_navigate`, `_snapshot`, `_click`, `_type`, `_press_key`, `_screenshot`, `_console`, `_evaluate`, `_tabs`, `_handle_dialog`); refs from the accessibility tree, reset on navigation; tools refuse to act behind an open dialog. The panel hosts the server and registers it as built-in server `zedd-browser` for local projects while `agent_tools` is on; agent use opens the panel. Verified: fake-Chrome tests, a real-Chrome form test, the official MCP TypeScript SDK 1.30.1 (the client Claude Code uses) driving navigate/snapshot/click/evaluate/screenshot over HTTP (temporary probe, removed), and the debug build's own MCP client initializing `zedd-browser`.
- Part 7: crosshair picker; Chrome inspect mode (needs `DOM.enable` before `Overlay.enable`, found by a failing real-Chrome test and fixed by sending the three commands in order from one task); `zed_actions::agent::AddBrowserElementToThread` puts the element's markup and selector in the agent message box. Fake and real-Chrome tests, plus an agent_ui test.
- Part 8: `dev_servers.rs`: `lsof` listening ports, kept when the process's folder is inside the project and the port answers HTTP; one result opens directly. Parsing test plus a real python http.server test.
- Part 9: suites pass (browser 16 + 3 real-Chrome, browser_panel 5 + 1 real, context_server 100, project store 22, agent_ui 516, zed 93, settings 46, settings_content 50); clippy clean (Chrome and lsof now spawn through smol, per the repo's lint); rustfmt on changed lines; README section.

## Open questions

- Resolved: branch `zedd-built-in-browser` from main (M: commit, push, raise a PR).
- Open: M's visual check of the panel (frame lag, `<select>` dropdowns, picker highlight drawn in frames) and a live Claude Code session in the panel.
