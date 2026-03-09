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

    func loadTriggers(projectRoot: String) async {
        isLoading = true
        error = nil
        do {
            let response = try await client.send(.listTriggers(projectRoot: projectRoot))
            if case .triggerList(let payloads) = response {
                self.triggers = payloads.map { TriggerItem(from: $0) }
            }
        } catch {
            self.error = "Failed to load triggers: \(error.localizedDescription)"
            print("Failed to load triggers: \(error)")
        }
        isLoading = false
    }

    func saveTrigger(
        projectRoot: String,
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
            await loadTriggers(projectRoot: projectRoot)
        } catch {
            self.error = "Failed to save trigger: \(error.localizedDescription)"
            print("Failed to save trigger: \(error)")
        }
    }

    func deleteTrigger(projectRoot: String, name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteTrigger(projectRoot: projectRoot, name: name, scope: scope))
            if selectedTriggerId == name { selectedTriggerId = nil }
            await loadTriggers(projectRoot: projectRoot)
        } catch {
            self.error = "Failed to delete trigger: \(error.localizedDescription)"
            print("Failed to delete trigger: \(error)")
        }
    }
}
