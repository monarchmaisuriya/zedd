# Preview long tool and terminal output, and collapse code inside tool output

- Date: 2026-09-30 22:17
- Status: Part 6 (terminal card polish) done; debug check pending M
- Repos: zedd (branch `zedd-fix-code-preview-tool`, from `main` at `58cec871fc`)   Folders: crates/agent_ui, crates/markdown, crates/settings_content, crates/agent_settings, crates/settings_ui, crates/agent, assets/settings   Files: crates/settings_content/src/agent.rs, crates/agent_settings/src/agent_settings.rs, assets/settings/default.json, crates/settings_ui/src/page_data.rs, crates/agent/src/tool_permissions.rs (test literal), crates/agent_ui/src/agent_ui.rs (test literal), crates/markdown/src/markdown.rs, crates/agent_ui/src/entry_view_state.rs, crates/agent_ui/src/conversation_view/thread_view.rs, crates/agent_ui/src/conversation_view.rs (tests)
- Subagents: none (parts share `thread_view.rs` and `entry_view_state.rs`)
- Research: `.claude/research/2026-09-30-2213_tool-output-collapse-preview_research.md` (Option 1)

## Context and problem

Long tool output and terminal output push the agent conversation apart, and code blocks inside tool output cannot be collapsed even with `agent.expand_code_block: false`. Asked for in [zed-industries/zed#58333](https://github.com/zed-industries/zed/discussions/58333). Affects anyone reading agent threads in zedd, with the native agent or ACP agents.

## Goals / Non-goals

**Goals**
- A setting `agent.tool_output_preview_lines` (default `0` = no limit, so nothing changes by default). When set to N, open tool output and terminal output show their first N lines, with a "Show all M lines" toggle and "Show less" after expanding.
- Code blocks inside tool output follow `agent.expand_code_block`, like code blocks in agent replies.
- A thread-search match inside a clamped tool output is never hidden.

**Non-goals**
- Changing when cards open (`expand_edit_card`, `expand_terminal_card` keep that job).
- Diffs and images (diffs already have `expand_edit_card`; images are size-capped).
- Terminal output over 1000 lines: it is already a fixed scroll box (`h_72`), which stays as is.
- Showing the last N lines (M chose first lines).
- `read_file`'s numbered output and code-block collapse: it is not markdown; it still gets the preview clamp.

## What is true today

- Only diffs and terminals auto-open, each behind a setting; other tool cards start collapsed [verified: `thread_view.rs:1289-1302`; `entry_view_state.rs:81-83`].
- Tool-card markdown output is rendered by `render_markdown_output`, which calls `self.render_markdown(markdown, markdown_style, cx)` with no code-block collapse flag; `read_file` output takes `render_numbered_read_file_output` instead [verified: `thread_view.rs`, `fn render_markdown_output`]. The collapse flag is applied only on the no-tool-call path [verified: `render_output_content_block`, `.collapse_code_blocks(collapse_code_blocks)` in the else branch].
- The output wrapper in `render_markdown_output` is a `v_flex` with padding and a left or top border and no height limit [verified: same function].
- Embedded terminals limit displayed lines with `TerminalMode::Embedded { max_lines_when_unfocused }`, set once at creation to `Some(1000)`; the limit applies only while the terminal is not focused; above `MAX_EMBEDDED_LINES = 1000` total lines the view becomes scrollable and the card gives it `h_72` [verified: `terminal_view.rs:176, 322-331, 378-407`; `entry_view_state.rs:743`; `thread_view.rs` terminal body].
- Per-card UI state lives in `EntryViewState` as sets keyed by tool-call id or entry position (`expanded_tool_calls`, `expanded_thinking_blocks`, `user_toggled_thinking_blocks`) [verified: `entry_view_state.rs:40-52`].
- Thread search reads the markdown of visible tool content and not terminal output [verified: `thread_search_bar.rs:374, 973`]. `Markdown` knows the active search highlight but exposes it only privately (`active_search_highlight_range`), which the code-block collapse uses to stay open on the active match [verified: `markdown.rs`, `is_code_block_collapsed`].
- Optional numeric agent settings resolve to a plain number with a documented special value in `default.json` (for example `max_idle_retained_threads: 5`, where `0` unloads all) and use the settings UI's number field [verified: `settings_content/src/agent.rs:255-260`; `agent_settings.rs:216`; `default.json:1167-1170`; `settings_ui.rs:619`; `page_data.rs:8947-8963`].

```
tool card open
   |- markdown output --> render_markdown_output --> render_markdown(...)   (no collapse flag, no height limit)
   |- read_file       --> render_numbered_read_file_output                   (no height limit)
   |- terminal        --> TerminalView embedded, max 1000 lines unfocused; >1000 lines -> h_72 scroll
   '- diff / image    --> unchanged
```

## Options considered

| Option | How it works | Tradeoffs | Fails when | Completeness |
| --- | --- | --- | --- | --- |
| 1: preview line count + route the flag | One number setting; terminals use `max_lines_when_unfocused = N`; markdown output clamped to N line heights; flag passed into `render_markdown_output` | Exact for terminals, approximate for markdown | Markdown with tall lines shows fewer than N lines | 8/10 |
| 2: tri-state like `thinking_display` | Enum also controls whether cards open | Two settings decide the same open state | Users combine it with `expand_*` settings | 8/10 |
| 3: fixed-height clamp | Reuse the thinking block's `max_h_64` for all output | No setting | Important output at the end stays hidden with no control | 5/10 |
| 4: first and last lines | Codex-style middle elision | Terminal needs a second view | Complexity | 6/10 |

RECOMMENDATION: Option 1 because it answers both asks with one default-off setting, reuses the terminal line limit, the clamp pattern, and the code-block collapse flag, and leaves opening cards to the existing settings (M chose it, first lines).

## Decision

Build Option 1. The structural reason: the limit belongs where each output kind is rendered, and each already has a lever (the terminal's embedded line limit, the markdown element's collapse flag, a container clamp), so one setting drives three existing mechanisms instead of a new rendering path. Consequence: markdown previews are line-height approximate. Rejected: 2 (second source of truth for opening cards), 3 (no control), 4 (large for terminals).

## Design

**Setting.** `agent.tool_output_preview_lines`: content `Option<usize>` in `AgentSettingsContent`, resolved `usize` in `AgentSettings`, `default.json` value `0` with a comment ("0 shows the full output"), a settings UI number field next to "Expand Code Block". Test literals that build `AgentSettings` by hand get the field (`crates/agent/src/tool_permissions.rs`, `crates/agent_ui/src/agent_ui.rs`).

**Code blocks inside tool output.** `render_markdown_output` passes `!expand_code_block` to `render_markdown(...).collapse_code_blocks(...)`. `read_file`'s numbered output is untouched (not markdown).

**Per-card "show all" state.** `EntryViewState` gains `fully_shown_tool_outputs: HashSet<ToolCallId>` with `is_tool_output_fully_shown`, `toggle_tool_output_fully_shown`. One toggle per tool call covers all of its outputs.

**Markdown and read_file preview.** When N > 0, not fully shown, and the output has more than N source lines, the wrapper in `render_markdown_output` gets `max_h(N × line height)` and `overflow_hidden`, where the line height comes from the same text style the output renders with, plus a bottom fade like the thinking block. A "Show all M lines" row toggles the state; "Show less" when fully shown. The clamp is skipped while the output's `Markdown` has the active search highlight; `markdown.rs` gains a public `has_active_search_highlight()` (the existing private range stays the single source).

**Terminal preview.** `create_terminal` sets `max_lines_when_unfocused` to N when N > 0 (else 1000). Toggling "Show all" sets it back to 1000; "Show less" sets N. A settings observer re-applies the limit to open terminals when the setting changes. The "Show all M lines" row appears when the terminal's `ContentMode::Inline { total_lines }` exceeds N. Terminals over 1000 lines keep today's scroll box. Focusing a terminal already shows all its lines (existing behavior).

```
tool card open
   |- markdown output --> render_markdown(...).collapse_code_blocks(!expand_code_block)   <-- changed
   |                      clamp N lines unless shown-all or active search match           <-- new
   |- read_file       --> numbered output, same clamp                                      <-- new
   |- terminal        --> max_lines_when_unfocused = N (1000 when shown-all)               <-- changed
   '- "Show all M lines" / "Show less" --> EntryViewState.fully_shown_tool_outputs          <-- new
```

No data, protocol, or persistence change; UI state resets per session like `expanded_tool_calls`.

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation | How noticed |
| --- | --- | --- | --- | --- |
| Markdown clamp shows fewer or partial lines | Medium | Cosmetic | Clamp by the output's own line height; toggle text says "Show all", count from source lines | Manual check at N=5 |
| Search match hidden in clamped output | Low | Search looks broken | Skip clamp while the output has the active highlight; test | UI test |
| Terminal limit stale after the setting changes | Low | Inconsistent previews | Settings observer re-applies the limit | UI test |
| Default behavior changes | Low | Surprise for users | Default 0 = unlimited; existing tests must pass unchanged | Full suites |
| Hand-built `AgentSettings` literals break | Certain | Compile error | Add the field to both literals | Compile |

## Acceptance criteria

- Given `tool_output_preview_lines` is 0, when any tool or terminal output opens, then it renders exactly as today.
- Given it is 5 and a tool's markdown output has 40 lines, when the card is open, then about the first 5 lines show with a fade and "Show all 40 lines"; clicking shows everything and "Show less".
- Given it is 5 and a terminal printed 40 lines, when the card is open and the terminal is not focused, then its first 5 lines show and "Show all 40 lines"; clicking shows all lines.
- Given `expand_code_block` is false, when a tool's markdown output contains a code block, then that block shows as one collapsed row.
- Given a thread search whose active match is inside a clamped output, when the match is activated, then that output is shown in full.
- Given the setting changes while a terminal card is open, then the terminal's preview limit updates without reopening.

## Rollout and rollback

Default `0` keeps current behavior, so no flag is needed. Parts land in order on `zedd-fix-code-preview-tool`; each leaves the workspace compiling and tested. Rollback is reverting the part's changes. Abort condition: if the markdown clamp cannot line up with rendered lines well enough in the manual check, stop and ask M before Part 3 ships.

## Parts

### Part 1: Setting
- [x] Add `tool_output_preview_lines` to content, resolved settings, `default.json`, settings UI, and the two test literals (done when: `cargo check` for `settings_content`, `agent_settings`, `settings_ui`, `agent`, `agent_ui` passes and `settings_ui` tests pass).

### Part 2: Code blocks inside tool output
- [x] `render_markdown_output` applies `collapse_code_blocks(!expand_code_block)` (done when: an `agent_ui` test renders a tool output with a code block and sees it collapsed with the setting off and expanded with it on).

### Part 3: Markdown and read_file preview
- [x] `EntryViewState` shown-all state and toggle (done when: unit test toggles it).
- [x] `Markdown::has_active_search_highlight()` (done when: markdown test covers it).
- [x] Clamp plus "Show all M lines" / "Show less" in `render_markdown_output`, skipped on the active match (done when: `agent_ui` tests cover clamped, shown-all, short-output-unclamped, and active-match-unclamped).

### Part 4: Terminal preview
- [x] `create_terminal` uses N, toggle switches between N and 1000, settings observer re-applies (done when: `agent_ui` tests see the terminal view's limit follow the setting and the toggle).
- [x] "Show all M lines" row from `total_lines` (done when: test sees the row only when total lines exceed N).

### Part 5: Verify
- [x] Suites for `markdown`, `agent`, `agent_ui`, `settings_ui`, `settings_content`, `agent_settings`; `./script/clippy` on those crates; rustfmt check; debug and release builds (done when: all pass).
- [ ] Manual check at N=5 with a long tool output, a long terminal command, a code block in tool output, and a search match inside a clamped output (done when: M confirms).

### Part 6: Terminal card polish (from M's debug check)

M's screenshot of a Claude Code thread: long commands take over the card, a "current directory" label appears when the agent sends no folder, the "Show all" link is easy to miss, and the cut-off terminal output ends abruptly.

- [x] Collapse a terminal command taller than 2 lines, only while `tool_output_preview_lines` > 0 and never while it awaits permission (done when: long command collapsed and expands on click; fenced 2-line command not collapsed; permission-pending command never collapsed)
- [x] Hide the working-folder label when the agent reports none (done when: `TerminalToolHeader` takes `Option<SharedString>`, no "current directory" string left)
- [x] "Show all M lines" / "Show less" as a full-width outlined row with a chevron (done when: existing toggle tests still click through)
- [x] Bottom fade on a cut terminal output (done when: fade present while cut, gone after Show all)

## Standardized review

### Review: plan: preview long tool and terminal output, and collapse code inside tool output

- **Bottom line:** present as is.
- **Findings:** 0 blocking, 1 must-verify, 1 note.

1. MUST-VERIFY: markdown clamp alignment. The line-height clamp is approximate for markdown with headings or wrapped lines. Fix: the Part 5 manual check; abort condition in Rollout. Need from M: nothing.
2. NOTE: one "show all" toggle per tool call covers all of its outputs. A tool with several outputs expands them together; simpler state, matches how the card itself opens. Need from M: nothing.

Criterion coverage:
- Goal fidelity: pass. Both asks (code in tool output, preview) with M's choices (option 1, first lines, default off).
- Current-system accuracy: pass. Every claim in "What is true today" cites a verified file location.
- Scope and cohesion: pass. Diffs, images, last-lines, and >1000-line terminals excluded explicitly.
- Architecture and dependency direction: pass. Settings in the settings crates; state in `EntryViewState`; rendering in `thread_view.rs`; markdown exposes one query.
- Sequencing and dependencies: pass. Setting first, then consumers; terminal last because it needs the observer.
- Completeness: pass. Settings UI, defaults, test literals, search interaction, live setting changes covered.
- Failure and recovery: pass. Default-off rollout; abort condition for the clamp.
- Verification: violation. Markdown alignment can only be judged by eye. Fix: Finding 1.
- Feasibility and precision: pass. All touched files exist; new items named.
- Decision readiness: pass. M's decisions recorded; no open decision blocks Part 1.

## Changes made

- Part 1: `tool_output_preview_lines` (content `Option<usize>`, resolved `usize`, `default.json` `0`, settings UI number field after "Expand Code Block", two test literals). Settings suites pass.
- Part 2: `render_markdown_output` applies `collapse_code_blocks(!expand_code_block)`; the collapsed row gets a `collapsed-code-block` debug selector. `test_code_blocks_in_tool_output_follow_expand_code_block` passes and fails without the flag.
- Part 3: `EntryViewState::fully_shown_tool_outputs`; `Markdown::has_active_search_highlight()`; clamp to N x the markdown line height with a bottom fade, "Show all M lines" / "Show less", skipped on the active search match. Tests: long output clamped and toggled by clicks, short output and setting 0 not clamped, active match unclamps (fails without the exception). Deviation: the shown-all state is tested through the UI clicks instead of a separate unit test.
- Part 4: `terminal_line_limit` rule (`EMBEDDED_TERMINAL_MAX_LINES = 1000`) used at terminal creation; `ThreadView::apply_terminal_preview_limits` on toggle and from a `ConversationView` settings observer; terminal "Show all" row counts lines with content (`used_lines`, not `total_lines`, which includes empty screen rows). Tests: `test_terminal_line_limit`; terminal shows 3 of 10 lines, Show all / Show less, follows a changed setting (fails without the observer), no toggle when the output fits. The last check does not distinguish `used_lines` from `total_lines` in the test setup (the embedded terminal is sized to its content); the choice rests on the definitions in `crates/terminal/src/alacritty.rs:810, 938`.
- Part 5: suites `markdown` 169, `agent` 761 (11 pre-existing ignored), `agent_ui` 484, `settings_ui` 57, `settings_content` 50+1 (1 pre-existing ignored), `agent_settings` 44, `terminal_view` 96; `./script/clippy` exit 0; rustfmt clean (layouts by hand); debug build passes.
- Part 6: command collapse measures the laid-out command (`on_children_prepainted`) and records it after the draw (`cx.defer`; a notify during a draw schedules no frame, `crates/gpui/src/window.rs:171`); code-block spacing counted only for fenced labels. `TerminalToolHeader.working_dir` is `Option`. Toggle is a full-width outlined `Button` with a chevron; terminal output gets a bottom fade. Tests: long command collapsed then expanded by click, fenced 2-line command not collapsed (fails without the fence check), permission-pending command never collapsed, terminal fade on/off. `agent_ui` 487, `markdown` 169, `acp_thread` 218; clippy clean; rustfmt clean.

## Open questions

None.
