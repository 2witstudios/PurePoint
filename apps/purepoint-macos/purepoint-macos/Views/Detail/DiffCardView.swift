import SwiftUI

/// Card displaying a single file's diff — header with metadata + AppKit diff content body.
struct DiffCardView: View {
    let fileDiff: FileDiff
    var onLoadPreview: (() -> Void)? = nil
    var loadPage: ((FileDiff, ReviewPageCursor?) async throws -> ReviewPage)? = nil
    var revealToken: UUID? = nil
    @State private var expanded: Bool
    @State private var history = ReviewPageHistory()
    @State private var pageIndex = 0
    @State private var loading = false
    @State private var loadError: String?
    @State private var retry = 0
    @State private var pagesVersion = ""
    @State private var loadToken = UUID()

    init(fileDiff: FileDiff, onLoadPreview: (() -> Void)? = nil,
         loadPage: ((FileDiff, ReviewPageCursor?) async throws -> ReviewPage)? = nil,
         revealToken: UUID? = nil) {
        self.fileDiff = fileDiff; self.onLoadPreview = onLoadPreview
        self.loadPage = loadPage; self.revealToken = revealToken
        _expanded = State(initialValue: revealToken != nil || !fileDiff.shouldCollapse)
    }

    private var page: ReviewPage? { pagesVersion == fileDiff.previewVersion ? history.page(at: pageIndex) : nil }
    private var usesPages: Bool { loadPage != nil && (fileDiff.patchArguments != nil || fileDiff.patchSlice != nil) }
    private var visibleHunks: [Hunk] { usesPages ? (page?.hunks ?? []) : fileDiff.pageHunks(offset: pageIndex * 200) }
    private var hasMore: Bool { usesPages ? (page?.hasMore ?? false) : fileDiff.hunks.reduce(0) { $0 + $1.lines.count } > (pageIndex + 1) * 200 }

    var body: some View {
        VStack(spacing: 0) {
            Button {
                expanded.toggle()
            } label: {
                HStack(spacing: 0) {
                    Image(systemName: expanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 10)).foregroundStyle(.secondary).padding(.leading, 10)
                    header
                }.contentShape(Rectangle())
            }.buttonStyle(.plain)
                .accessibilityLabel("\(expanded ? "Collapse" : "Expand") diff for \(fileDiff.filename)")
            if expanded {
                if let error = fileDiff.previewError ?? loadError {
                    Text(error).font(.system(size: 11)).foregroundStyle(.orange).textSelection(.enabled).padding(10)
                    if loadError != nil { Button("Retry") { retry += 1 }.padding(8) }
                }
                if loading { ProgressView().controlSize(.small).padding(10) }
                if fileDiff.previewDeferred { Button("Load preview") { onLoadPreview?() }.buttonStyle(.borderless).padding(10) }
                if fileDiff.statusCode == "U" {
                    Text("Unresolved merge conflict · \(fileDiff.conflictBaseline ?? "resolve this file before committing")")
                        .font(.system(size: 11)).foregroundStyle(.orange).padding(10)
                }
                if fileDiff.isBinary || page?.isBinary == true {
                    Text("Binary file changed").font(.system(size: 11)).foregroundStyle(.secondary).padding(10)
                }
                if let old = fileDiff.oldFilename {
                    Text("Renamed from \(old)").font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).padding(10)
                }
                if !visibleHunks.isEmpty { DiffContentRepresentable(hunks: visibleHunks).frame(minHeight: 20) }
                if page?.hasLongLines == true {
                    Text("Long lines are shortened in this preview. Open the full patch to read them.")
                        .font(.system(size: 11)).foregroundStyle(.secondary).padding(10)
                }
                if pageIndex > 0 || hasMore || page != nil {
                    HStack {
                        if pageIndex > 0 { Button("Previous lines") { pageIndex -= 1 }.disabled(loading) }
                        Text("Page \(pageIndex + 1)").font(.system(size: 11)).foregroundStyle(.secondary)
                        if hasMore { Button("More lines") { pageIndex += 1 }.disabled(loading) }
                        Spacer()
                        // The iPad shares the fetched patch instead of opening it locally (IPAD-7.2).
                        #if os(macOS)
                        if let page { Button("Open full patch") { NSWorkspace.shared.open(page.slice.snapshot.url) } }
                        #endif
                    }.buttonStyle(.borderless).padding(10)
                }
            }
        }
        .background(Color(nsColor: Theme.cardBackground))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color(nsColor: .separatorColor), lineWidth: 1))
        .onChange(of: expanded) { _, visible in if !visible { history.releaseBody() } }
        .onDisappear { history.releaseBody() }
        .onChange(of: revealToken) { _, token in if token != nil { expanded = true } }
        .onChange(of: fileDiff.previewVersion) { _, _ in history = ReviewPageHistory(); pageIndex = 0; loadError = nil }
        .task(id: fileDiff.previewVersion + "/\(expanded)/\(pageIndex)/\(retry)") {
            guard expanded, usesPages, page == nil, let loadPage else { return }
            if pagesVersion != fileDiff.previewVersion { history = ReviewPageHistory(); pagesVersion = fileDiff.previewVersion }
            let token = UUID(); loadToken = token
            loading = true; loadError = nil
            defer { if loadToken == token { loading = false } }
            do {
                let loaded = try await loadPage(fileDiff, history.cursor(at: pageIndex))
                guard !Task.isCancelled, loadToken == token else { return }
                history.store(loaded, at: pageIndex)
            } catch {
                guard !Task.isCancelled, loadToken == token else { return }
                loadError = error.localizedDescription
            }
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 6) {
            Image(systemName: statusIcon)
                .font(.system(size: 12))
                .foregroundColor(statusColor)

            filenameText
                .lineLimit(1)
                .truncationMode(.middle)

            statusBadge
                .layoutPriority(-1)

            Spacer(minLength: 4)

            statsText
        }
        .padding(.horizontal, 10)
        .frame(height: 32)
        .background(Color(nsColor: Theme.cardHeaderBackground))
    }

    private var filenameText: Text {
        let components = fileDiff.filename.split(separator: "/")
        if components.count > 1 {
            let dir = components.dropLast().joined(separator: "/") + "/"
            let file = String(components.last!)
            return Text(
                "\(Text(dir).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary))\(Text(file).font(.system(size: 12, weight: .semibold, design: .monospaced)))"
            )
        } else {
            return Text(fileDiff.filename).font(.system(size: 12, weight: .semibold, design: .monospaced))
        }
    }

    private var statusBadge: some View {
        Text(statusLabel)
            .font(.system(size: 10, weight: .medium))
            .foregroundColor(.white)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(statusColor.opacity(0.6))
            .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    private var statsText: some View {
        HStack(spacing: 4) {
            if fileDiff.added > 0 {
                Text("+\(fileDiff.added)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(Color(nsColor: Theme.additionText))
            }
            if fileDiff.removed > 0 {
                Text("-\(fileDiff.removed)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(Color(nsColor: Theme.deletionText))
            }
        }
    }

    // MARK: - Status Helpers

    private var statusIcon: String {
        switch fileDiff.statusCode {
        case "A": "doc.badge.plus"
        case "D": "doc.badge.minus"
        case "M": "doc.text"
        default: "doc.text"
        }
    }

    private var statusColor: Color {
        switch fileDiff.statusCode {
        case "A": .green
        case "D": .red
        case "M": .yellow
        default: .gray
        }
    }

    private var statusLabel: String {
        switch fileDiff.statusCode {
        case "U": "Conflict"
        case "M": "Modified"
        case "A": "Added"
        case "D": "Deleted"
        case "??": "Untracked"
        default: fileDiff.statusCode
        }
    }
}
