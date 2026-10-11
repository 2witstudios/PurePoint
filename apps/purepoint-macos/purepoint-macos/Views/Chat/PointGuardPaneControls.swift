import SwiftUI

/// Connection controls live in the standard tab strip; history controls stay in the sidebar.
struct PointGuardPaneControls: View {
    @ObservedObject var model: PiChatModel
    @Environment(AppState.self) private var appState
    @State private var showSetup = false
    @State private var showPhone = false

    var body: some View {
        HStack(spacing: 6) {
            Group {
                if model.busy {
                    ProgressView().controlSize(.mini)
                } else {
                    Circle().fill(model.connected ? Color.green : Color.secondary).frame(width: 6, height: 6)
                }
            }
            .frame(width: 12)
            .help(model.connectionStatus)
            .accessibilityLabel(model.busy ? "Point Guard is working" : model.connectionStatus)
            Menu {
                Button("Connect phone") {
                    showPhone = true
                    Task { await appState.pointGuardService.connectPhone() }
                }
                Button("Setup") { showSetup = true }
                Divider()
                Button(model.connected ? "Disconnect" : "Connect") {
                    if model.connected { model.disconnect() } else { model.connect() }
                }
            } label: {
                Image(systemName: "ellipsis").frame(width: 24, height: 22)
            }
            .menuStyle(.borderlessButton).fixedSize()
            .accessibilityLabel("Point Guard options")
        }
        .buttonStyle(.plain).foregroundStyle(.secondary)
        .sheet(isPresented: $showSetup) {
            VStack(spacing: 0) {
                HStack {
                    Text("Point Guard setup").font(.headline)
                    Spacer()
                    Button("Done") { showSetup = false }.keyboardShortcut(.cancelAction)
                }.padding()
                Divider()
                ScrollView {
                    PointGuardSetupView(service: appState.pointGuardService, chat: model).padding(24)
                }
            }.frame(width: 620, height: 650)
        }
        .sheet(isPresented: $showPhone) {
            PointGuardPhoneView(service: appState.pointGuardService, isPresented: $showPhone)
        }
    }
}
