import Foundation

struct User: Codable, Identifiable {
    let id: String
    let email: String
    let displayName: String
    enum CodingKeys: String, CodingKey {
        case id, email, displayName = "display_name"
    }
}

struct AuthResponse: Codable {
    let accessToken: String
    let refreshToken: String?
    let tokenType: String
    let refreshExpiresIn: Int?
    let user: User
    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case tokenType = "token_type"
        case refreshExpiresIn = "refresh_expires_in"
        case user
    }
}

struct RefreshResponse: Codable {
    let accessToken: String
    let refreshToken: String
    let tokenType: String
    let refreshExpiresIn: Int
    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case tokenType = "token_type"
        case refreshExpiresIn = "refresh_expires_in"
    }
}

struct Project: Codable, Identifiable {
    let id: String
    let userId: String
    let name: String
    let description: String?
    let sourceType: String
    let sourcePath: String
    let createdAt: String
    let updatedAt: String
    enum CodingKeys: String, CodingKey {
        case id, name, description
        case userId = "user_id", sourceType = "source_type", sourcePath = "source_path"
        case createdAt = "created_at", updatedAt = "updated_at"
    }
}

struct Conversation: Codable, Identifiable {
    let id: String
    let projectId: String
    let userId: String
    let title: String
    let mode: String
    let createdAt: String
    let updatedAt: String

    enum CodingKeys: String, CodingKey {
        case id, title, mode
        case projectId = "project_id", userId = "user_id"
        case createdAt = "created_at", updatedAt = "updated_at"
    }

    var modeDisplayName: String {
        switch mode {
        case "hermes": return "Hermes"
        case "debate": return "Debate"
        default: return "OpenClaw"
        }
    }
}

struct Message: Codable, Identifiable {
    let id: String
    let conversationId: String
    let role: String
    let content: String
    let agentName: String?
    let createdAt: String
    enum CodingKeys: String, CodingKey {
        case id, role, content
        case conversationId = "conversation_id", agentName = "agent_name", createdAt = "created_at"
    }
}

// MARK: - Roadmap

struct ProjectTask: Codable, Identifiable {
    let id: String
    let projectId: String
    let title: String
    let why: String?
    let priority: String       // low / medium / high / critical
    let status: String         // todo / in-progress / done / cancelled
    let assignee: String?
    let dueDate: String?
    let labels: [String]
    let sprintId: String?
    let sprintName: String?
    let epicId: String?
    let epicName: String?
    let linkedPrUrl: String?
    let commentCount: Int?
    let createdAt: String
    let updatedAt: String

    enum CodingKeys: String, CodingKey {
        case id, title, why, priority, status, assignee, labels
        case projectId = "project_id"
        case dueDate = "due_date"
        case sprintId = "sprint_id"
        case sprintName = "sprint_name"
        case epicId = "epic_id"
        case epicName = "epic_name"
        case linkedPrUrl = "linked_pr_url"
        case commentCount = "comment_count"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
    }
}

/// Cross-project task (B1) — same shape as ProjectTask but with the
/// project name joined in.
struct UserTask: Codable, Identifiable {
    let id: String
    let projectId: String
    let projectName: String
    let title: String
    let status: String
    let priority: String
    let assignee: String?
    let dueDate: String?
    let labels: [String]
    let sprintId: String?
    let sprintName: String?
    let epicId: String?
    let epicName: String?
    let linkedPrUrl: String?
    let commentCount: Int
    let updatedAt: String

    enum CodingKeys: String, CodingKey {
        case id, title, status, priority, assignee, labels
        case projectId = "project_id"
        case projectName = "project_name"
        case dueDate = "due_date"
        case sprintId = "sprint_id"
        case sprintName = "sprint_name"
        case epicId = "epic_id"
        case epicName = "epic_name"
        case linkedPrUrl = "linked_pr_url"
        case commentCount = "comment_count"
        case updatedAt = "updated_at"
    }
}

struct Sprint: Codable, Identifiable {
    let id: String
    let projectId: String
    let name: String
    let status: String
    let goal: String?
    let startDate: String?
    let endDate: String?
    let taskTotal: Int
    let taskDone: Int
    enum CodingKeys: String, CodingKey {
        case id, name, status, goal
        case projectId = "project_id"
        case startDate = "start_date"
        case endDate = "end_date"
        case taskTotal = "task_total"
        case taskDone = "task_done"
    }
}

struct Epic: Codable, Identifiable {
    let id: String
    let name: String
    let description: String?
    let color: String?
    let status: String
    let targetDate: String?
    let taskTotal: Int
    let taskDone: Int
    let projectCount: Int
    enum CodingKeys: String, CodingKey {
        case id, name, description, color, status
        case targetDate = "target_date"
        case taskTotal = "task_total"
        case taskDone = "task_done"
        case projectCount = "project_count"
    }
}

// MARK: - Agents

struct AgentProfile: Codable, Identifiable {
    let id: String
    let name: String
    let provider: String       // openai / openai_compatible / gemini / anthropic
    let model: String
    let baseUrl: String?
    let enabled: Bool
    let labels: [String]
    let updatedAt: String
    enum CodingKeys: String, CodingKey {
        case id, name, provider, model, enabled, labels
        case baseUrl = "base_url"
        case updatedAt = "updated_at"
    }
}
