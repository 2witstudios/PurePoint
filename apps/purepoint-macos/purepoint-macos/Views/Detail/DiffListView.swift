import SwiftUI

/// Metadata navigation stays complete while only visible, expanded bodies load.
struct DiffListView: View {
    let diff: DiffData?
    let isLoading: Bool
    let emptyMessage: String
    let error: String?
    var onRetry: (() -> Void)?
    var groups: [DiffFileGroup]? = nil
    var loadPage: (@MainActor (FileDiff, ReviewPageCursor?) async throws -> ReviewPage)? = nil
    @AppStorage("showReviewFilesSidebar") private var showSidebar = true
    @State private var search = ""
    @State private var selectedFile: String?
    @State private var revealToken = UUID()

    private var entries: [ReviewFileEntry] {
        (groups ?? [DiffFileGroup(title: "", files: diff?.files ?? [])]).flatMap { group in
            group.files.enumerated().map { ReviewFileEntry(file: $0.element, group: group.title, startsGroup: $0.offset == 0) }
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("\(entries.count) changed files").font(.system(size: 11)).foregroundStyle(.secondary)
                Spacer()
                Button { showSidebar.toggle() } label: { Image(systemName: "sidebar.right") }
                    .buttonStyle(.borderless)
                    .help(showSidebar ? "Hide changed files" : "Show changed files")
                    .accessibilityLabel(showSidebar ? "Hide changed files" : "Show changed files")
            }.padding(.horizontal, 16).frame(height: PaneTabBar.height)
            Divider()
            HSplitView {
                middle.frame(minWidth: 160, maxWidth: .infinity, maxHeight: .infinity).layoutPriority(1)
                if showSidebar {
                    navigator.frame(minWidth: 240, idealWidth: 300, maxWidth: 640, maxHeight: .infinity)
                }
            }
        }
    }

    @ViewBuilder private var middle: some View {
        if isLoading && diff == nil && entries.isEmpty {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if entries.isEmpty {
            VStack(spacing: 12) {
                Image(systemName: error == nil ? "checkmark.circle" : "exclamationmark.triangle")
                    .font(.system(size: 28)).foregroundStyle(.secondary)
                Text(error ?? emptyMessage).font(.system(size: 12)).foregroundStyle(.secondary).textSelection(.enabled)
                if error != nil, let onRetry { Button("Retry", action: onRetry) }
            }.padding(16).frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        if let error {
                            Text(error).font(.system(size: 12)).foregroundStyle(.orange).textSelection(.enabled)
                            if let onRetry { Button("Retry", action: onRetry) }
                        }
                        ForEach(entries) { entry in
                            if !entry.group.isEmpty && entry.startsGroup {
                                Text(entry.group).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
                            }
                            DiffCardView(fileDiff: entry.file, loadPage: loadPage,
                                         revealToken: selectedFile == entry.id ? revealToken : nil)
                                .id(entry.id)
                        }
                    }.padding(16)
                }
                .onChange(of: revealToken) { _, _ in
                    if let selectedFile { proxy.scrollTo(selectedFile, anchor: .top) }
                }
            }
        }
    }

    private var navigator: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Files").font(.system(size: 11, weight: .medium))
                Text("\(entries.count)").font(.system(size: 11)).foregroundStyle(.tertiary)
                Spacer()
            }.padding(.horizontal, 10).frame(height: PaneTabBar.height)
            Divider()
            TextField("Find changed file", text: $search).textFieldStyle(.roundedBorder).padding(10)
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(entries.filter { search.isEmpty || $0.file.filename.localizedCaseInsensitiveContains(search) }) { entry in
                        Button {
                            selectedFile = entry.id
                            revealToken = UUID()
                        } label: {
                            HStack(spacing: 7) {
                                Image(systemName: "doc.text").foregroundStyle(.secondary)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text((entry.file.filename as NSString).lastPathComponent).font(.system(size: 11)).lineLimit(1)
                                    Text(entry.group.isEmpty ? (entry.file.filename as NSString).deletingLastPathComponent : entry.group + " · " + (entry.file.filename as NSString).deletingLastPathComponent)
                                        .font(.system(size: 9)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                                }
                                Spacer(minLength: 4)
                                Text("+\(entry.file.added)").foregroundStyle(Color(nsColor: Theme.additionText))
                                Text("−\(entry.file.removed)").foregroundStyle(Color(nsColor: Theme.deletionText))
                            }.font(.system(size: 10, design: .monospaced)).padding(.horizontal, 10).padding(.vertical, 7)
                                .frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
                                .background(selectedFile == entry.id ? Color.accentColor.opacity(0.12) : Color.clear)
                        }.buttonStyle(.plain).help(entry.file.filename)
                            .accessibilityLabel("Show \(entry.group) diff for \(entry.file.filename)")
                        Divider()
                    }
                }
            }
        }.background(Color(nsColor: Theme.cardBackground))
    }
}
