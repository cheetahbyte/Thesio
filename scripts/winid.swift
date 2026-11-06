// Prints the window ids of the app's windows on screen, for `screencapture -l`.
// Usage: swift scripts/winid.swift [process name, default thesio]
import Cocoa

let name = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "thesio"
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    guard let owner = w[kCGWindowOwnerName as String] as? String, owner.lowercased().contains(name.lowercased()),
          let id = w[kCGWindowNumber as String] as? Int, let layer = w[kCGWindowLayer as String] as? Int, layer == 0 else { continue }
    print(id)
}
