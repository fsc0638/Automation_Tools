import SwiftUI

@main
struct KwayDevApp: App {
    @StateObject private var authStore = AuthStore()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(authStore)
        }
    }
}

struct RootView: View {
    @EnvironmentObject var authStore: AuthStore

    var body: some View {
        Group {
            if authStore.isAuthenticated {
                MainTabView()
            } else {
                LoginView()
            }
        }
        .animation(.easeInOut, value: authStore.isAuthenticated)
    }
}
