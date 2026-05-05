import SwiftUI

struct MainTabView: View {
    var body: some View {
        TabView {
            ProjectsListView()
                .tabItem {
                    Label("Projects", systemImage: "folder.fill")
                }

            SettingsView()
                .tabItem {
                    Label("Settings", systemImage: "gearshape.fill")
                }
        }
        .tint(Color(red: 0.0, green: 0.314, blue: 0.627))
    }
}

struct ProjectsListView: View {
    @State private var projectsList: [Project] = []
    @State private var isLoading = true
    @State private var error = ""

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView("Loading projects...")
                } else if projectsList.isEmpty {
                    ContentUnavailableView("No Projects",
                        systemImage: "folder.badge.plus",
                        description: Text("Open the web app to create your first project"))
                } else {
                    List(projectsList) { project in
                        NavigationLink(destination: ProjectDetailView(project: project)) {
                            ProjectRow(project: project)
                        }
                    }
                    .listStyle(.insetGrouped)
                }
            }
            .navigationTitle("Projects")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: load) {
                        Image(systemName: "arrow.clockwise")
                    }
                }
            }
        }
        .task { load() }
    }

    private func load() {
        isLoading = true
        Task {
            do {
                projectsList = try await APIService.shared.listProjects()
            } catch {
                self.error = error.localizedDescription
            }
            isLoading = false
        }
    }
}

struct ProjectRow: View {
    let project: Project
    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color(red: 0.945, green: 0.961, blue: 1.0))
                    .frame(width: 36, height: 36)
                Image(systemName: project.sourceType == "git" ? "arrow.triangle.branch" : "folder.fill")
                    .font(.system(size: 14))
                    .foregroundColor(Color(red: 0.0, green: 0.314, blue: 0.627))
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(project.name).font(.body.weight(.medium))
                if let desc = project.description {
                    Text(desc).font(.caption).foregroundColor(.secondary).lineLimit(1)
                }
            }
        }
        .padding(.vertical, 4)
    }
}

struct ProjectDetailView: View {
    let project: Project
    @State private var convsList: [Conversation] = []
    @State private var isLoading = true

    var body: some View {
        Group {
            if isLoading {
                ProgressView()
            } else if convsList.isEmpty {
                ContentUnavailableView("No Conversations",
                    systemImage: "bubble.left.and.bubble.right",
                    description: Text("Start a conversation in the web app"))
            } else {
                List(convsList) { conv in
                    NavigationLink(destination: ConversationView(project: project, conversation: conv)) {
                        Label(conv.title, systemImage: "bubble.left.fill")
                    }
                }
                .listStyle(.insetGrouped)
            }
        }
        .navigationTitle(project.name)
        .task {
            do {
                convsList = try await APIService.shared.listConversations(projectId: project.id)
            } catch {}
            isLoading = false
        }
    }
}

struct ConversationView: View {
    let project: Project
    let conversation: Conversation
    @State private var messages: [Message] = []
    @State private var isLoading = true

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 12) {
                    ForEach(messages) { msg in
                        MessageBubble(message: msg)
                            .id(msg.id)
                    }
                }
                .padding()
            }
            .onChange(of: messages.count) { _, _ in
                if let last = messages.last {
                    proxy.scrollTo(last.id, anchor: .bottom)
                }
            }
        }
        .navigationTitle(conversation.title)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            do {
                let data = try await APIService.shared.getConversation(
                    projectId: project.id, convId: conversation.id)
                messages = data.messages
            } catch {}
            isLoading = false
        }
    }
}

struct MessageBubble: View {
    let message: Message
    var isUser: Bool { message.role == "user" }

    var agentColor: Color {
        message.role == "hermes"
            ? Color(red: 0.486, green: 0.227, blue: 0.929)
            : Color(red: 0.0, green: 0.314, blue: 0.627)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            if !isUser {
                ZStack {
                    Circle().fill(agentColor).frame(width: 28, height: 28)
                    Image(systemName: message.role == "hermes" ? "brain" : "cpu")
                        .font(.system(size: 12)).foregroundColor(.white)
                }
            }
            VStack(alignment: isUser ? .trailing : .leading, spacing: 4) {
                if !isUser {
                    Text(message.agentName ?? message.role.capitalized)
                        .font(.caption.bold()).foregroundColor(agentColor)
                }
                Text(message.content)
                    .font(.subheadline)
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .background(isUser ? Color(red: 0.0, green: 0.176, blue: 0.384) : Color(.systemGray6))
                    .foregroundColor(isUser ? .white : .primary)
                    .cornerRadius(16)
                    .cornerRadius(isUser ? 4 : 16, corners: isUser ? .topRight : .topLeft)
            }
            if isUser { Spacer(minLength: 40) } else { Spacer(minLength: 40) }
        }
        .frame(maxWidth: .infinity, alignment: isUser ? .trailing : .leading)
    }
}

struct SettingsView: View {
    @EnvironmentObject var authStore: AuthStore
    var body: some View {
        NavigationStack {
            List {
                Section {
                    if let user = authStore.user {
                        LabeledContent("Name", value: user.displayName)
                        LabeledContent("Email", value: user.email)
                    }
                } header: { Text("Account") }

                Section {
                    Button(role: .destructive) { authStore.logout() } label: {
                        Label("Sign Out", systemImage: "rectangle.portrait.and.arrow.right")
                    }
                }
            }
            .navigationTitle("Settings")
        }
    }
}

extension View {
    func cornerRadius(_ radius: CGFloat, corners: UIRectCorner) -> some View {
        clipShape(RoundedCorner(radius: radius, corners: corners))
    }
}

struct RoundedCorner: Shape {
    var radius: CGFloat
    var corners: UIRectCorner
    func path(in rect: CGRect) -> Path {
        Path(UIBezierPath(roundedRect: rect, byRoundingCorners: corners,
                          cornerRadii: CGSize(width: radius, height: radius)).cgPath)
    }
}
