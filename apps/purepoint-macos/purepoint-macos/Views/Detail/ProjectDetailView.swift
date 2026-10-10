import SwiftUI

struct ProjectDetailView: View {
    let project: ProjectState
    @Binding var selection: SidebarSelection?
    private enum ProjectTab: String, CaseIterable, Identifiable {
        case overview = "Overview"
        case channel = "Channel"

        var id: String { rawValue }
    }

    @State private var selectedTab: ProjectTab = .overview
    @State private var showRootFiles = false
    @State private var summaries: [String: GitReviewSummary] = [:]
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "folder").foregroundStyle(.secondary)
                Text(project.projectName).font(.system(size: 15, weight: .semibold))
                Text("\(project.worktrees.count) worktrees").font(.system(size: 11)).foregroundStyle(.secondary)
                Spacer()
                Picker("Project view", selection: $selectedTab) {
                    ForEach(ProjectTab.allCases) { tab in
                        Text(tab.rawValue).tag(tab)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: 200)
            }.padding(.horizontal, 20).padding(.vertical, 14)
            Divider()
            if selectedTab == .channel {
                ProjectChannelView(project: project)
            } else if showRootFiles {
                HStack {
                    Button("Back to overview") { showRootFiles = false }
                    Spacer()
                }.padding(.horizontal, 20).padding(.vertical, 10)
                RootCheckoutDetailView(project: project)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    Text("WORKTREES").font(.system(size: 10, weight: .semibold)).tracking(1).foregroundStyle(.secondary).padding(18)
                    ScrollView {
                        VStack(spacing: 0) {
                            Button { showRootFiles = true } label: { row(name: "Root checkout", branch: project.projectName, agents: project.rootAgents, summary: summaries[project.projectRoot], root: true) }.buttonStyle(.plain)
                            Divider().padding(.horizontal, 18)
                            ForEach(project.worktrees) { worktree in
                                Button { selection = .worktree(worktree.id) } label: { row(name: worktree.name, branch: worktree.branch, agents: worktree.agents, summary: summaries[worktree.path]) }.buttonStyle(.plain)
                                Divider().padding(.horizontal, 18)
                            }
                        }
                    }
                }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        }.onChange(of: project.projectRoot) { _, _ in
            selectedTab = .overview
            showRootFiles = false
            summaries = [:]
        }.task(id: project.projectRoot) {
            while !Task.isCancelled {
                guard NSApplication.shared.isActive else {
                    do { try await Task.sleep(for: .seconds(5)) } catch { return }
                    continue
                }
                let service = GitService()
                var next: [String: GitReviewSummary] = [:]
                next[project.projectRoot] = await service.reviewSummary(at: project.projectRoot, baseBranch: nil)
                for worktree in project.worktrees {
                    next[worktree.path] = await service.reviewSummary(at: worktree.path, baseBranch: worktree.baseBranch)
                    if Task.isCancelled { return }
                }
                summaries = next
                do { try await Task.sleep(for: .seconds(5)) } catch { return }
            }
        }
    }
    private func row(name: String, branch: String, agents: [AgentModel], summary: GitReviewSummary?, root: Bool = false) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: root ? "folder" : "arrow.triangle.branch").foregroundStyle(.secondary).padding(.top, 2)
            VStack(alignment: .leading, spacing: 7) {
                Text(name).font(.system(size: 13, weight: .medium)).lineLimit(1)
                Text(branch).font(.system(size: 10, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1)
                if let summary {
                    if let error = summary.error { Text(error).font(.system(size: 10)).foregroundStyle(.orange).lineLimit(2) }
                    else { Text("\(summary.commitCount) commits · \(summary.localFileCount) local files").font(.system(size: 11)).foregroundStyle(.secondary) }
                } else { Text("Reading changes…").font(.system(size: 11)).foregroundStyle(.tertiary) }
                if !agents.isEmpty { Text(agents.map(\.displayName).joined(separator: ", ")).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(2) }
            }
            Spacer(minLength: 4)
            Image(systemName: "chevron.right").font(.system(size: 9)).foregroundStyle(.tertiary)
        }.padding(18).frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
    }
}
