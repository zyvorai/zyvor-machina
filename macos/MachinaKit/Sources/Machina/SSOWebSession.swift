import AppKit
import AuthenticationServices
import MachinaKit

/// Runs the system web authentication sheet for the daemon's native SSO start URL and returns the
/// `machina://auth?...` URL it ends on. All validation of that URL happens in `NativeSSO.parseCallback`.
@MainActor
enum SSOWebSession {
    private final class Anchor: NSObject, ASWebAuthenticationPresentationContextProviding {
        func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
            NSApp.keyWindow ?? NSApp.windows.first ?? ASPresentationAnchor()
        }
    }

    private static var active: (session: ASWebAuthenticationSession, anchor: Anchor)?

    static func run(startURL: URL) async throws -> URL {
        try await withCheckedThrowingContinuation { cont in
            let anchor = Anchor()
            let session = ASWebAuthenticationSession(url: startURL, callbackURLScheme: NativeSSO.callbackScheme) { url, error in
                Task { @MainActor in
                    active = nil
                    if let url {
                        cont.resume(returning: url)
                    } else if let e = error as? ASWebAuthenticationSessionError, e.code == .canceledLogin {
                        cont.resume(throwing: NativeSSOError.cancelled)
                    } else {
                        cont.resume(throwing: error ?? NativeSSOError.cancelled)
                    }
                }
            }
            session.presentationContextProvider = anchor
            // Shared browser session, so an identity provider the user is already signed in to can skip its prompt.
            session.prefersEphemeralWebBrowserSession = false
            active = (session, anchor)
            if !session.start() {
                active = nil
                cont.resume(throwing: NativeSSOError.cancelled)
            }
        }
    }
}
