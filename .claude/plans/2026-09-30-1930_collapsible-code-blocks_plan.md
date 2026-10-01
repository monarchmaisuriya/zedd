# Let code blocks in agent messages start collapsed

- Date: 2026-09-30 19:30
- Status: all code and automated checks done; manual check pending M
- Repos: zedd
- Folders: crates/markdown, crates/agent_ui, crates/agent_settings, crates/settings_content, crates/settings_ui, crates/agent, assets/settings
- Files: crates/markdown/src/markdown.rs, crates/agent_ui/src/conversation_view/thread_view.rs, crates/agent_settings/src/agent_settings.rs, crates/settings_content/src/agent.rs, crates/settings_ui/src/page_data.rs, assets/settings/default.json, crates/agent/src/tool_permissions.rs (test literal), crates/agent_ui/src/agent_ui.rs (test literal)
- Subagents: none
- Source: upstream PR zed-industries/zed#64793 (open, unreviewed), addressing discussion zed-industries/zed#58333

## Context and problem

Long agent turns in the Agent Panel are hard to scan because fenced code blocks inside agent messages always render in full. Tool cards already start collapsed, and diff cards, terminal cards, and thinking blocks already have settings (`expand_edit_card`, `expand_terminal_card`, `thinking_display`). Code blocks are the one verbose block with no control.

## Decisions (M, 2026-09-30)

1. Scope A: code blocks in agent messages only. No tool-output preview mode, no umbrella switch.
2. Port upstream PR #64793 instead of writing a new design, so a later upstream merge syncs cleanly.
3. Keep upstream's default `agent.expand_code_block: true` (no behavior change unless the user sets `false`).

## Design (as in the PR)

- `markdown`: `MarkdownElement::collapse_code_blocks(bool)` option. When on, a code block the user has not opened renders as one clickable row (`<language or path> · N lines`, chevron). Open state lives on the `Markdown` entity in `expanded_code_blocks`, keyed by the block's source offset like `wrapped_code_blocks`, so it survives re-renders and streaming. An expanded block gets a collapse button in its hover toolbar. The block holding the active search match is never collapsed.
- Settings: `agent.expand_code_block` in `settings_content`, `agent_settings`, `default.json`, and the settings UI, next to `expand_terminal_card`.
- `agent_ui`: assistant message content passes `collapse_code_blocks = !expand_code_block`; tool output and compaction summaries pass `false`.

## Risks

| Risk | Mitigation | How noticed |
| --- | --- | --- |
| The one hunk that does not apply (`render_output_content_block` call near `thread_view.rs:10189`) is adapted wrongly | Adapt by hand to the same meaning (tool output passes `false`); compile and read the diff | `cargo check`, diff review |
| Other zedd code builds `AgentSettings` by hand and breaks | Rescan found only the two test literals the PR patches | compile of every touched crate |
| zedd drift from upstream elsewhere in `markdown.rs` | `patch --dry-run` first; stop if anything other than the known hunk fails | dry run output |

## Acceptance criteria

- With default settings, agent code blocks render exactly as before.
- With `"agent": { "expand_code_block": false }`, each code block in an agent message shows as one row with language and line count; clicking expands it; the hover chevron collapses it again.
- Code blocks inside tool cards are unaffected.
- The setting appears in the settings UI under the agent section next to "Expand Terminal Card".

## Parts

### Part 1: Port
- [x] Dry-run the patch with `patch -p1 --dry-run` (done when: only the known `thread_view.rs` hunk fails). Actual: BSD patch applied every hunk (it allows offsets); each `thread_view.rs` call site was checked by hand.
- [x] Apply the patch, adapt the rejected hunk by hand, remove patch leftovers (done when: `cargo check` passes for markdown, agent_settings, settings_content, settings_ui, agent, agent_ui).

### Part 2: Verify
- [x] Markdown tests from the PR pass, full suites for `markdown`, `agent`, `agent_ui`, `settings_ui` pass (done when: 0 failures).
- [x] `./script/clippy` on touched crates exits 0; rustfmt check clean on touched files; `cargo build -p zed` passes (done when: all pass).
- [ ] Manual check with the setting on and off (done when: M confirms).

## Standardized review

| Principle | Verdict | Evidence | Structural fix |
| --- | --- | --- | --- |
| Modularity & Cohesion | pass | Collapse lives in the markdown element; `agent_ui` only chooses whether to turn it on | - |
| Dependency Direction | pass | `agent_ui` depends on `markdown`'s published builder option; `markdown` knows nothing about agent settings | - |
| Single Source of Truth | pass | Open state per block lives only on the `Markdown` entity, keyed like `wrapped_code_blocks`; the setting has one definition in `settings_content` | - |
| Simplicity | pass | One boolean option and one set of opened offsets; no new modes | - |
| YAGNI | pass | Scope A only; preview mode and umbrella switch rejected by M | - |
| Explicit Boundaries & Contracts | pass | New `MarkdownElement::collapse_code_blocks` is the only surface other crates use | - |
| Composition & Reusability | pass | Any markdown view can opt in; reuses existing hover toolbar and icon buttons | - |
| Fail-Fast | n/a | No new input parsing; the setting is a typed bool with a default in `default.json` | - |
| Comments & Docstrings | pass | Setting and builder option documented; no plan labels | - |

In plain terms: the plan ports a small, self-contained upstream change that adds one off-by-default switch. The only manual work is one call site where zedd's file has drifted.

## Changes made

- Part 1: applied upstream PR zed-industries/zed#64793 with `patch -p1` (no git). Every hunk applied; the three `render_output_content_block` call sites were checked by hand (`thread_view.rs:4026` compaction `false`, `:7509` agent message = setting, `:10307` tool card `false`). Removed 6 `.orig` backups written by patch.
- Pricing leftovers found because `settings_ui` tests had not compiled since the pricing removal (outside this plan's scope, fixed under "finish all"): deleted the `llm_providers_page.rs` test module, which only tested the deleted Zed AI young-account provider row (`cloud::test_support::young_account_configuration`); removed the deleted web-search tool from the Tool Permissions settings page (`TOOLS` entry, page match arm, `render_web_search_tool_config`, its re-export in `pages.rs`). Kept the `web_search` settings migration and the ACP session-option test fixtures, which are unrelated.
- Part 2: PR markdown tests 3/3 pass; suites `markdown` 168, `agent` 760 (11 pre-existing ignored), `agent_ui` 474, `settings_ui` 57, `agent_settings` 44, `settings_content` 50+1 (1 pre-existing ignored); `./script/clippy` on the six crates exit 0; rustfmt check clean; `cargo build -p zed` passes.
