import SwiftUI

@Observable
@MainActor
final class TriggersState {
    var triggers: [TriggerItem] = []
    var selectedTriggerId: String?
    var showingCreationSheet = false
    var isLoading = false
    var error: String?

    @ObservationIgnored private let client: DaemonClient

    init(client: DaemonClient = DaemonClient()) {
        self.client = client
    }

    var selectedTrigger: TriggerItem? {
        guard let id = selectedTriggerId else { return nil }
        return triggers.first { $0.id == id }
    }

    // MARK: - Backend Integration

    func loadTriggers(projectRoots: [String]) async {
        isLoading = true
        error = nil
        do {
            var merged: [String: TriggerItem] = [:]
            for root in projectRoots {
                let projectName = URL(fileURLWithPath: root).lastPathComponent
                let response = try await client.send(.listTriggers(projectRoot: root))
                if case .triggerList(let payloads) = response {
                    for payload in payloads {
                        var item = TriggerItem(from: payload)
                        if payload.scope != "global" {
                            item.projectRoot = root
                            item.projectName = projectName
                        }
                        let key = "\(item.scope):\(item.name)"
                        if merged[key] == nil {
                            merged[key] = item
                        }
                    }
                }
            }
            self.triggers = Array(merged.values).sorted { $0.name < $1.name }
        } catch {
            self.error = "Failed to load triggers: \(error.localizedDescription)"
            print("Failed to load triggers: \(error)")
        }
        isLoading = false
    }

    func saveTrigger(
        projectRoot: String,
        projectRoots: [String],
        name: String,
        description: String?,
        on: String,
        sequence: [TriggerActionPayload],
        variables: [String: String],
        scope: String
    ) async {
        do {
            _ = try await client.send(
                .saveTrigger(
                    projectRoot: projectRoot,
                    name: name,
                    description: description,
                    on: on,
                    sequence: sequence,
                    variables: variables,
                    scope: scope
                ))
            await loadTriggers(projectRoots: projectRoots)
        } catch {
            self.error = "Failed to save trigger: \(error.localizedDescription)"
            print("Failed to save trigger: \(error)")
        }
    }

    func deleteTrigger(projectRoot: String, projectRoots: [String], name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteTrigger(projectRoot: projectRoot, name: name, scope: scope))
            if selectedTriggerId == name { selectedTriggerId = nil }
            await loadTriggers(projectRoots: projectRoots)
        } catch {
            self.error = "Failed to delete trigger: \(error.localizedDescription)"
            print("Failed to delete trigger: \(error)")
        }
    }
}
