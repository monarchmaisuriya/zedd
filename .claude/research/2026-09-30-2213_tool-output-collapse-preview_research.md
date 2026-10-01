# Collapse code in tool output, and preview long tool and terminal output

- Date: 2026-09-30 22:13
- Status: presented
- Mode: explore options
- Repos: zedd (branch `zedd-fix-code-preview-tool`, from `main` at `58cec871fc`)   Folders: crates/agent_ui, crates/markdown, crates/terminal_view, crates/settings_content, crates/agent_settings, crates/settings_ui, assets/settings
- Question / problem: How should zedd let users (A) collapse code blocks inside tool output and (B) see only the first few lines of long tool and terminal output, with a way to see all of it?

## Options: Show the first N lines of an open tool or terminal output, and let code blocks inside tool output collapse like code in replies, without changing today's defaults

- **Bottom line:** Add one number setting, `agent.tool_output_preview_lines` (unset by default, so nothing changes), that limits open tool and terminal output to its first N lines with a "Show all M lines" toggle. Use the terminal's existing line-limit hook for terminals and a line-height clamp for markdown output. Also pass the existing `expand_code_block` flag through to markdown inside tool cards. This reuses three mechanisms already in the code and leaves auto-open to the existing `expand_*` settings.
- **Where we are:** framing done; codebase swept (verified by spot checks); 8 external sources read, 3 primary. Next step is `/plan` after M picks.
- **Need from M:** pick an option, and answer the head-or-tail question for terminal previews (Open question 1).

### The problem, framed

- **What:** Long tool and terminal output pushes the conversation apart; code blocks inside tool output cannot be collapsed at all.
- **Why:** M wants the Agent Panel to be scannable, as asked in [zed-industries/zed#58333](https://github.com/zed-industries/zed/discussions/58333) (tool-call, command-output, and code blocks should be able to start collapsed, with identifying headers).
- **Who:** anyone reading agent threads in zedd, with the native agent or ACP agents (Claude Code, OpenCode).
- **Whom:** M first; the setting defaults keep upstream behavior for everyone else.
- **When:** no deadline; applies to every rendered thread, live and reloaded.
- **Where:** the Agent Panel's tool cards and terminal cards (`crates/agent_ui/src/conversation_view/thread_view.rs`), their state (`crates/agent_ui/src/entry_view_state.rs`), the markdown renderer (`crates/markdown`), and the embedded terminal (`crates/terminal_view`).
- **Which:** reuse `expand_code_block` (markdown collapse), the terminal's `max_lines_when_unfocused`, and the thinking block's preview clamp.
- **How:** see the recommended flow below.
- **How much:** medium; about 6 to 8 files, one new setting, no data or protocol change; UI only.

Refocused problem: open tool output has no size limit and markdown in tool cards ignores the code-block collapse flag; add a limit that defaults off and route the existing flag through.

### What is true today (verified)

- Tool cards start collapsed; only diffs and terminals auto-open, each behind a setting (`expand_edit_card`, `expand_terminal_card`), plus cards awaiting permission [verified: `thread_view.rs:1289-1302`, `entry_view_state.rs:81-83`].
- Open tool output has no height limit, except the floating permission row (`max_h_40`) and images (`max_w_96`/`max_h_96`) [verified: `thread_view.rs` around 8836-8843 and the image path, per the codebase sweep].
- Markdown in tool cards never receives the code-block collapse flag: `render_output_content_block` applies `.collapse_code_blocks(...)` only when there is no tool call; with a tool call it goes to `render_markdown_output`, which calls `self.render_markdown(...)` without it [verified: `thread_view.rs`, `render_output_content_block` else-branch and `render_markdown_output`]. Changing the `false` at the tool-card call site alone would do nothing.
- `read_file` output uses a custom numbered-lines renderer, not a `MarkdownElement`, so markdown collapse cannot reach it [verified: `render_markdown_output` calls `render_numbered_read_file_output`].
- Terminal output renders inline at full height up to 1000 lines, then becomes an `h_72` scroll box [verified: `terminal_view.rs:378` `MAX_EMBEDDED_LINES`, `:383` `content_mode`; `entry_view_state.rs:743` `set_embedded_mode(Some(1000))`]. `TerminalMode::Embedded { max_lines_when_unfocused }` already limits displayed lines and reports `Inline { displayed_lines, total_lines }` [verified: `terminal_view.rs:176, 324`].
- Thinking blocks already have a preview: `ThinkingBlockDisplay::{Auto, Preview, AlwaysExpanded, AlwaysCollapsed}` and a `max_h_64` clamp with a gradient overlay while constrained [verified: `settings_content/src/agent.rs` enum; `thread_view.rs:7694-7713`].
- Markdown has no line or height limit of its own [inferred from the codebase sweep: `MarkdownStyle` has no max-lines field].
- Collapsed tool content is excluded from thread search [unverified here, from the sweep: `thread_search_bar.rs:374, 973`]; a preview clamp keeps content rendered, so a search match can sit in the hidden part.

### How others do it

- **Claude Code (CLI):** Bash output collapses to a few lines with "+N lines (ctrl+o to expand)"; Ctrl+O opens a transcript viewer that "expands lines that collapse by default" [verified, primary: [Claude Code interactive mode docs](https://code.claude.com/docs/en/interactive-mode)]. The shown count is 3 to 4 lines and not configurable [unverified, tertiary: [issue #12589](https://github.com/anthropics/claude-code/issues/12589)]. An open request proposes `toolOutputDisplay: "collapsed" | "preview" | "verbose"` with `toolOutputPreviewLines` [verified, tertiary: [issue #96962](https://github.com/anthropics/claude-code/issues/96962)].
- **Codex CLI:** folds exec output to head and tail lines with "… +N lines"; the full text is in a transcript overlay (Ctrl+T); the limit is hard-coded [verified, primary: [openai/codex#17076](https://github.com/openai/codex/pull/17076); tertiary: [issue #4550](https://github.com/openai/codex/issues/4550)].
- **VS Code (Copilot agent host):** the chat terminal card keeps "a truncated preview" and opens the full output in a read-only editor [verified, maintainer test plan: [microsoft/vscode#338053](https://github.com/microsoft/vscode/issues/338053)].
- **Zed upstream:** code-block collapse in replies is proposed in [zed-industries/zed#64793](https://github.com/zed-industries/zed/pull/64793) (open, unreviewed); zedd already carries it.
- Pattern across all four: short preview by default in the most-used surfaces, an explicit "N more" marker, and a one-step way to see everything. Only Claude Code users ask for the count to be configurable; none of the tools make it configurable today.

### What must be true

- Hard constraint: defaults must not change upstream behavior (keeps zedd close to upstream and matches the code-block decision) [verified: M's earlier decision to keep `expand_code_block` default `true`].
- Hard constraint: the full output stays one click away and remains copyable.
- Hard constraint: a search match inside the clamped part must become visible (same rule as collapsed code blocks) [verified precedent: `markdown.rs` keeps the block with the active match open].
- Convention: settings live next to `expand_*` in `settings_content/src/agent.rs`, `agent_settings.rs`, `default.json`, and the settings UI page [verified].
- Assumption to check: a line-height clamp on markdown output lines up with "N lines" closely enough (markdown lines vary in height). Check: render a long tool output at N=5 in the app.

### Options

| Aspect | 1: preview line count + route the flag (recommended) | 2: tri-state `tool_output_display` like `thinking_display` | 3: fixed-height clamp only | 4: head and tail preview (Codex style) |
| --- | --- | --- | --- | --- |
| How it works | New `agent.tool_output_preview_lines: Option<u32>` (unset = unlimited). Open terminal output uses `max_lines_when_unfocused = N`; open markdown output is clamped to N line heights with a gradient; a "Show all M lines" / "Show less" toggle per card. Separately, `render_markdown_output` passes `!expand_code_block` to the markdown element | New enum `auto | preview | always_expanded | always_collapsed` plus preview lines; controls both whether cards open and how much shows | Reuse the thinking block's `max_h_64` + gradient for all open tool output; no setting, or a single on/off | Show the first K and last K lines with a "… N lines" row between; needs splitting output into two views |
| Fit with our code | Reuses three existing mechanisms; one new setting; open-or-closed stays with the `expand_*` settings | Overlaps `expand_terminal_card` and `expand_edit_card`: two settings decide the same open state (two sources of truth) | Smallest; reuses one pattern | Terminal view renders one grid; head and tail needs a second view or custom rendering |
| Cost / blast radius | Medium: 6 to 8 files, UI only | Medium+: plus migration or precedence rules against `expand_*` | Small | Large for terminals |
| Fails when | Markdown line heights vary, so "N lines" is approximate for markdown (exact for terminals) | Users set both settings and get surprising precedence | Users want a count; long outputs that matter at the end (build errors) stay hidden | Complexity; head-only tools behave differently from terminals |
| Evidence | Terminal hook at `terminal_view.rs:176, 324`; clamp precedent `thread_view.rs:7702`; flag gap in `render_markdown_output`; requested setting shape in claude-code#96962 | `ThinkingBlockDisplay` precedent | thinking preview precedent | Codex PR #17076 |
| Completeness | 8/10 | 8/10 (with SSOT cost) | 5/10 | 9/10 for terminals, 6/10 overall |

### Flow of the recommended option

```
tool card opens (by user, or expand_terminal_card / expand_edit_card as today)
   |
   |- terminal output --> TerminalView embedded max_lines_when_unfocused = N   <-- new value
   |                        more lines? -> "Show all M lines" row              <-- new
   |
   |- markdown output --> clamp to N line heights + gradient                   <-- new
   |                        code blocks inside honor expand_code_block        <-- flag routed (A)
   |                        active search match inside -> unclamped
   |
   '- diffs, images      --> unchanged
toggle "Show all" / "Show less" --> per-card state in EntryViewState           <-- new
```

### Open questions

1. Terminal preview shows the **first** N lines (Claude Code, VS Code) or the **last** N (where build errors and test summaries usually are)? `max_lines_when_unfocused` shows the first lines today; the last N needs a scroll-to-bottom on the embedded grid. Recommendation: first N, matching the other tools and the existing hook; revisit if it hides errors in practice.
2. `read_file`'s numbered output: include it in the preview clamp (yes, it is plain output) and leave code-block collapse off for it (it is not markdown).
3. Default for `tool_output_preview_lines`: unset (no change). A value like 8 is a sensible personal setting.

### Recommendation

RECOMMENDATION: Option 1 because it answers both asks with one default-off setting, reuses the terminal line limit, the thinking-block clamp, and the code-block collapse flag already in the code, and leaves open-or-closed to the existing `expand_*` settings instead of creating a second source of truth.

### Sources

- [Allow ACP/tool/code transcript blocks to be collapsed by default (zed-industries/zed discussion #58333)](https://github.com/zed-industries/zed/discussions/58333)
- [agent: Add `expand_code_block` setting (zed-industries/zed#64793)](https://github.com/zed-industries/zed/pull/64793)
- [Claude Code docs: Interactive mode](https://code.claude.com/docs/en/interactive-mode)
- [Claude Code issue #96962: middle display level with preview lines](https://github.com/anthropics/claude-code/issues/96962)
- [Claude Code issue #12589: configurable output collapse threshold](https://github.com/anthropics/claude-code/issues/12589)
- [openai/codex#17076: ctrl+t hint on truncated exec output](https://github.com/openai/codex/pull/17076)
- [openai/codex issue #4550: make output truncation configurable](https://github.com/openai/codex/issues/4550)
- [microsoft/vscode#338053: full output for large shell results (test plan)](https://github.com/microsoft/vscode/issues/338053)
- zedd source: `crates/agent_ui/src/conversation_view/thread_view.rs`, `crates/agent_ui/src/entry_view_state.rs`, `crates/terminal_view/src/terminal_view.rs`, `crates/markdown/src/markdown.rs`, `crates/settings_content/src/agent.rs`

## Standardized review

### Review: research: collapse code in tool output, and preview long tool and terminal output

- **Bottom line:** present as is.
- **Findings:** 0 blocking, 1 must-verify, 1 note.

1. MUST-VERIFY: "N lines" is approximate for markdown output. A line-height clamp matches exact line counts only for uniform lines; headings and wrapped lines differ. Fix: the plan's manual check at N=5, and wording the toggle as "Show all" rather than promising an exact count for markdown. Need from M: nothing.
2. NOTE: search interaction is taken from the codebase sweep, not re-read here (`thread_search_bar.rs:374, 973`). The plan must re-read it before building. Need from M: nothing.

Criterion coverage:
- Question and decision fit: pass. Question, decision, and constraints stated in the framing.
- Method: pass. Codebase sweep with spot checks of the three load-bearing claims, then web sources.
- Source quality: pass. 3 primary (Claude Code docs, codex PR, VS Code maintainer test plan) plus the zedd source; tertiary issues used only for user demand and proposed setting shapes.
- Coverage and alternatives: pass. Four options including the status quo-adjacent fixed clamp and the Codex head-and-tail model.
- Claim traceability: pass. Each claim tagged and linked or cited to a file.
- Evidence handling: pass. The unverified search claim and the approximate-lines limit are labeled.
- Analysis quality: pass. Options compared on the same rows.
- Codebase fit: pass. Integration points named with file paths.
- Uncertainty and limits: violation. Markdown line approximation is not yet measured. Fix: Finding 1.
- Recommendation proportionality: pass. Default-off, reuse-first, with conditions to revisit (Open question 1).
- Reproducibility: pass. Sources and file paths listed.
