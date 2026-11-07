import SwiftUI

@main
struct MachinaApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        Window("Machina", id: "main") {
            Group {
                if model.phase == .ready { MainWindow() } else { LoginView() }
            }
            .environment(model)
            .frame(minWidth: 760, minHeight: 480)
        }
        MenuBarExtra("Machina", systemImage: "server.rack") {
            MenuBarContent().environment(model)
        }
    }
}
