import SwiftUI
#if os(iOS)
import UIKit
#endif

/// Connection controls live in the standard tab strip; history controls stay in the sidebar.
struct PointGuardPaneControls: View {
    @ObservedObject var model: PiChatModel
    @Environment(AppState.self) private var appState
    @State private var showSetup = false
    #if os(macOS)
    @State private var showPhone = false
    #endif

    var body: some View {
        HStack(spacing: 6) {
            Group {
                if model.busy {
                    TypingDots(diameter: 3, spacing: 1.5)
                } else {
                    Circle().fill(model.connected ? Color.green : Color.secondary).frame(width: 6, height: 6)
                }
            }
            .frame(width: 12)
            .help(model.connectionStatus)
            .accessibilityLabel(model.busy ? "Point Guard is working" : model.connectionStatus)
            Menu {
                // Pairing phones belongs to the Mac that hosts Point Guard (REQ-IPAD-038).
                #if os(macOS)
                Button("Connect phone") {
                    showPhone = true
                    Task { await appState.pointGuardService.connectPhone() }
                }
                #endif
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
        #if os(iOS) && DEBUG
        .onAppear { if DebugLaunchArguments.current.pointGuardSetup { showSetup = true } }
        #endif
        .sheet(isPresented: $showSetup) {
            VStack(spacing: 0) {
                HStack {
                    Text("Point Guard setup").font(.headline)
                    Spacer()
                    Button("Done") { showSetup = false }.keyboardShortcut(.cancelAction)
                }.padding()
                Divider()
                ScrollView {
                    #if os(macOS)
                    PointGuardSetupView(service: appState.pointGuardService, chat: model).padding(24)
                    #else
                    PointGuardSetupView(chat: model).padding(24)
                    #endif
                }
            }.frame(width: 620, height: 650)
            #if os(iOS)
            // The strip forces a dark scheme; the sheet follows the window like a Mac sheet window.
            .environment(\.colorScheme, Self.windowColorScheme)
            #endif
        }
        #if os(macOS)
        .sheet(isPresented: $showPhone) {
            PointGuardPhoneView(service: appState.pointGuardService, isPresented: $showPhone)
        }
        #endif
    }

    #if os(iOS)
    private static var windowColorScheme: ColorScheme {
        let style = UIApplication.shared.connectedScenes.lazy
            .compactMap { ($0 as? UIWindowScene)?.traitCollection.userInterfaceStyle }.first
        return style == .dark ? .dark : .light
    }
    #endif
}
