import SwiftUI
#if os(iOS)
import UIKit
#else
import AppKit
#endif

/// Native, selectable Markdown with the same hierarchy on phone and desktop.
struct ChatMarkdownView: View, Equatable {
    let text: String
    @State private var document: Document?
    private struct Document { let blocks: [ChatMarkdownBlock]; let inlineContent: [String: AttributedString] }
    nonisolated static func == (lhs: Self, rhs: Self) -> Bool { lhs.text == rhs.text }
    var body: some View {
        Group {
            if let document { blocksView(document.blocks) }
            else { selectable(text) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .task(id: text) {
            let source = text
            let parser = Task.detached(priority: .userInitiated) { ChatMarkdown.document(source) }
            let prepared = await withTaskCancellationHandler {
                await parser.value
            } onCancel: {
                parser.cancel()
            }
            guard !Task.isCancelled else { return }
            document = Document(blocks: prepared.blocks, inlineContent: prepared.inlineContent.mapValues(styledInline))
        }
    }
    private func blocksView(_ blocks: [ChatMarkdownBlock]) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in blockView(block) }
        }
    }
    private func blockView(_ block: ChatMarkdownBlock) -> AnyView {
        switch block {
        case .image(let alt, let url):
            return AnyView(VStack(alignment: .leading, spacing: 8) {
                AsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image): image.resizable().scaledToFit().frame(maxHeight: 420).accessibilityLabel(alt.isEmpty ? "Image" : alt)
                    case .failure: Label("Image unavailable", systemImage: "photo").font(.callout).foregroundStyle(.secondary)
                    default: ProgressView().frame(height: 80).frame(maxWidth: .infinity)
                    }
                }.clipShape(RoundedRectangle(cornerRadius: 12))
                if !alt.isEmpty { Text(alt).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                Link("Open image", destination: url).font(.caption)
            })
        case .paragraph(let text): return AnyView(selectable(text))
        case .heading(let level, let text):
            return AnyView(Text(inline(text)).font(headingFont(level)).textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 6)
                .accessibilityAddTraits(.isHeader))
        case .listItem(let depth, let marker, let checked, let text):
            return AnyView(HStack(alignment: .firstTextBaseline, spacing: 10) {
                Group {
                    if let checked { Image(systemName: checked ? "checkmark.square.fill" : "square").accessibilityLabel(checked ? "Completed" : "Unchecked") }
                    else { Text(marker).monospacedDigit().accessibilityHidden(true) }
                }.font(bodyFont).foregroundStyle(.secondary).frame(minWidth: 20, alignment: .trailing)
                selectable(text)
            }.padding(.leading, CGFloat(depth) * 18))
        case .listContinuation(let depth, let blocks):
            return AnyView(blocksView(blocks).padding(.leading, CGFloat(depth) * 18 + 12))
        case .quote(let blocks):
            return AnyView(HStack(alignment: .top, spacing: 14) {
                RoundedRectangle(cornerRadius: 2).fill(Color.secondary.opacity(0.35)).frame(width: 3)
                blocksView(blocks).foregroundStyle(.secondary).padding(.vertical, 4)
            }.fixedSize(horizontal: false, vertical: true))
        case .code(let language, let text): return AnyView(ChatMarkdownCodeView(language: language, code: text))
        case .rule: return AnyView(Divider().padding(.vertical, 6))
        case .table(let headers, let alignments, let rows):
            return AnyView(ScrollView(.horizontal) {
                Grid(horizontalSpacing: 0, verticalSpacing: 0) {
                    tableRow(headers, alignments: alignments, header: true)
                    ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                        tableRow(row, alignments: alignments, header: false)
                            .background(Color.primary.opacity(index.isMultiple(of: 2) ? 0.025 : 0), in: Rectangle())
                    }
                }.fixedSize(horizontal: true, vertical: true)
                    .background(Color.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10))
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                    .overlay { RoundedRectangle(cornerRadius: 10).stroke(Color.primary.opacity(0.1), lineWidth: 1) }
            })
        }
    }
    private func tableRow(_ cells: [String], alignments: [ChatTableAlignment], header: Bool) -> some View {
        GridRow {
            ForEach(Array(cells.enumerated()), id: \.offset) { index, cell in
                Text(inline(cell)).font(bodyFont).fontWeight(header ? .semibold : .regular)
                    .textSelection(.enabled).frame(minWidth: 100, maxWidth: 280, alignment: alignment(alignments[index]))
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .background(Color.primary.opacity(header ? 0.055 : 0), in: Rectangle())
                    .overlay(alignment: .bottom) { Rectangle().fill(Color.primary.opacity(0.08)).frame(height: 0.5) }
                    .gridColumnAlignment(horizontalAlignment(alignments[index]))
            }
        }
    }
    private func alignment(_ value: ChatTableAlignment) -> Alignment { switch value { case .leading: .leading; case .center: .center; case .trailing: .trailing } }
    private func horizontalAlignment(_ value: ChatTableAlignment) -> HorizontalAlignment { switch value { case .leading: .leading; case .center: .center; case .trailing: .trailing } }
    private func selectable(_ text: String) -> some View {
        Text(inline(text)).font(bodyFont).lineSpacing(5).textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading).fixedSize(horizontal: false, vertical: true)
    }
    private var bodyFont: Font {
        #if os(iOS)
        .body
        #else
        .system(size: 15)
        #endif
    }
    private func headingFont(_ level: Int) -> Font {
        switch level { case 1: .title2.weight(.bold); case 2: .title3.weight(.semibold); default: .headline }
    }
    private func inline(_ text: String) -> AttributedString {
        document?.inlineContent[text] ?? AttributedString(text)
    }
    private func styledInline(_ content: AttributedString) -> AttributedString {
        var result = content
        for run in result.runs {
            if run[AttributeScopes.FoundationAttributes.InlinePresentationIntentAttribute.self]?.contains(.code) == true {
                result[run.range][AttributeScopes.SwiftUIAttributes.FontAttribute.self] = .system(.callout, design: .monospaced)
                result[run.range][AttributeScopes.SwiftUIAttributes.BackgroundColorAttribute.self] = Color.primary.opacity(0.075)
            }
            if let url = run[AttributeScopes.FoundationAttributes.LinkAttribute.self], !["http", "https", "mailto"].contains(url.scheme?.lowercased() ?? "") { result[run.range][AttributeScopes.FoundationAttributes.LinkAttribute.self] = nil }
        }
        return result
    }
}

struct ChatMarkdownCodeView: View {
    let language: String
    let code: String
    @State private var copied = false
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(language.isEmpty ? "Code" : language).font(.caption.weight(.medium)).foregroundStyle(.secondary)
                Spacer()
                Button {
                    #if os(iOS)
                    UIPasteboard.general.string = code
                    UIAccessibility.post(notification: .announcement, argument: "Code copied")
                    #else
                    NSPasteboard.general.clearContents(); NSPasteboard.general.setString(code, forType: .string)
                    #endif
                    copied = true
                } label: { Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc").font(.caption) }
                .buttonStyle(.plain).accessibilityLabel(copied ? "Code copied" : "Copy code")
                #if os(iOS)
                .frame(minWidth: 44, minHeight: 44)
                #endif
            }.padding(.horizontal, 14).padding(.vertical, 8)
            Divider().opacity(0.5)
            ScrollView(.horizontal) {
                Text(code).font(.system(.callout, design: .monospaced)).lineSpacing(4)
                    .textSelection(.enabled).fixedSize(horizontal: true, vertical: true)
                    .padding(14).frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .background(Color.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 12))
        .overlay { RoundedRectangle(cornerRadius: 12).stroke(Color.primary.opacity(0.09), lineWidth: 1) }
        .onChange(of: code) { _, _ in copied = false }
    }
}
