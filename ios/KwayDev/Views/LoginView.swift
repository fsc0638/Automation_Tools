import SwiftUI

struct LoginView: View {
    @EnvironmentObject var authStore: AuthStore
    @State private var email = ""
    @State private var password = ""
    @State private var isLoading = false
    @State private var errorMessage = ""
    @State private var showRegister = false

    var body: some View {
        NavigationStack {
            ZStack {
                Color(red: 0.973, green: 0.976, blue: 0.980).ignoresSafeArea()

                VStack(spacing: 0) {
                    Spacer()

                    // Logo
                    VStack(spacing: 12) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 16)
                                .fill(Color(red: 0.0, green: 0.176, blue: 0.384))
                                .frame(width: 64, height: 64)
                            Text("K")
                                .font(.system(size: 32, weight: .bold))
                                .foregroundColor(.white)
                        }
                        Text("Kway Dev")
                            .font(.system(size: 28, weight: .bold))
                            .foregroundColor(Color(red: 0.0, green: 0.176, blue: 0.384))
                        Text("AI-powered development platform")
                            .font(.subheadline)
                            .foregroundColor(.secondary)
                    }
                    .padding(.bottom, 40)

                    // Card
                    VStack(alignment: .leading, spacing: 20) {
                        Text("Sign in")
                            .font(.title3.bold())

                        VStack(spacing: 14) {
                            TextField("Email", text: $email)
                                .textContentType(.emailAddress)
                                .keyboardType(.emailAddress)
                                .autocapitalization(.none)
                                .fieldStyle()
                            SecureField("Password", text: $password)
                                .textContentType(.password)
                                .fieldStyle()
                        }

                        if !errorMessage.isEmpty {
                            Text(errorMessage)
                                .font(.caption)
                                .foregroundColor(.red)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 8)
                                .background(Color.red.opacity(0.08))
                                .cornerRadius(8)
                        }

                        Button(action: handleLogin) {
                            HStack {
                                if isLoading { ProgressView().tint(.white) }
                                Text("Sign In")
                                    .fontWeight(.semibold)
                            }
                            .frame(maxWidth: .infinity)
                            .frame(height: 48)
                            .background(Color(red: 0.0, green: 0.314, blue: 0.627))
                            .foregroundColor(.white)
                            .cornerRadius(12)
                        }
                        .disabled(isLoading || email.isEmpty || password.isEmpty)
                        .opacity(isLoading || email.isEmpty || password.isEmpty ? 0.6 : 1)

                        HStack {
                            Spacer()
                            Button("Create account") { showRegister = true }
                                .font(.subheadline)
                                .foregroundColor(Color(red: 0.0, green: 0.314, blue: 0.627))
                            Spacer()
                        }
                    }
                    .padding(28)
                    .background(.white)
                    .cornerRadius(20)
                    .shadow(color: .black.opacity(0.06), radius: 20, x: 0, y: 4)
                    .padding(.horizontal, 24)

                    Spacer()
                }
            }
            .navigationDestination(isPresented: $showRegister) {
                RegisterView()
            }
        }
    }

    private func handleLogin() {
        errorMessage = ""
        isLoading = true
        Task {
            do {
                try await authStore.login(email: email, password: password)
            } catch {
                errorMessage = error.localizedDescription
            }
            isLoading = false
        }
    }
}

struct RegisterView: View {
    @EnvironmentObject var authStore: AuthStore
    @Environment(\.dismiss) var dismiss
    @State private var displayName = ""
    @State private var email = ""
    @State private var password = ""
    @State private var isLoading = false
    @State private var errorMessage = ""

    var body: some View {
        ZStack {
            Color(red: 0.973, green: 0.976, blue: 0.980).ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    Text("Create account")
                        .font(.title3.bold())

                    VStack(spacing: 14) {
                        TextField("Display Name", text: $displayName).fieldStyle()
                        TextField("Email", text: $email)
                            .textContentType(.emailAddress).keyboardType(.emailAddress)
                            .autocapitalization(.none).fieldStyle()
                        SecureField("Password (min 8 chars)", text: $password)
                            .textContentType(.newPassword).fieldStyle()
                    }

                    if !errorMessage.isEmpty {
                        Text(errorMessage).font(.caption).foregroundColor(.red)
                            .padding(10).background(Color.red.opacity(0.08)).cornerRadius(8)
                    }

                    Button(action: handleRegister) {
                        HStack {
                            if isLoading { ProgressView().tint(.white) }
                            Text("Create Account").fontWeight(.semibold)
                        }
                        .frame(maxWidth: .infinity).frame(height: 48)
                        .background(Color(red: 0.0, green: 0.314, blue: 0.627))
                        .foregroundColor(.white).cornerRadius(12)
                    }
                    .disabled(isLoading || displayName.isEmpty || email.isEmpty || password.count < 8)
                    .opacity(isLoading || displayName.isEmpty || email.isEmpty || password.count < 8 ? 0.6 : 1)
                }
                .padding(28)
                .background(.white).cornerRadius(20)
                .shadow(color: .black.opacity(0.06), radius: 20, x: 0, y: 4)
                .padding(24)
            }
        }
        .navigationTitle("Register")
    }

    private func handleRegister() {
        errorMessage = ""
        isLoading = true
        Task {
            do {
                try await authStore.register(email: email, password: password, displayName: displayName)
            } catch {
                errorMessage = error.localizedDescription
            }
            isLoading = false
        }
    }
}

extension View {
    func fieldStyle() -> some View {
        self
            .padding(.horizontal, 14)
            .frame(height: 48)
            .background(Color(red: 0.973, green: 0.976, blue: 0.980))
            .cornerRadius(10)
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color(red: 0.886, green: 0.906, blue: 0.941), lineWidth: 1))
    }
}
