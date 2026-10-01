# Built-in browser that agents can drive

- Date: 2026-10-01 17:02
- Status: decided (M: "go with your recommendation")
- Mode: explore options
- Repos: zedd   Folders: crates/context_server, crates/project, crates/agent, crates/agent_servers, crates/workspace, new browser crates
- Question / problem: how should zedd show a browser inside the editor that both the user and the agents (zedd's own agent and Claude Code over ACP) use on the same page, like Cursor's and Claude desktop's browser panes?

## Options: a browser panel drawn by zedd, driven by Chrome, exposed to every agent through one built-in MCP server

- **Bottom line:** run the user's installed Chrome headless, draw its frames in a zedd dock panel, forward the user's mouse and keys to it, and give agents browser tools through one MCP server that zedd hosts on localhost. It is the only option that draws as real zedd UI (no menus hidden behind the page) without shipping a 100+ MB engine, and it gives agents real clicks and the accessibility tree.
- **Where we are:** framing done; codebase sweep and web research done (source notes: 43 URLs); headless Chrome probe run on this machine. Next step: plan.
- **Need from M:** nothing; M chose "go with your recommendation".

### The problem, framed

- **What:** a browser panel inside zedd that agents can read, click, type into and screenshot, while the user watches and uses the same page.
- **Why:** agents check their own web work (a dev server, a form, a layout) without the user copying screenshots or switching apps.
- **Who:** M, using zedd's own agent and Claude Code.
- **Whom:** the agents, which get browser tools; M, who sees and can take over the page.
- **When:** every agent session that touches a web app; tools load lazily, Chrome starts on first use.
- **Where:** a new right-dock panel (M's screenshot of Claude desktop: tabs, back, forward, reload, URL bar, element picker, empty state, "Detect dev server"); a tool server registered into each local project's MCP server list.
- **Which:** Chrome DevTools Protocol (CDP, Chrome's remote-control protocol over a WebSocket) and MCP over HTTP.
- **How:** see the flow below.
- **How much:** two new crates, an HTTP transport for zedd's existing in-process MCP server, one registration hook in the project's MCP server store. No new engine is bundled.

### What must be true

- Hard constraint: GPUI draws everything itself with Metal; a native macOS view placed in the window always sits above GPUI content [verified: the only native sibling view is the blur view, inserted below GPUI, crates/gpui_macos/src/window.rs:1876-1891].
- Hard constraint: zedd ships for macOS on Apple Silicon only [verified: README "Install (macOS, Apple Silicon)"].
- Hard constraint: Claude Code's adapter accepts HTTP MCP servers [verified: adapter initialize reply `"mcpCapabilities":{"http":true,"sse":true}`, scratchpad adapter clone src/tests/acp-scenarios/origin-main/zed/session-setup.jsonl:1].
- Hard constraint: zedd's agent already turns MCP server tools into agent tools, images included [verified: crates/agent/src/tools/context_server_registry.rs:304-428], and the default "Write" profile enables all MCP servers [verified: assets/settings/default.json:1298].
- Hard constraint: the same project MCP server list is what zedd sends to external agents at session start [verified: crates/agent_servers/src/acp.rs:5467-5513, used at 1649, 1779, 1823].
- Verified by experiment (Chrome 154 on this Mac, scratchpad cdp_probe/probe.mjs): headless Chrome starts with `--remote-debugging-port=0`; a tab created with `PUT /json/new`; a CDP mouse click reaches the page as a trusted user click (`clicked true`); `Accessibility.getFullAXTree` returns the button "Go"; `Page.startScreencast` sends frames only when the page changes; `Page.captureScreenshot` works.
- Convention: tools follow the snapshot-with-refs model used by Cursor (`browser_snapshot` then `browser_click` by ref [verified: https://forum.cursor.com/t/browser-mcp-browser-snapshot-returns-only-metadata/153200]), Playwright MCP [verified: https://github.com/microsoft/playwright-mcp/blob/main/README.md] and Claude's browser (`read_page` refs; tool schemas visible in this session).
- Unverified: frame latency of the screencast at panel size [check: measure in the debug build]; `<select>` dropdown popups do not appear in headless screencast frames [check: open a page with a select in the panel].

### Options

| | A. Native WebKit view over the panel (wry / WKWebView) | B. CEF rendered off-screen into a GPUI element | C. Headless Chrome, frames drawn by zedd (recommended) | D. Separate Chrome window plus Playwright MCP |
| --- | --- | --- | --- | --- |
| How it works | A macOS WKWebView child view kept at the panel's bounds every frame | Chromium embedded in-process, paints into a buffer zedd uploads | User's Chrome runs headless; zedd draws its JPEG frames and sends input over CDP | zedd launches Chrome with a debug port; agents get playwright-mcp pointed at it |
| Draws as zedd UI? | No: covers menus, popovers, the command palette where they overlap; Zed PR #52447 was closed partly because "dropdowns hid behind the page" [verified: https://github.com/zed-industries/zed/pull/52447] | Yes | Yes | No pane at all |
| Agent control | JavaScript only: synthetic (untrusted) clicks, no accessibility tree, no network log [verified: Apple WKWebView docs; WebKit refuses http(s) scheme handlers, WKWebViewConfiguration.mm ~L568] | Full CDP | Full CDP: trusted input, accessibility tree, console, network, screenshots [verified: probe] | Full CDP |
| Cost | Small dependency; plus VS Code-style hide-and-placeholder workaround for overlaps [verified: VS Code webContentsViewHost.ts] | >100 MB installer, >300 MB installed [verified: CEF maintainers, chromiumembedded/cef#3836]; ~38% CPU copy at 60 fps in gpui-cef [verified: https://github.com/bokuweb/gpui-cef] | Needs Chrome (or Chromium, Edge, Brave) installed; JPEG decode per frame | Needs Node for playwright-mcp; not built in |
| Fails when | Any overlay opens over the page; sites that ignore untrusted events | Build, signing, multi-process helpers | No Chromium browser installed; native `<select>` popups and IME are weaker | User wants it in the editor |
| Completeness | 5/10 | 7/10 (cost) | 8/10 | 4/10 |

### Flow of the recommended option

```
 user mouse/keys ──► BrowserPanel (GPUI dock panel)          <-- new
                         │  Input.dispatchMouseEvent / insertText
                         ▼
 agents ──► MCP over HTTP (127.0.0.1, bearer token)          <-- new transport on context_server::listener
   zedd agent: project MCP list ─┐
   Claude Code: ACP mcpServers ──┴─► browser tools ──► Browser model ──► CDP WebSocket ──► headless Chrome
                                                          ▲                                   │
                                                          └── Page.screencastFrame (JPEG) ◄───┘
                                                              drawn by the panel as an image
```

Tools act on the panel's active tab, so the user sees every agent action, and the user's own clicks change the page the agent reads next.

### Recommendation

RECOMMENDATION: C, because it is the only option that draws as real zedd UI without bundling an engine, and the probe proved trusted clicks, the accessibility tree and screenshots work with the Chrome already on this Mac.

Rejected: A (page covers zedd's own menus, weak agent control), B (engine size and build cost for a personal fork), D (not built in). Reusing playwright-mcp on top of C was also rejected: it needs Node at runtime, does not run inside zedd's agent without a separate server, and does not know which tab the panel shows [inferred from its CDP-attach model, https://github.com/microsoft/playwright-mcp/blob/main/README.md].

### Sources

- Cursor browser docs: https://cursor.com/docs/agent/tools/browser
- Cursor 2.0 changelog: https://cursor.com/changelog/2-0
- Cursor forum, `browser_snapshot` refs: https://forum.cursor.com/t/browser-mcp-browser-snapshot-returns-only-metadata/153200
- Claude Code desktop, browser and preview: https://code.claude.com/docs/en/desktop
- VS Code 1.110 agent browser tools: https://code.visualstudio.com/updates/v1_110
- VS Code browser view source: https://github.com/microsoft/vscode/blob/main/src/vs/platform/browserView/electron-main/browserView.ts
- VS Code overlay workaround: https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/browserView/electron-browser/webContentsViewHost.ts
- Playwright MCP: https://github.com/microsoft/playwright-mcp/blob/main/README.md
- Chrome DevTools MCP: https://github.com/ChromeDevTools/chrome-devtools-mcp
- CDP protocol (Page, Input, Accessibility): https://chromedevtools.github.io/devtools-protocol/tot/Page/
- wry WebViewBuilder: https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html
- gpui-kit webview: https://github.com/longbridge/gpui-kit/tree/main/crates/webview
- gpui-cef: https://github.com/bokuweb/gpui-cef
- CEF size discussion: https://github.com/chromiumembedded/cef/issues/3836
- Zed issue 21208 (built-in browser): https://github.com/zed-industries/zed/issues/21208#issuecomment-2500939041
- Zed PR 52447 (wry browser, closed): https://github.com/zed-industries/zed/pull/52447
- Apple WKWebView evaluateJavaScript: https://developer.apple.com/documentation/webkit/wkwebview/evaluatejavascript(_:completionhandler:)
- Apple WKWebView takeSnapshot: https://developer.apple.com/documentation/webkit/wkwebview/takesnapshot(with:completionhandler:)
- Full per-claim notes: scratchpad browser_research_web.md (session scratchpad)

## Standardized review

- **Bottom line:** decision-grade for a personal fork; no blocking findings; two must-verify items carried into the plan.
- Question and decision fit: pass (one question, the engine and tool path; out of scope: bundling Chrome, Linux/Windows).
- Method: pass (codebase sweep with file:line, web sources graded, one runtime probe).
- Source quality: pass (primary: CDP protocol JSON, Apple docs, VS Code and WebKit source, Cursor and Claude docs; Cursor internals marked inferred).
- Coverage and alternatives: pass (four engines plus reuse of existing MCP servers).
- Claim traceability: pass (each claim has a URL or file:line).
- Evidence handling: pass (Cursor implementation and screencast latency marked unverified).
- Analysis quality: pass (same criteria for every option).
- Codebase fit: pass (reuses the in-process MCP server, the MCP-to-tool adapter, and the project MCP list that already feeds ACP).
- Uncertainty and limits: must-verify: screencast latency and `<select>` popups; both are checked in the plan's verification part, and neither changes the engine choice because the alternatives' costs are structural.
- Recommendation proportionality: pass (names when it fails: no Chromium browser installed).
- Reproducibility: pass (probe script and steps recorded).
