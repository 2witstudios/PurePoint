import SwiftUI

struct ScheduleHeaderView: View {
    @Bindable var state: ScheduleState

    var body: some View {
        HStack(spacing: 12) {
            viewModePicker

            Spacer()

            dateNavigation

            Spacer()

            newButton
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    // MARK: - View Mode Picker

    private var viewModePicker: some View {
        Picker("", selection: $state.viewMode) {
            ForEach(ScheduleViewMode.allCases) { mode in
                Text(mode.label).tag(mode)
            }
        }
        .pickerStyle(.segmented)
        .frame(width: 240)
    }

    // MARK: - Date Navigation

    private var dateNavigation: some View {
        HStack(spacing: 8) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    state.goBackward()
                }
            } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 11, weight: .semibold))
            }
            .buttonStyle(.borderless)

            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    state.goToToday()
                }
            } label: {
                Text("Today")
                    .font(.system(size: 12, weight: .medium))
            }
            .buttonStyle(.borderless)

            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    state.goForward()
                }
            } label: {
                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .semibold))
            }
            .buttonStyle(.borderless)

            Text(state.headerDateLabel)
                .font(.system(size: 14, weight: .semibold))
                .frame(minWidth: 160)
        }
    }

    // MARK: - New Button

    private var newButton: some View {
        Button {
            state.creationPrefillDate = nil
            state.showingCreationSheet = true
        } label: {
            Label("New Schedule", systemImage: "plus")
                .font(.system(size: 12, weight: .medium))
        }
        .buttonStyle(.borderless)
    }
}
