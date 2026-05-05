import Foundation
import Combine

@MainActor
class AuthStore: ObservableObject {
    @Published var user: User?
    @Published var isAuthenticated = false

    private let tokenKey = "kway_token"

    init() {
        isAuthenticated = UserDefaults.standard.string(forKey: tokenKey) != nil
    }

    func login(email: String, password: String) async throws {
        let res = try await APIService.shared.login(email: email, password: password)
        UserDefaults.standard.set(res.accessToken, forKey: tokenKey)
        user = res.user
        isAuthenticated = true
    }

    func register(email: String, password: String, displayName: String) async throws {
        let res = try await APIService.shared.register(email: email, password: password, displayName: displayName)
        UserDefaults.standard.set(res.accessToken, forKey: tokenKey)
        user = res.user
        isAuthenticated = true
    }

    func logout() {
        UserDefaults.standard.removeObject(forKey: tokenKey)
        user = nil
        isAuthenticated = false
    }
}
