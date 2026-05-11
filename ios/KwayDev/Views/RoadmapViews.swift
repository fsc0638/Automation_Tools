import SwiftUI

// MARK: - Color helpers

private func priorityColor(_ p: String) -> Color {
    switch p {
    case "critical": return Color.red
    case "high":     return Color.orange
    case "medium":   return Color.yellow
    default:         return Color.gray
    }
}

private func statusColor(_ s: String) -> Color {
    switch s {
    case "done":        return Color.green
    case "in-progress": return Color.blue
    case "cancelled":   return Color.gray
    default:            return Color(.systemGray3)
    }
}

private struct Chip: View {
    let text: String
    let color: Color
    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 6).padding(.vertical, 2)
            .background(color.opacity(0.15))
            .foregroundColor(color)
            .clipShape(Capsule())
    }
}

// MARK: - Per-project Tasks list

struct ProjectTasksView: View {
    let project: Project
    @State private var tasks: [ProjectTask] = []
    @State private var isLoading = true
    @State private var error = ""

    var body: some View {
        Group {
            if isLoading {
                ProgressView()
            } else if tasks.isEmpty {
                ContentUnavailableView(
                    "No Tasks",
                    systemImage: "checklist",
                    description: Text("Use the web Roadmap to create tasks.")
                )
            } else {
                List {
                    ForEach(["todo", "in-progress", "done", "cancelled"], id: \.self) { col in
                        let column = tasks.filter { $0.status == col }
                        if !column.isEmpty {
                            Section(header: Text(label(col))) {
                                ForEach(column) { task in
                                    TaskRow(task: task)
                                }
                            }
                        }
                    }
                }
                .listStyle(.insetGrouped)
            }
        }
        .navigationTitle("Tasks")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button(action: load) { Image(systemName: "arrow.clockwise") }
            }
        }
        .task { load() }
    }

    private func label(_ status: String) -> String {
        switch status {
        case "in-progress": return "In progress"
        case "done":        return "Done"
        case "cancelled":   return "Cancelled"
        default:            return "Todo"
        }
    }

    private func load() {
        isLoading = true
        Task {
            do {
                tasks = try await APIService.shared.listTasks(projectId: project.id)
            } catch {
                self.error = error.localizedDescription
            }
            isLoading = false
        }
    }
}

private struct TaskRow: View {
    let task: ProjectTask
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(task.title).font(.body.weight(.medium))
            if let w = task.why, !w.isEmpty {
                Text(w).font(.caption).foregroundColor(.secondary).lineLimit(2)
            }
            HStack(spacing: 6) {
                Chip(text: task.priority, color: priorityColor(task.priority))
                if let assignee = task.assignee, !assignee.isEmpty {
                    Chip(text: "@\(assignee)", color: .secondary)
                }
                if let sprintName = task.sprintName {
                    Chip(text: sprintName, color: .indigo)
                }
                if let epicName = task.epicName {
                    Chip(text: "🎯 \(epicName)", color: .pink)
                }
                if task.linkedPrUrl != nil {
                    Chip(text: "PR", color: .purple)
                }
                if let c = task.commentCount, c > 0 {
                    Chip(text: "💬 \(c)", color: .cyan)
                }
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - Global Roadmap (cross-project)

struct GlobalRoadmapView: View {
    @State private var tasks: [UserTask] = []
    @State private var isLoading = true
    @State private var error = ""
    @State private var filter: String = "all"  // status filter

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView()
                } else if tasks.isEmpty {
                    ContentUnavailableView(
                        "No Tasks",
                        systemImage: "map",
                        description: Text("Once you have tasks across projects they'll show up here.")
                    )
                } else {
                    List {
                        Picker("Filter", selection: $filter) {
                            Text("All").tag("all")
                            Text("Todo").tag("todo")
                            Text("In progress").tag("in-progress")
                            Text("Done").tag("done")
                        }
                        .pickerStyle(.segmented)

                        ForEach(filteredTasks) { task in
                            UserTaskRow(task: task)
                        }
                    }
                    .listStyle(.insetGrouped)
                }
            }
            .navigationTitle("Roadmap")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: load) { Image(systemName: "arrow.clockwise") }
                }
            }
            .task { load() }
        }
    }

    private var filteredTasks: [UserTask] {
        guard filter != "all" else { return tasks }
        return tasks.filter { $0.status == filter }
    }

    private func load() {
        isLoading = true
        Task {
            do {
                tasks = try await APIService.shared.listUserTasks()
            } catch {
                self.error = error.localizedDescription
            }
            isLoading = false
        }
    }
}

private struct UserTaskRow: View {
    let task: UserTask
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(task.title).font(.body.weight(.medium))
            HStack(spacing: 6) {
                Chip(text: task.projectName, color: .blue)
                Chip(text: task.priority, color: priorityColor(task.priority))
                Chip(text: task.status, color: statusColor(task.status))
                if let assignee = task.assignee, !assignee.isEmpty {
                    Chip(text: "@\(assignee)", color: .secondary)
                }
                if let epicName = task.epicName {
                    Chip(text: "🎯 \(epicName)", color: .pink)
                }
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - Agents list

struct AgentsListView: View {
    @State private var agents: [AgentProfile] = []
    @State private var isLoading = true

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView()
                } else if agents.isEmpty {
                    ContentUnavailableView(
                        "No Agents",
                        systemImage: "person.crop.circle.badge.questionmark",
                        description: Text("Open the web app to add a custom agent profile.")
                    )
                } else {
                    List(agents) { a in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text(a.name).font(.body.weight(.medium))
                                Spacer()
                                if a.enabled {
                                    Chip(text: "Enabled", color: .green)
                                } else {
                                    Chip(text: "Disabled", color: .gray)
                                }
                            }
                            Text("\(a.provider) · \(a.model)")
                                .font(.caption).foregroundColor(.secondary)
                            if !a.labels.isEmpty {
                                HStack(spacing: 4) {
                                    ForEach(a.labels, id: \.self) { label in
                                        Chip(text: label, color: .indigo)
                                    }
                                }
                            }
                        }
                        .padding(.vertical, 2)
                    }
                    .listStyle(.insetGrouped)
                }
            }
            .navigationTitle("Agents")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: load) { Image(systemName: "arrow.clockwise") }
                }
            }
            .task { load() }
        }
    }

    private func load() {
        isLoading = true
        Task {
            do {
                agents = try await APIService.shared.listAgents()
            } catch { /* surface in a toast in future revisions */ }
            isLoading = false
        }
    }
}
