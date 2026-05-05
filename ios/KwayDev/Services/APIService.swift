import Foundation

class APIService {
    static let shared = APIService()
    private let baseURL: String

    private init() {
        baseURL = Bundle.main.object(forInfoDictionaryKey: "API_BASE_URL") as? String
            ?? "http://localhost:8080/api"
    }

    private var token: String? {
        UserDefaults.standard.string(forKey: "kway_token")
    }

    private func request<T: Decodable>(_ path: String, method: String = "GET", body: Encodable? = nil) async throws -> T {
        guard let url = URL(string: baseURL + path) else { throw APIError.invalidURL }
        var req = URLRequest(url: url)
        req.httpMethod = method
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let t = token { req.setValue("Bearer \(t)", forHTTPHeaderField: "Authorization") }
        if let b = body { req.httpBody = try JSONEncoder().encode(b) }

        let (data, response) = try await URLSession.shared.data(for: req)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            let err = try? JSONDecoder().decode(ErrorResponse.self, from: data)
            throw APIError.serverError(err?.error ?? "Unknown error")
        }
        return try JSONDecoder().decode(T.self, from: data)
    }

    // Auth
    func login(email: String, password: String) async throws -> AuthResponse {
        try await request("/auth/login", method: "POST", body: LoginRequest(email: email, password: password))
    }

    func register(email: String, password: String, displayName: String) async throws -> AuthResponse {
        try await request("/auth/register", method: "POST",
                          body: RegisterRequest(email: email, password: password, display_name: displayName))
    }

    // Projects
    func listProjects() async throws -> [Project] {
        try await request("/projects")
    }

    // Conversations
    func listConversations(projectId: String) async throws -> [Conversation] {
        try await request("/projects/\(projectId)/conversations")
    }

    func getConversation(projectId: String, convId: String) async throws -> ConversationWithMessages {
        try await request("/projects/\(projectId)/conversations/\(convId)")
    }
}

struct ConversationWithMessages: Decodable {
    let id: String
    let title: String
    let messages: [Message]
}

struct LoginRequest: Encodable { let email: String; let password: String }
struct RegisterRequest: Encodable { let email: String; let password: String; let display_name: String }
struct ErrorResponse: Decodable { let error: String }

enum APIError: LocalizedError {
    case invalidURL
    case serverError(String)
    var errorDescription: String? {
        switch self {
        case .invalidURL: return "Invalid URL"
        case .serverError(let msg): return msg
        }
    }
}
