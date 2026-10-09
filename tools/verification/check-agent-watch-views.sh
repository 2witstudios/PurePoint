#!/bin/sh
# Checks actual channel/review/file-editor components against actual producers, with
# fixtures only for app-wide state/routing, terminal split-axis and syntax-library facade.
# Does not build the application, execute its build phases or install any binary.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
src="$repo/apps/purepoint-macos/purepoint-macos"
set --
for name in ChannelModel ManifestModel WorkspaceModel AgentStatus DiffModel PRModel EditorTab FileNode; do set -- "$@" "$src/Models/$name.swift"; done
for name in DaemonProtocol DaemonClient DaemonConnection GitService WorktreeWatcher FileIOService FileTreeWatcher; do set -- "$@" "$src/Services/$name.swift"; done
for name in ChannelState DiffState FileTreeState EditorState; do set -- "$@" "$src/State/$name.swift"; done
for path in "$src"/Views/Channel/*.swift "$src"/Views/Editor/*.swift; do set -- "$@" "$path"; done
for name in ReviewChangesView ProjectDetailView RootCheckoutDetailView WorktreeDetailView DiffListView DiffCardView DiffContentNSView PRRowView; do set -- "$@" "$src/Views/Detail/$name.swift"; done
for path in "$src/Views/PaneGrid/DraggableSplit.swift" "$src/Theme/Theme.swift" "$src/Theme/EditorTheme.swift"; do set -- "$@" "$path"; done
platform=$(xcrun --show-sdk-platform-path 2>/dev/null || true)
if [ -z "$platform" ]; then platform=/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform; fi
xcrun swiftc -typecheck -swift-version 5 -warnings-as-errors -default-isolation MainActor \
  -enable-upcoming-feature NonisolatedNonsendingByDefault \
  -plugin-path "$platform/Developer/usr/lib/swift/host/plugins" \
  "$@" "$repo/tools/verification/agent-watch-view-fixtures.swift"
printf '%s\n' 'PASS native channel, review and editable-file components with actual wire/Git producers (isolated app-wide dependency fixtures)'
