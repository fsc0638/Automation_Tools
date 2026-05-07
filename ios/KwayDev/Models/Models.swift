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
    let tokenType: String
    let user: User
    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token", tokenType = "token_type", user
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
