# Shared Point Guard Markdown

Both Xcode app targets explicitly include `ChatMarkdown.swift` and
`ChatMarkdownView.swift`. The other files in this directory are standalone
verification tools and are excluded from the apps. No package dependency or
WebView is required. Parsing is independent of networking and SwiftUI.

Run parser tests safely (temporary Swift package, no app build phases):

```sh
python3 apps/shared/ChatMarkdown/check-markdown.py
```

Generate native macOS light/dark and narrow-width visual fixtures safely:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library apps/shared/ChatMarkdown/ChatMarkdown.swift apps/shared/ChatMarkdown/ChatMarkdownView.swift apps/shared/ChatMarkdown/RenderPreview.swift -o /tmp/pointguard-markdown-preview
/tmp/pointguard-markdown-preview
```

These PNGs appear at `/tmp/pointguard-desktop-light.png`,
`/tmp/pointguard-desktop-dark.png`, and `/tmp/pointguard-phone-width.png`.
The narrow fixture uses macOS at phone width; it does not prove iOS runtime behavior.

The renderer supports chat-oriented block Markdown and Foundation inline Markdown,
not the full CommonMark specification. Nested list rows retain indentation;
continuation text remains with its item, and nested code/quotes get their own indented blocks. Fences support backticks/tildes, longer
closing delimiters and incomplete streamed blocks. Raw HTML is text. Links only
activate HTTP, HTTPS and mailto; standalone Markdown images load HTTP(S) URLs.
Tables and code scroll horizontally rather than widening the transcript.

Final acceptance requires building/running both apps in Xcode per AGENTS.md.
Inspect live replies and history in light/dark appearance, narrow split panes,
phone portrait/landscape and large Dynamic Type. Verify long code/table scrolling,
text selection, code copy feedback, image success/failure, streaming, and scrolling
up during a reply without losing your place. Standalone checks do not establish
complete macOS target compilation or full app interaction.

## Runtime fixtures

Run the actual desktop chat view/model in an isolated native host (only app chrome
is stubbed), generating light, dark and narrow-pane PNGs in `/tmp`:

```sh
python3 apps/shared/ChatMarkdown/render-desktop-chat.py
```

Run the actual phone chat view/model with demo snapshots on an already-booted
arm64 iOS simulator, using a separate `dev.purepoint.MarkdownVerification` bundle:

```sh
python3 apps/shared/ChatMarkdown/render-phone-preview.py
```

This uses standalone `swiftc`/asset compilation and bypasses all production app
build phases. Preview controls switch between overview and nested-code/table
fixtures, light/dark appearance and accessibility text sizing. The fixture never
connects to a bridge. It does not replace the installed PurePoint app. Desktop's
fixture uses a temporary home for recovery state; phone state lives in the separate
simulator bundle's sandbox. Neither fixture replaces daemon/CLI binaries.

Observed iOS checks (2026-10-11): native heading/list/task/quote layout and preserved
line breaks; indented multiline fenced code; Copy changes to Copied and the simulator
pasteboard contains both exact source lines; wide tables scroll to their final column;
dark appearance and accessibility2 text sizing retain readable content and controls.
These checks exercise the real views against local snapshots, not a live Pi session.
Production target build/runtime acceptance in Xcode remains required by AGENTS.md.

Observed desktop checks (2026-10-11): the actual `PiPointGuardChatView` renders
user bubbles plus headings, preserved lines, inline code, nested fenced panels,
task checks, quotes and tables in light/dark and a 400-point pane. The initial
spurious Latest button on a fully visible short transcript was corrected by
observing scroll geometry after asynchronous Markdown layout. Phone's equivalent
layout-following change was exercised with a long reply: its final paragraph stays
visible after parsing. Phone's fixture also offers Long reply and Append a section
for verifying updates while reading earlier content. The scroll-up/append check
remains unverified: Device Hub's window became unavailable during that interaction.
