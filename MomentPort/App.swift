import SwiftUI

@main
struct MomentPortApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .frame(minWidth: 860, minHeight: 620)
        }
        .windowStyle(.titleBar)
        .commands {
            CommandGroup(replacing: .newItem) {}
        }
    }
}
