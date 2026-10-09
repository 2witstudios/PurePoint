import SwiftUI

struct ConnectionView: View {
    @ObservedObject var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var endpoint = ""
    @State private var secret = ""
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Your Mac.\nYour Pi.").font(.system(.largeTitle, design: .rounded).weight(.semibold))
                        Text("Pi runs on your Mac. This phone connects over your existing Tailscale network, so work continues when you close the app.").font(.body).foregroundStyle(.secondary)
                    }.padding(.vertical, 16)
                }
                Section {
                    TextField("ws://100.100.1.2:8787/v1", text: $endpoint).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL).accessibilityLabel("Mac endpoint")
                    SecureField("Pairing secret", text: $secret).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityLabel("Pairing secret")
                    Button("Connect") { model.pair(endpoint: endpoint, secret: secret) }
                } header: { Text("Connect to your Mac") } footer: { Text("Use the address printed by the Mac bridge and the secret you created there. The secret is stored only in this device’s Keychain. Keep both devices connected to Tailscale.") }
                if let error = model.error { Section { Text(error).foregroundStyle(.red).textSelection(.enabled) } }
                Section {
                    HStack { Text(model.connectionStatus); Spacer(); if model.connected && !model.demo { Image(systemName: "checkmark.circle.fill").foregroundStyle(.green) } }
                    if model.connected { Button("Disconnect", role: .destructive) { model.disconnect() } }
                    Button("Reconnect") { model.connect() }.disabled(endpoint.isEmpty)
                }
                Section { Button("Explore on-device preview") { model.exploreDemo(); dismiss() } } header: { Text("Try the interface") } footer: { Text("For deterministic full-flow testing, run the fixture bridge on your Mac. In the simulator use ws://127.0.0.1:8787/v1. Setup instructions are in the standalone project’s README.") }
            }
            .navigationTitle("Connection").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } } }
            .onAppear { endpoint = model.endpoint; secret = PairingSecret.read(endpoint: endpoint) }
            .onChange(of: model.connected) { _, connected in if connected && !model.demo { dismiss() } }
        }
    }
}
struct ConversationView: View {
    @ObservedObject var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var confirmResume = false
    @State private var search = ""
    var body: some View {
        NavigationStack {
            Group {
                if let history = model.browsing {
                    ScrollView { LazyVStack(alignment: .leading, spacing: 24) { ForEach(history.messages) { MessageView(message: $0) } }.padding(20) }
                        .safeAreaInset(edge: .bottom) {
                            Button(model.busy ? "Stop and resume this conversation" : "Resume this conversation") {
                                if model.busy { confirmResume = true } else { Task { await model.changeSession(to: history.sessionId); if model.browsing == nil { dismiss() } } }
                            }.buttonStyle(.borderedProminent).disabled(!model.connected || model.demo || model.changingSession).padding().frame(maxWidth: .infinity).background(.bar)
                        }
                } else {
                    List {
                        if model.conversations.isEmpty { ContentUnavailableView("No conversations yet", systemImage: "bubble.left.and.bubble.right", description: Text("Your Pi conversations will appear here. You can browse them while Pi works.")) }
                        ForEach(model.conversations.filter { search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) }) { conversation in
                            Button { Task { await model.browse(conversation) } } label: {
                                VStack(alignment: .leading, spacing: 6) { Text(conversation.title).foregroundStyle(.primary).lineLimit(2); if let date = conversation.date { Text(String(date.prefix(10))).font(.caption).foregroundStyle(.secondary) } }.padding(.vertical, 6)
                            }
                        }
                    }.searchable(text: $search, prompt: "Find a conversation").refreshable { await model.loadConversations() }
                }
            }
            .safeAreaInset(edge: .top) { if let error = model.error { Text(error).font(.footnote).foregroundStyle(.red).padding(12).frame(maxWidth: .infinity).background(.bar) } }
            .navigationTitle(model.browsing?.title ?? "Conversations").navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { if model.browsing != nil { Button("Back") { model.browsing = nil } } }
                ToolbarItem(placement: .topBarTrailing) { Button("Done") { model.browsing = nil; dismiss() } }
            }
            .confirmationDialog("Stop Pi and resume this conversation?", isPresented: $confirmResume, titleVisibility: .visible) {
                Button("Stop and resume", role: .destructive) { if let history = model.browsing { Task { await model.changeSession(to: history.sessionId, stopFirst: true); if model.browsing == nil { dismiss() } } } }
                Button("Keep working", role: .cancel) {}
            } message: { Text("Queued messages will be recoverable. Delegated workers keep running.") }
        }
    }
}
struct ExtensionDialogView: View {
    @ObservedObject var model: ChatModel
    let dialog: ExtensionDialog
    @State private var text = ""
    @State private var answering = false
    var body: some View {
        NavigationStack {
            Form {
                if let message = dialog.message, !message.isEmpty { Section { Text(message).textSelection(.enabled) } }
                if dialog.method == "select" {
                    Section { ForEach(Array((dialog.options ?? []).enumerated()), id: \.offset) { _, option in Button(option) { answer(value: option) }.disabled(answering) } }
                } else if dialog.method == "confirm" {
                    Section { Button("Allow") { answer(confirmed: true) }; Button("Decline", role: .cancel) { answer(confirmed: false) } }.disabled(answering)
                } else {
                    Section {
                        if dialog.method == "editor" { TextEditor(text: $text).frame(minHeight: 240).accessibilityLabel(dialog.title ?? "Extension editor") }
                        else { TextField(dialog.placeholder ?? "Your answer", text: $text).accessibilityLabel(dialog.title ?? "Extension input") }
                        Button("Send answer") { answer(value: text) }.disabled(answering)
                    }
                }
            }
            .safeAreaInset(edge: .top) { if let error = model.error { Text(error).font(.footnote).foregroundStyle(.red).padding(12).frame(maxWidth: .infinity).background(.bar) } }
            .navigationTitle(dialog.title ?? "Pi needs your input").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button("Cancel") { answer(cancelled: true) }.disabled(answering) } }
            .onAppear { text = dialog.prefill ?? "" }
        }
    }
    private func answer(value: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false) {
        answering = true; Task { await model.answer(dialog, value: value, confirmed: confirmed, cancelled: cancelled); answering = false }
    }
}
