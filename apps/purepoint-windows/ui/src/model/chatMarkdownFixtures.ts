// Replies rendered by the apps/shared/ChatMarkdown visual fixtures (RenderPreview.swift,
// DesktopChatPreview.swift via render-desktop-chat.py, PhonePreview.swift via
// render-phone-preview.py). Shared by the parser and view tests.

const renderPreview = [
  '## A clearer view of the work',
  "Here's the **release plan**, with `pu status` ready to run.",
  'This line stays on its own line.',
  '',
  '### Next steps',
  '1. Review the desktop changes',
  '   - [x] Preserve line breaks',
  '   - [ ] Check the phone layout',
  '2. Ship the polished chat',
  '',
  '> **Keep the conversation readable.**',
  '> Long replies should have a clear rhythm, even in narrow panes.',
  '',
  '| Platform | Rendering | Status |',
  '| :--- | :--- | ---: |',
  '| Desktop | Native Markdown | Ready |',
  '| iOS | Shared styles | Ready |',
  '',
  '```swift',
  'let conversation = PointGuard()',
  'conversation.send("Hello, Markdown")',
  '```',
  '',
  'Read the [project notes](https://example.com) for details.',
].join('\n');

const desktopChat = [
  '## A clearer view of the work',
  "Here's the **release plan**, with `pu status` ready to run.",
  'This line stays on its own line.',
  '',
  '1. Check the workspace:',
  '   ```sh',
  '   pu status --global --json',
  '   pu agents list --global --json',
  '   ```',
  '2. Review the results',
  '   - [x] Preserve line breaks',
  '   - [ ] Check the phone layout',
  '',
  '> **Keep the conversation readable.**',
  '> Long replies should have a clear rhythm.',
  '',
  '| Platform | Rendering | Status |',
  '| :--- | :--- | ---: |',
  '| Desktop | Native Markdown | Ready |',
  '| iOS | Shared styles | Ready |',
].join('\n');

const phoneCode = [
  '### Ready to run',
  '1. Check the workspace:',
  '   ```sh',
  '   pu status --global --json',
  '   pu agents list --global --json',
  '   ```',
  '',
  '| Platform | Rendering | Status |',
  '| :--- | :--- | ---: |',
  '| Desktop | Native Markdown | Ready |',
  '| iOS | Shared styles | Ready |',
].join('\n');

const phoneOverview = [
  '## A clearer view of the work',
  "Here's the **release plan**, with `pu status` ready to run.",
  'This line stays on its own line.',
  '',
  '### Next steps',
  '1. Review the desktop changes',
  '   - [x] Preserve line breaks',
  '   - [ ] Check the phone layout',
  '2. Ship the polished chat',
  '',
  '> **Keep the conversation readable.**',
  '> Long replies should have a clear rhythm.',
].join('\n');

const phoneLongReply =
  Array.from(
    { length: 12 },
    (_, index) =>
      `### Step ${String(index + 1)}\nRead the result, then check the next task.\n\n\`\`\`sh\npu status --global --json\n\`\`\``,
  ).join('\n\n') + '\n\nEnd of the long reply.';

const phoneAppendedSection =
  phoneOverview +
  '\n\n## New section\nThis arrived after the reply was laid out.\n\n```swift\nlet next = "Keep following the reply"\n```\n\nEnd of the appended section.';

export const chatMarkdownFixtures = {
  renderPreview,
  desktopChat,
  phoneOverview,
  phoneCode,
  phoneLongReply,
  phoneAppendedSection,
} as const;
