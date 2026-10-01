# Make the agent's reply stand out from everything around it

- Date: 2026-10-01 12:00
- Status: done (screenshot check pending M)
- Repos: zedd   Branch: zedd-claude-code-transcript (PR #3)
- Files: crates/agent_ui/src/conversation_view/thread_view.rs
- Subagents: none

## Context and problem

M's screenshot: folded tool lines, the subagent report line and the notification line are as bright as the reply, so the reply does not stand out. M wants the reply in full text color and everything else slightly grey.

## What is true today

- M's dark theme (VSCode Dark Modern) sets `text` and `text.muted` to the same `#CCCCCC` [verified: extension theme json], so everything drawn `Color::Muted` / `text_muted` is exactly as bright as the reply.
- Transcript chrome uses `Color::Muted` (tool run and member lines, injected notices, chevrons) or `text_muted` (thinking header, tool labels in `render_tool_call_label`, tool output in `render_markdown_output`) [verified: thread_view.rs].
- The thinking body is not dimmed at all [verified: render_thinking_block].
- Markdown inherits its container's text color (the thinking header and tool output already rely on it) [verified: thread_view.rs:7704, 11425].

## Decision

One transcript color, `secondary_text_color`: the theme's main text blended 70% over the panel background. Derived from `text` so it is dimmer than the reply in every theme, including those whose muted color equals their text color. The reply keeps the theme's own text color (no forced white, so light themes keep working). Applied only to transcript elements; the composer, plan and token usage keep the theme's muted color.

## Design

| Element | Today | After |
| --- | --- | --- |
| Folded run lines, member lines, chevrons | Muted | secondary |
| Subagent report line and body, notification line | Muted / text | secondary |
| Thinking header and body | Muted / text | secondary |
| Tool labels, tool output text | text_muted | secondary |
| Assistant reply | text | text (unchanged) |

## Acceptance criteria

- Given a theme whose muted color equals its text color, then the secondary color is still darker than the text color on a dark background (and lighter on a light one).
- Given the transcript, then only the reply uses the full text color among the elements above.

## Parts

### Part 1
- [x] `secondary_text_color` (pure blend over text and background, unit-tested) and its use at the sites above (done when: test passes; screenshot check by M)

### Part 2: Verify
- [x] `agent_ui` suite, clippy, rustfmt check, debug build

## Standardized review

- **Bottom line:** ready.
- Single source of truth: pass (one function). Simplicity: pass. Scope: pass (transcript only). Theme fidelity: note: the reply stays the theme's text color rather than forced white; offered to M as a follow-up if the contrast is not enough.

## Changes made

- `secondary_text_color` / `secondary_text_blend` (text at 70% over the panel background; about #969696 against #CCCCCC replies in M's theme) used for folded run and member lines, injected notice lines and the opened report, the thinking header and body, tool labels (card and non-card) and tool output. Test `test_secondary_text_sits_between_text_and_background` (dark theme with muted = text, and a light theme). `agent_ui` 503; clippy and rustfmt clean; debug build passes.

## Open questions

None.
