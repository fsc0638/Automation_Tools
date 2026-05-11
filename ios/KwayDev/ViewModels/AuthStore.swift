import Foundation
import Combine

@MainActor
class AuthStore: ObservableObject {
    @Published var user: User?
    @Published var isAuthenticated = false

    private let tokenKey = "kway_token"

    init() {
        // Authenticated state is driven by the presence of an access
        // token. The refresh-token flow lives entirely inside APIService;
        // the store doesn't need to know about it.
        isAuthenticated = UserDefaults.standard.string(forKey: tokenKey) != nil
    }

    func login(email: String, password: String) async throws {
        let res = try await APIService.shared.login(email: email, password: password)
        user = res.user
        isAuthenticated = true
    }

    func register(email: String, password: String, displayName: String) async throws {
        let res = try await APIService.shared.register(email: email, password: password, displayName: displayName)
        user = res.user
        isAuthenticated = true
    }

    func logout() {
        Task { await APIService.shared.logout() }
        APIService.shared.clearSession()
        user = nil
        isAuthenticated = false
    }
}
