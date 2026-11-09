import SwiftUI
import AppKit
import MachinaKit

/// Sign-in screen: gradient backdrop, a frosted card, saved-server chips, and a certificate-trust sheet.
struct LoginView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var password = ""
    @State private var showPassword = false
    @State private var shake: CGFloat = 0
    @FocusState private var focus: Field?
    enum Field { case server, user, password }

    private var busy: Bool { model.phase == .connecting }
    private var urlLooksValid: Bool {
        guard let u = URL(string: model.serverURL.trimmingCharacters(in: .whitespaces)), let h = u.host, !h.isEmpty else { return false }
        return u.scheme == "https" || u.scheme == "http"
    }
    private var canSubmit: Bool { urlLooksValid && !model.username.isEmpty && !password.isEmpty && !busy }

    var body: some View {
        @Bindable var model = model
        ZStack {
            LoginBackdrop(animated: !reduceMotion)
            VStack(spacing: 22) {
                header
                if !model.servers.isEmpty { savedServers }
                VStack(spacing: 10) {
                    LoginField(icon: "server.rack", prompt: "Server  (https://host:5092)") {
                        TextField("Server  (https://host:5092)", text: $model.serverURL).textContentType(.URL).focused($focus, equals: .server).onSubmit { focus = .user }
                    }
                    LoginField(icon: "person", prompt: "Username") {
                        TextField("Username", text: $model.username).textContentType(.username).focused($focus, equals: .user).onSubmit { focus = .password }
                    }
                    LoginField(icon: "lock", prompt: "Password") {
                        Group {
                            if showPassword { TextField("Password", text: $password) } else { SecureField("Password", text: $password) }
                        }
                        .textContentType(.password).focused($focus, equals: .password).onSubmit(go)
                    } trailing: {
                        Button { showPassword.toggle() } label: { Image(systemName: showPassword ? "eye.slash" : "eye").foregroundStyle(.secondary) }
                            .buttonStyle(.plain).help(showPassword ? String(localized: "Hide password") : String(localized: "Show password"))
                            .accessibilityLabel(showPassword ? String(localized: "Hide password") : String(localized: "Show password"))
                    }
                }
                if let p = model.providers, p.hasSSO { ssoNote(p) }
                if let e = model.error { errorBanner(e) }
                Button(action: go) {
                    ZStack {
                        Text("Sign In").opacity(busy ? 0 : 1)
                        if busy { HStack(spacing: 8) { ProgressView().controlSize(.small); Text("Connecting…") } }
                    }
                    .font(.headline).frame(maxWidth: .infinity).frame(height: 30)
                }
                .buttonStyle(.borderedProminent).controlSize(.large).tint(Color(red: 0, green: 0.443, blue: 0.89))
                .keyboardShortcut(.defaultAction).disabled(!canSubmit)
                footer
            }
            .padding(32).frame(width: 400)
            .background(.regularMaterial, in: .rect(cornerRadius: 24))
            .overlay(RoundedRectangle(cornerRadius: 24).strokeBorder(.white.opacity(0.18), lineWidth: 1))
            .shadow(color: .black.opacity(0.22), radius: 30, y: 14)
            .modifier(ShakeEffect(travel: shake))
        }
        .onAppear { focus = model.serverURL.count > 8 && !model.username.isEmpty ? .password : (model.serverURL.count > 8 ? .user : .server) }
        .task(id: model.serverURL) {
            try? await Task.sleep(nanoseconds: 600_000_000)   // debounce typing
            if !Task.isCancelled, urlLooksValid { await model.probeProviders() }
        }
        .onChange(of: model.error) { _, new in
            if new != nil, !reduceMotion { withAnimation(.linear(duration: 0.45)) { shake += 1 } }
        }
        .sheet(isPresented: Binding(get: { model.pendingFingerprint != nil }, set: { if !$0 { model.resolveTrust(false) } })) {
            TrustSheet(fingerprint: model.pendingFingerprint ?? "", host: URL(string: model.serverURL)?.host ?? "this server").environment(model)
        }
    }

    // MARK: pieces

    private var header: some View {
        VStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .fill(LinearGradient(colors: [Color(red: 0.16, green: 0.60, blue: 1), Color(red: 0, green: 0.443, blue: 0.89)], startPoint: .top, endPoint: .bottom))
                Text("m").font(.system(size: 44, weight: .semibold, design: .rounded)).foregroundStyle(.white).offset(y: -2)
            }
            .frame(width: 68, height: 68).shadow(color: Color(red: 0, green: 0.443, blue: 0.89).opacity(0.45), radius: 14, y: 6)
            .accessibilityHidden(true)
            Text("machina").font(.system(size: 34, weight: .semibold, design: .rounded)).tracking(-0.5)
            Text("Sign in to your hypervisor").font(.callout).foregroundStyle(.secondary)
        }
    }

    private var savedServers: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("RECENT").font(.caption2.weight(.semibold)).foregroundStyle(.secondary).tracking(0.8)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(model.servers.prefix(6)) { p in
                        Button { Task { await model.switchTo(p) } } label: {
                            HStack(spacing: 6) {
                                Image(systemName: model.hasSavedPassword(p) ? "bolt.fill" : "server.rack").font(.caption)
                                    .foregroundStyle(model.hasSavedPassword(p) ? Color.accentColor : .secondary)
                                Text(p.name + (p.username.map { " · \($0)" } ?? "")).font(.callout).lineLimit(1)
                            }
                            .padding(.horizontal, 12).padding(.vertical, 7)
                            .background(.quaternary.opacity(0.6), in: .capsule)
                        }
                        .buttonStyle(.plain).disabled(busy)
                        .help(model.hasSavedPassword(p) ? "Sign in with the saved password" : "Fill in this server; you will need to enter the password")
                        .contextMenu { Button("Forget this server", role: .destructive) { model.forget(p) } }
                    }
                }
            }
        }
    }

    /// OIDC works through the daemon's native code exchange (`machina://auth`). A daemon without it, and
    /// SAML (no login flow in the daemon), get an explanation instead of a button that cannot work.
    @ViewBuilder private func ssoNote(_ p: AuthProviders) -> some View {
        if p.canSignInWithSSO {
            let name = p.oidcLabel ?? "SSO"
            Button { Task { await model.signInWithSSO() } } label: {
                Label(String(localized: "Sign in with \(name)"), systemImage: "person.badge.key").frame(maxWidth: .infinity).frame(height: 24)
            }
            .buttonStyle(.bordered).controlSize(.large).disabled(!urlLooksValid || busy)
        } else {
            let name = p.oidc ? (p.oidcLabel ?? "OIDC") : (p.samlLabel ?? "SAML")
            let text = p.ssoServerTooOld
                ? String(localized: "\(name) sign-in is enabled, but this server is too old for single sign-on from Machina for Mac. Update the daemon, or use a local account.")
                : (p.ssoOnly ? String(localized: "This server only offers \(name) sign-in, which Machina for Mac cannot use yet. Ask for a local account or use the web UI.")
                             : String(localized: "\(name) sign-in is enabled on this server but works only in the web UI. Use a local account here."))
            Label(text, systemImage: "info.circle")
                .font(.caption).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func errorBanner(_ text: String) -> some View {
        Label(text, systemImage: "exclamationmark.triangle.fill")
            .font(.callout).foregroundStyle(.red).lineLimit(3)
            .padding(10).frame(maxWidth: .infinity, alignment: .leading)
            .background(.red.opacity(0.12), in: .rect(cornerRadius: 10))
            .transition(.opacity.combined(with: .move(edge: .top)))
    }

    private var footer: some View {
        VStack(spacing: 4) {
            Label("Your password is kept in the macOS Keychain.", systemImage: "key.fill").font(.caption).foregroundStyle(.secondary)
            Text("Self-signed certificates are confirmed the first time you connect.").font(.caption2).foregroundStyle(.tertiary)
        }.multilineTextAlignment(.center)
    }

    private func go() { guard canSubmit else { return }; Task { await model.signIn(password: password) } }
}

/// One rounded input row: leading icon, the field, an optional trailing control.
private struct LoginField<Field: View, Trailing: View>: View {
    let icon: String
    let prompt: LocalizedStringKey
    @ViewBuilder var field: Field
    @ViewBuilder var trailing: Trailing

    init(icon: String, prompt: LocalizedStringKey, @ViewBuilder field: () -> Field, @ViewBuilder trailing: () -> Trailing = { EmptyView() }) {
        self.icon = icon; self.prompt = prompt; self.field = field(); self.trailing = trailing()
    }

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: icon).foregroundStyle(.secondary).frame(width: 18)
            field.textFieldStyle(.plain).accessibilityLabel(prompt)
            trailing
        }
        .padding(.horizontal, 12).frame(height: 42)
        .background(.background.opacity(0.55), in: .rect(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).strokeBorder(.primary.opacity(0.12)))
    }
}

/// Slowly drifting colour blobs behind the card; static when Reduce Motion is on.
private struct LoginBackdrop: View {
    let animated: Bool
    @State private var drift = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack {
            LinearGradient(colors: scheme == .dark ? [Color(red: 0.05, green: 0.07, blue: 0.13), Color(red: 0.02, green: 0.03, blue: 0.07)]
                                                   : [Color(red: 0.94, green: 0.96, blue: 1), Color(red: 0.88, green: 0.92, blue: 0.99)],
                           startPoint: .topLeading, endPoint: .bottomTrailing)
            blob(Color(red: 0, green: 0.443, blue: 0.89), size: 460, x: drift ? -230 : -170, y: drift ? -150 : -110)
            blob(Color(red: 0.35, green: 0.35, blue: 0.95), size: 380, x: drift ? 250 : 200, y: drift ? 160 : 120)
            blob(Color(red: 0.1, green: 0.75, blue: 0.9), size: 300, x: drift ? 180 : 230, y: drift ? -170 : -130)
        }
        .ignoresSafeArea()
        .onAppear { if animated { withAnimation(.easeInOut(duration: 9).repeatForever(autoreverses: true)) { drift = true } } }
    }

    private func blob(_ c: Color, size: CGFloat, x: CGFloat, y: CGFloat) -> some View {
        Circle().fill(c.opacity(scheme == .dark ? 0.30 : 0.28)).frame(width: size, height: size).blur(radius: 90).offset(x: x, y: y)
    }
}

private struct ShakeEffect: GeometryEffect {
    var travel: CGFloat
    var animatableData: CGFloat { get { travel } set { travel = newValue } }
    func effectValue(size: CGSize) -> ProjectionTransform {
        ProjectionTransform(CGAffineTransform(translationX: 9 * sin(travel * .pi * 4), y: 0))
    }
}

/// Shown on the first connect to a server with a self-signed certificate.
private struct TrustSheet: View {
    @Environment(AppModel.self) private var model
    let fingerprint: String
    let host: String

    /// "dbd682e1…" becomes four lines of "DB:D6:82:E1:…" so it can be compared by eye.
    private var grouped: String {
        let bytes = stride(from: 0, to: fingerprint.count - 1, by: 2).map { i -> String in
            let s = fingerprint.index(fingerprint.startIndex, offsetBy: i)
            return String(fingerprint[s..<fingerprint.index(s, offsetBy: 2)]).uppercased()
        }
        return stride(from: 0, to: bytes.count, by: 8).map { bytes[$0..<min($0 + 8, bytes.count)].joined(separator: ":") }.joined(separator: "\n")
    }

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "lock.trianglebadge.exclamationmark.fill").font(.system(size: 34)).foregroundStyle(.orange)
            Text("Trust this certificate?").font(.title3.weight(.semibold))
            Text("\(host) uses a self-signed certificate. Check that the fingerprint below matches what the server's administrator gave you, then trust it. It is remembered, and a different certificate later will be refused.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Text(grouped).font(.system(.callout, design: .monospaced)).textSelection(.enabled)
                .padding(12).frame(maxWidth: .infinity).background(.quaternary.opacity(0.6), in: .rect(cornerRadius: 10))
            Text("SHA-256 fingerprint").font(.caption2).foregroundStyle(.tertiary).offset(y: -8)
            HStack {
                Button("Copy fingerprint") { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(fingerprint, forType: .string) }
                Spacer()
                Button("Cancel", role: .cancel) { model.resolveTrust(false) }.keyboardShortcut(.cancelAction)
                Button("Trust") { model.resolveTrust(true) }.keyboardShortcut(.defaultAction).buttonStyle(.borderedProminent)
            }
        }
        .padding(26).frame(width: 440)
    }
}
