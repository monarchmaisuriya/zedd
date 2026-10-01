# zedd

zedd is a personal fork of the [Zed](https://github.com/zed-industries/zed) code editor, focused on working with AI coding agents (Zed's own agent and external agents such as Claude Code over ACP). It is not affiliated with or endorsed by Zed Industries.

[Download the latest release](https://github.com/monarchmaisuriya/zedd/releases/latest) · [All releases](https://github.com/monarchmaisuriya/zedd/releases)

---

## What's different from Zed

### Agent panel

- **Transcript views:** Normal (each run of tool calls folds into one line, like "Read 9 files ›"), Thinking (also shows thinking) and Verbose (every tool call in full). Switch with the dropdown next to the send button or `ctrl-o`; setting `agent.transcript_view`.
- **Readable replies:** the agent's reply is drawn in pure white on dark themes, with tool activity, notices and thinking in a dimmer gray.
- **Long output:** `agent.tool_output_preview_lines` limits open tool and terminal output to its first N lines, with "Show all"; long terminal commands collapse to two lines; code blocks follow `agent.expand_code_block`.
- **Fork a thread** from any message or reply.
- **Background tasks:** a list of the agent's background work (a dev server, a monitor) with a Stop button. It fills in once the Claude adapter ships [agentclientprotocol/claude-agent-acp#1206](https://github.com/agentclientprotocol/claude-agent-acp/pull/1206).

### Built-in browser

- **Browser panel** (right dock, `browser panel: toggle focus`): tabs, back, forward, reload, and an address bar. zedd draws pages from your installed Chrome (or Chromium, Edge, Brave) running in the background, so menus and popups never hide behind the page. Set `browser.chrome_path` to pick a browser.
- **Agents use the same page:** in local projects, zedd's agent and external agents such as Claude Code get browser tools: open a page, read it as a list of elements, click, type, press keys, take a screenshot, read the console, run JavaScript, switch tabs, and answer dialogs. The panel opens when an agent uses it. Turn this off with `"browser": { "agent_tools": false }`.
- **Element picker:** the crosshair button lets you click an element on the page; its markup and selector go into the agent's message box.
- **Detect dev server:** on an empty panel, finds web servers started from the project's folders and opens one.
- The browser keeps its own profile, so logins persist and stay separate from your everyday browser. Chrome exits with zedd.

### Reviewing agent work

- **Branch review:** "Review Branch with Agent" in the git panel menu (or `git: review branch`) asks the agent to check your branch for bugs and security issues.
- **Diff comments:** comment on lines in a diff and send them all to the agent at once (`cmd-alt-g c` to comment, `cmd-alt-g enter` to send). In an agent's diff they go to the thread that made the changes.
- **Plan file:** when Claude asks to leave plan mode, its plan file opens next to the thread, and the approval card links to it.
- **Verify gate:** `"agent_verification": { "command": "cargo test" }` in a project's `.zed/settings.json` runs the command after each agent turn that used tools, and asks the agent once to fix a failure. Trusted projects only; a check button in the message box turns it off per thread.
- **Hooks for Zed's own agent:** `agent.hooks` in your user settings runs commands before and after tool calls, when you send a prompt, and when the agent stops. A failing hook blocks the step or reports back to the agent; hooks never approve anything on their own.

### Fixes for open Zed issues

| Zed issue | Fix in zedd |
| --- | --- |
| [#64410](https://github.com/zed-industries/zed/issues/64410) | Agent file writes no longer duplicate text. |
| [#64538](https://github.com/zed-industries/zed/issues/64538) | A thread keeps updating when the agent mentions an unknown terminal. |
| [#62828](https://github.com/zed-industries/zed/issues/62828) | After an agent process exits, the next thread starts a fresh agent. |
| [#59323](https://github.com/zed-industries/zed/issues/59323) | Agents get 2 seconds to clean up before they are killed. |
| [#60435](https://github.com/zed-industries/zed/issues/60435) | Unsent text to an external agent survives a restart. |
| [#63177](https://github.com/zed-industries/zed/issues/63177) | Saving can no longer leave an empty file after a crash. |
| [#55726](https://github.com/zed-industries/zed/issues/55726) | Opening a file to launch zedd restores your last session first. |
| [#63202](https://github.com/zed-industries/zed/issues/63202) | The threads sidebar no longer redraws every row while an agent runs. |

### Removed

- Zed Pro pricing, trial and upsell surfaces, and the Zed-hosted model provider. Bring your own model providers or external agents.

## Install (macOS, Apple Silicon)

1. Download `zedd-aarch64.dmg` from the [latest release](https://github.com/monarchmaisuriya/zedd/releases/latest), open it, and drag **zedd** to Applications.
2. The app is not signed or notarized. The first time, right-click zedd in Applications and choose **Open**, or run `xattr -dr com.apple.quarantine /Applications/zedd.app`.
3. zedd uses the system `git`, which must be installed (for example from the Xcode command line tools).

zedd uses the `dev` release channel, so it never updates itself into official Zed. It shares Zed's settings and data folders.

Other platforms have no prebuilt release; build from source.

## Build from source

Follow Zed's setup guides first: [macOS](./docs/src/development/macos.md), [Linux](./docs/src/development/linux.md), [Windows](./docs/src/development/windows.md).

```sh
cargo run -p zed                                # debug build
ZEDD_SKIP_BUNDLED_GIT=1 ./script/bundle-mac     # release DMG at target/aarch64-apple-darwin/release/zedd-aarch64.dmg
```

## Upstream

zedd tracks [zed-industries/zed](https://github.com/zed-industries/zed). Fixes that belong in Zed or in an agent adapter are proposed upstream where possible. Zed is developed by Zed Industries, Inc.; to support Zed itself, see [zed.dev](https://zed.dev).

## License

Like Zed, zedd is licensed primarily under GPL-3.0-or-later ([LICENSE-GPL](./LICENSE-GPL)), with Apache-2.0 components where marked ([LICENSE-APACHE](./LICENSE-APACHE)). Third-party license compliance follows Zed's [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) setup in `script/licenses/`.
