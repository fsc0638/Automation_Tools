import Foundation

class APIService {
    static let shared = APIService()
    private let baseURL: String
    private let tokenKey = "kway_token"
    private let refreshKey = "kway_refresh_token"

    /// Coalesces parallel refresh attempts so a burst of 401s only
    /// triggers a single /auth/refresh call. Mirrors the singleton
    /// in the web app's request() helper.
    private actor RefreshGate {
        private var inFlight: Task<Bool, Never>?
        func run(_ op: @escaping @Sendable () async -> Bool) async -> Bool {
            if let existing = inFlight { return await existing.value }
            let task = Task { await op() }
            inFlight = task
            let ok = await task.value
            inFlight = nil
            return ok
        }
    }
    private let refreshGate = RefreshGate()

    private init() {
        baseURL = Bundle.main.object(forInfoDictionaryKey: "API_BASE_URL") as? String
            ?? "http://localhost:8080/api"
    }

    private var token: String? {
        UserDefaults.standard.string(forKey: tokenKey)
    }
    private var refreshToken: String? {
        UserDefaults.standard.string(forKey: refreshKey)
    }
    private func setSession(access: String, refresh: String?) {
        UserDefaults.standard.set(access, forKey: tokenKey)
        if let r = refresh {
            UserDefaults.standard.set(r, forKey: refreshKey)
        }
    }
    func clearSession() {
        UserDefaults.standard.removeObject(forKey: tokenKey)
        UserDefaults.standard.removeObject(forKey: refreshKey)
    }

    private func attemptRefresh() async -> Bool {
        await refreshGate.run { [self] in
            guard let rt = refreshToken,
                  let url = URL(string: baseURL + "/auth/refresh") else { return false }
            var req = URLRequest(url: url)
            req.httpMethod = "POST"
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
            let body: [String: String] = ["refresh_token": rt]
            req.httpBody = try? JSONSerialization.data(withJSONObject: body)
            do {
                let (data, response) = try await URLSession.shared.data(for: req)
                guard let http = response as? HTTPURLResponse,
                      (200..<300).contains(http.statusCode) else { return false }
                let decoded = try JSONDecoder().decode(RefreshResponse.self, from: data)
                setSession(access: decoded.accessToken, refresh: decoded.refreshToken)
                return true
            } catch {
                return false
            }
        }
    }

    /// Core request helper. Transparently retries once on 401 by trying
    /// the refresh-token endpoint; if refresh also fails, the caller
    /// gets `APIError.unauthorized` so the auth layer can clear state.
    private func request<T: Decodable>(_ path: String,
                                       method: String = "GET",
                                       body: Encodable? = nil,
                                       retry: Bool = true) async throws -> T {
        guard let url = URL(string: baseURL + path) else { throw APIError.invalidURL }
        var req = URLRequest(url: url)
        req.httpMethod = method
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let t = token { req.setValue("Bearer \(t)", forHTTPHeaderField: "Authorization") }
        if let b = body { req.httpBody = try JSONEncoder().encode(b) }

        let (data, response) = try await URLSession.shared.data(for: req)
        guard let http = response as? HTTPURLResponse else {
            throw APIError.serverError("Invalid response")
        }
        if http.statusCode == 401 && retry && token != nil && !path.hasPrefix("/auth/") {
            if await attemptRefresh() {
                return try await request(path, method: method, body: body, retry: false)
            }
            clearSession()
            throw APIError.unauthorized
        }
        guard (200..<300).contains(http.statusCode) else {
            let err = try? JSONDecoder().decode(ErrorResponse.self, from: data)
            throw APIError.serverError(err?.error ?? "HTTP \(http.statusCode)")
        }
        if T.self == EmptyResponse.self { return EmptyResponse() as! T }
        return try JSONDecoder().decode(T.self, from: data)
    }

    // MARK: - Auth (client-held KEK protocol, see backend mig 0049)
    //
    // The server never sees the plaintext password.  We derive
    // auth_hash + user_kek locally via ClientCrypto.swift and send
    // only the 32-byte outputs.

    /// Public — used by both register and login flows to learn the
    /// salt + Argon2 contract for an email. Unknown emails get a
    /// deterministic fake salt so this endpoint cannot be used for
    /// account enumeration.
    func fetchKekParams(email: String) async throws -> KekParams {
        try await request("/auth/kek-params?email=\(email.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? email)")
    }

    func login(email: String, password: String) async throws -> AuthResponse {
        let params = try await fetchKekParams(email: email)
        let secrets = try deriveAuthAndKek(password: password, params: params)
        let res: AuthResponse = try await request("/auth/login", method: "POST",
            body: LoginRequest(email: email, auth_hash: secrets.authHashB64, user_kek: secrets.userKekB64))
        setSession(access: res.accessToken, refresh: res.refreshToken)
        await UserKekHolder.shared.remember(secrets.userKekBytes)
        return res
    }

    func register(email: String, password: String, displayName: String) async throws -> AuthResponse {
        // Generate the salt locally — the server only stores it.
        let saltB64 = randomKekSalt()
        // Pull domains + Argon2 params; ignore the fake-salt the server
        // returns for the (still-unknown) email.
        let stockParams = try await fetchKekParams(email: email)
        let params = KekParams(
            kek_salt: saltB64,
            argon2_m_cost: stockParams.argon2_m_cost,
            argon2_t_cost: stockParams.argon2_t_cost,
            argon2_p_cost: stockParams.argon2_p_cost,
            argon2_output_len: stockParams.argon2_output_len,
            kek_domain: stockParams.kek_domain,
            auth_domain: stockParams.auth_domain
        )
        let secrets = try deriveAuthAndKek(password: password, params: params)
        let res: AuthResponse = try await request("/auth/register", method: "POST",
            body: RegisterRequest(
                email: email,
                display_name: displayName,
                kek_salt: saltB64,
                auth_hash: secrets.authHashB64,
                user_kek: secrets.userKekB64
            ))
        setSession(access: res.accessToken, refresh: res.refreshToken)
        await UserKekHolder.shared.remember(secrets.userKekBytes)
        return res
    }

    func changePassword(email: String, currentPassword: String, newPassword: String) async throws {
        let params = try await fetchKekParams(email: email)
        let cur = try deriveAuthAndKek(password: currentPassword, params: params)
        let next = try deriveAuthAndKek(password: newPassword, params: params)
        let body = ChangePasswordRequest(
            current_auth_hash: cur.authHashB64,
            new_auth_hash: next.authHashB64,
            current_user_kek: cur.userKekB64,
            new_user_kek: next.userKekB64,
            new_kek_salt: nil
        )
        _ = try await request("/auth/change-password", method: "POST",
                              body: body) as EmptyResponse
        await UserKekHolder.shared.remember(next.userKekBytes)
    }

    func logout() async {
        if let rt = refreshToken {
            _ = try? await request("/auth/logout", method: "POST",
                body: ["refresh_token": rt], retry: false) as EmptyResponse
        }
        clearSession()
        await UserKekHolder.shared.forget()
    }

    // MARK: - Projects
    func listProjects() async throws -> [Project] {
        try await request("/projects")
    }

    // MARK: - Conversations
    func listConversations(projectId: String) async throws -> [Conversation] {
        try await request("/projects/\(projectId)/conversations")
    }
    func getConversation(projectId: String, convId: String) async throws -> ConversationWithMessages {
        try await request("/projects/\(projectId)/conversations/\(convId)")
    }

    // MARK: - Roadmap (per-project)
    func listTasks(projectId: String, sprintId: String? = nil) async throws -> [ProjectTask] {
        var path = "/projects/\(projectId)/tasks"
        if let s = sprintId { path += "?sprint_id=\(s)" }
        return try await request(path)
    }
    func listSprints(projectId: String) async throws -> [Sprint] {
        try await request("/projects/\(projectId)/sprints")
    }

    // MARK: - Cross-project user views
    func listUserTasks() async throws -> [UserTask] {
        try await request("/user/tasks")
    }
    func listEpics() async throws -> [Epic] {
        try await request("/epics")
    }

    // MARK: - Agents
    func listAgents() async throws -> [AgentProfile] {
        try await request("/agents")
    }
}

struct ConversationWithMessages: Decodable {
    let id: String
    let title: String
    let messages: [Message]
}

struct LoginRequest: Encodable {
    let email: String
    let auth_hash: String
    let user_kek: String
}
struct RegisterRequest: Encodable {
    let email: String
    let display_name: String
    let kek_salt: String
    let auth_hash: String
    let user_kek: String
}
struct ChangePasswordRequest: Encodable {
    let current_auth_hash: String
    let new_auth_hash: String
    let current_user_kek: String
    let new_user_kek: String
    let new_kek_salt: String?
}
struct ErrorResponse: Decodable { let error: String }
/// Marker for endpoints that return 204 / no JSON body.
struct EmptyResponse: Decodable {}

enum APIError: LocalizedError {
    case invalidURL
    case unauthorized
    case serverError(String)
    var errorDescription: String? {
        switch self {
        case .invalidURL: return "Invalid URL"
        case .unauthorized: return "Session expired — please sign in again"
        case .serverError(let msg): return msg
        }
    }
}
