import AppKit
import CoreImage.CIFilterBuiltins
import SwiftUI

struct PointGuardSetupView: View {
    @ObservedObject var service: PointGuardServiceModel
    @ObservedObject var chat: PiChatModel
    @State private var provider = ""
    @State private var model = ""
    @State private var response = ""
    @State private var workingDirectory = ""
    @State private var showPhone = false
    private var selected: PiJSONValue { service.providers.first { $0["id"].text == provider } ?? .null }
    private func flag(_ value: PiJSONValue) -> Bool { if case .bool(true) = value { return true }; return false }
    private func values(_ value: PiJSONValue) -> [PiJSONValue] { if case .array(let result) = value { return result }; return [] }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Label(service.phase, systemImage: service.ready ? "checkmark.circle" : "circle")
                Spacer()
                if service.ready {
                    Button("Stop Pi") { Task { do { try await service.stop() } catch { service.error = error.localizedDescription } } }.disabled(chat.busy)
                    Button("Restart Pi") { Task { await service.restart(chat: chat) } }.disabled(chat.busy)
                } else {
                    Button("Start / Retry") { service.start(chat: chat) }
                }
            }
            Text("Pi runs with PurePoint. Your provider and conversations are saved on this Mac.")
                .font(.callout).foregroundStyle(.secondary)
            if let error = service.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            if service.restartRequired {
                HStack {
                    Text("Setup saved. Apply when Pi is idle.")
                    Button("Apply and restart") { Task { await service.restart(chat: chat) } }.disabled(chat.busy)
                }
            }
            if service.ready {
                GroupBox("Provider") {
                    VStack(alignment: .leading, spacing: 10) {
                        Picker("Provider", selection: $provider) {
                            Text("Choose provider").tag("")
                            ForEach(service.providers.indices, id: \.self) { index in
                                let item = service.providers[index]
                                Text(item["name"].text ?? item["id"].text ?? "Provider").tag(item["id"].text ?? "")
                            }
                        }
                        HStack {
                            if flag(selected["oauth"]) {
                                Button("Sign in with browser") { response = ""; Task { await service.login(provider: provider, type: "oauth") } }
                            }
                            if flag(selected["apiKey"]) {
                                Button("Use API key") { response = ""; Task { await service.login(provider: provider, type: "api_key") } }
                            }
                            if flag(selected["configured"]) { Label("Configured", systemImage: "checkmark") }
                        }.disabled(chat.busy)
                        loginInteraction
                        let models = values(selected["models"])
                        if !models.isEmpty {
                            HStack {
                                Picker("Model", selection: $model) {
                                    Text("Choose model").tag("")
                                    ForEach(models.indices, id: \.self) { index in
                                        Text(models[index]["name"].text ?? models[index]["id"].text ?? "Model").tag(models[index]["id"].text ?? "")
                                    }
                                }
                                Button("Use model") { Task { await service.selectModel(provider: provider, model: model) } }
                                    .disabled(model.isEmpty || chat.busy)
                            }
                        }
                    }.padding(8)
                }
                GroupBox("Working folder") {
                    HStack {
                        Text(workingDirectory.isEmpty ? service.cwd : workingDirectory).lineLimit(2).textSelection(.enabled)
                        Spacer()
                        Button("Choose…") {
                            let panel = NSOpenPanel(); panel.canChooseDirectories = true; panel.canChooseFiles = false
                            panel.allowsMultipleSelection = false
                            if panel.runModal() == .OK, let url = panel.url {
                                workingDirectory = url.path
                                Task { await service.configure(cwd: url.path) }
                            }
                        }.disabled(chat.busy)
                    }.padding(8)
                }
                GroupBox("Phone") {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack {
                            Text("Pair once. Saved devices reconnect securely through Tailscale.")
                            Spacer()
                            Button("Connect phone") { showPhone = true; Task { await service.connectPhone() } }
                        }
                        ForEach(service.devices.indices, id: \.self) { index in
                            let device = service.devices[index]
                            HStack {
                                Text(device["name"].text ?? "Phone")
                                Spacer()
                                if case .null = device["revokedAt"] {
                                    Button("Require re-pair") { if let id = device["deviceId"].text { Task { await service.rotateDevice(id) } } }
                                    Button("Revoke") { if let id = device["deviceId"].text { Task { await service.revokeDevice(id) } } }
                                } else { Text("Revoked").foregroundStyle(.secondary) }
                            }
                        }
                    }.padding(8)
                }
            }
        }
        .task { service.start(chat: chat); if service.ready { await service.refresh() } }
         .onAppear { provider = service.selectedProvider; model = service.selectedModel }
        .onChange(of: provider) { _, value in
            model = value == service.selectedProvider ? service.selectedModel : ""
            response = ""; Task { await service.cancelLogin() }
        }
        .onChange(of: service.selectedProvider) { old, value in
            if provider.isEmpty || provider == old { provider = value; model = service.selectedModel }
        }
        .onChange(of: service.selectedModel) { _, value in if provider == service.selectedProvider { model = value } }
        .onChange(of: service.auth["prompt"]["id"].text) { _, _ in response = "" }
        .onChange(of: service.auth["status"].text) { _, value in if value != "pending" { response = "" } }
        .sheet(isPresented: $showPhone) { PointGuardPhoneView(service: service) }
    }
    @ViewBuilder private var loginInteraction: some View {
        if let status = service.auth["status"].text {
            Text("Login: " + status).font(.callout).foregroundStyle(.secondary)
            ForEach(values(service.auth["events"]).indices, id: \.self) { index in
                let event = values(service.auth["events"])[index]
                if let message = event["message"].text { Text(message).textSelection(.enabled) }
                if let code = event["userCode"].text { Text("Code: " + code).font(.system(.body, design: .monospaced)).textSelection(.enabled) }
                if let text = event["url"].text ?? event["verificationUri"].text,
                   let url = URL(string: text), url.scheme == "https" {
                    Link("Open provider sign-in", destination: url)
                }
                if let instructions = event["instructions"].text { Text(instructions).font(.callout) }
            }
            if let promptId = service.auth["prompt"]["id"].text {
                let prompt = service.auth["prompt"]
                Text(prompt["message"].text ?? "Provider response")
                if prompt["type"].text == "select" {
                    ForEach(values(prompt["options"]).indices, id: \.self) { index in
                        let option = values(prompt["options"])[index]
                        Button(option["label"].text ?? option["id"].text ?? "Select") {
                            if let id = option["id"].text { Task { await service.respond(id) } }
                        }
                    }
                } else {
                    HStack {
                        SecureField(prompt["placeholder"].text ?? "Response", text: $response).textFieldStyle(.roundedBorder)
                        Button("Continue") { let value = response; response = ""; Task { await service.respond(value) } }.disabled(response.isEmpty)
                    }.id(promptId)
                }
            }
            if let message = service.auth["error"].text ?? service.auth["error"]["message"].text { Text(message).foregroundStyle(.red) }
            if status == "pending" { Button("Cancel login") { response = ""; Task { await service.cancelLogin() } } }
        }
    }
}

struct PointGuardPhoneView: View {
    @ObservedObject var service: PointGuardServiceModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(spacing: 16) {
            Text("Connect your phone").font(.title2)
            Text("Open the iPhone app → Connection → Scan Mac QR code. Keep Tailscale connected on both devices.")
            if service.enrollmentStatus == "consumed" {
                Label("Phone connected", systemImage: "checkmark.circle.fill")
            } else if let payload = service.enrollment["payload"].text, let image = qr(payload) {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    let expired = expiry.map { context.date >= $0 } ?? true
                    if expired { Text("This QR has expired. Generate a new code.") }
                    else {
                        Image(nsImage: image).interpolation(.none).resizable().frame(width: 260, height: 260)
                            .padding(16).background(.white)
                        Text("One-time code · expires " + (expiry?.formatted(date: .omitted, time: .standard) ?? "soon"))
                            .font(.callout).foregroundStyle(.secondary)
                    }
                }
            } else { Text(service.error ?? "Preparing a one-time pairing code…") }
            Text("This QR enrolls one device. Revoke saved devices from Point Guard setup.").font(.callout).foregroundStyle(.secondary)
            HStack {
                Button("New code") { Task { await service.closeEnrollment(); await service.connectPhone() } }
                Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
            }
        }.padding(28).frame(width: 420)
         .task(id: service.enrollment["enrollmentId"].text) {
            while !Task.isCancelled && service.enrollmentStatus == "pending" {
                await service.refreshEnrollment()
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .onDisappear { Task { await service.closeEnrollment(); await service.refresh() } }
    }
    private var expiry: Date? {
        if case .number(let milliseconds) = service.enrollment["expiresAt"] { return Date(timeIntervalSince1970: milliseconds / 1000) }
        return nil
    }
    private func qr(_ payload: String) -> NSImage? {
        let filter = CIFilter.qrCodeGenerator(); filter.message = Data(payload.utf8); filter.correctionLevel = "M"
        guard let output = filter.outputImage,
              let image = CIContext().createCGImage(output.transformed(by: CGAffineTransform(scaleX: 8, y: 8)), from: output.extent.applying(CGAffineTransform(scaleX: 8, y: 8))) else { return nil }
        return NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
    }
}
