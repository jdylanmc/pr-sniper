import AppKit
import ApplicationServices
import Foundation

enum SmokeFailure: Error, CustomStringConvertible {
    case failed(String)

    var description: String {
        switch self {
        case .failed(let message): return message
        }
    }
}

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw SmokeFailure.failed(message) }
}

func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
        return nil
    }
    return value
}

func elements(_ element: AXUIElement, _ name: String) -> [AXUIElement] {
    attribute(element, name) as? [AXUIElement] ?? []
}

func text(_ element: AXUIElement, _ name: String) -> String {
    attribute(element, name) as? String ?? ""
}

func descendants(_ element: AXUIElement, depth: Int = 0) -> [AXUIElement] {
    if depth == 12 { return [] }
    var children = elements(element, kAXChildrenAttribute)
    if depth == 0 {
        for name in [kAXMenuBarAttribute, "AXExtrasMenuBar"] {
            if let value = attribute(element, name),
               CFGetTypeID(value) == AXUIElementGetTypeID() {
                children.append(unsafeBitCast(value, to: AXUIElement.self))
            }
        }
    }
    return [element] + children.flatMap {
        descendants($0, depth: depth + 1)
    }
}

func waitFor(_ message: String, _ predicate: () -> Bool) throws {
    let deadline = Date().addingTimeInterval(10)
    while Date() < deadline {
        if predicate() { return }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    throw SmokeFailure.failed("Timed out: \(message)")
}

func press(_ element: AXUIElement, _ description: String) throws {
    let result = AXUIElementPerformAction(element, kAXPressAction as CFString)
    try require(result == .success, "\(description): Accessibility action failed (\(result.rawValue))")
}

func visibleWindows(_ pid: pid_t) -> [[String: Any]] {
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements],
                                           kCGNullWindowID) as? [[String: Any]] ?? []
    return windows.filter {
        ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid)
            && ($0[kCGWindowLayer as String] as? Int) == 0
    }
}

func menuItem(_ application: AXUIElement, title: String) -> AXUIElement? {
    descendants(application).first {
        text($0, kAXRoleAttribute) == kAXMenuItemRole && text($0, kAXTitleAttribute) == title
    }
}

func openTray(_ application: AXUIElement) throws {
    if menuItem(application, title: "Quit PR Sniper") != nil { return }
    let items = descendants(application).filter {
        text($0, kAXRoleAttribute) == kAXMenuBarItemRole
    }
    for item in items.reversed() {
        try press(item, "open native tray")
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        if menuItem(application, title: "Quit PR Sniper") != nil { return }
        _ = AXUIElementPerformAction(item, kAXCancelAction as CFString)
    }
    throw SmokeFailure.failed("No application-owned tray menu exposes Quit PR Sniper")
}

func choose(_ application: AXUIElement, title: String) throws {
    try openTray(application)
    guard let item = menuItem(application, title: title) else {
        throw SmokeFailure.failed("Tray entry missing: \(title)")
    }
    try press(item, title)
}

func preflight() throws {
    try require(AXIsProcessTrusted(), "Accessibility permission absent; native smoke UNVERIFIED")
    try require(CGPreflightScreenCaptureAccess(),
                "Screen Recording permission absent; window observation UNVERIFIED")
}

func run() throws {
    if CommandLine.arguments.dropFirst() == ["--preflight"] {
        try preflight()
        print("PASS native automation permissions available (no app launched)")
        return
    }
    try require(CommandLine.arguments.count == 2,
                "Usage: swift tests/macos-native-smoke.swift --preflight | /absolute/PR Sniper.app")
    try preflight()
    let bundleURL = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
    guard let bundle = Bundle(url: bundleURL),
          let identifier = bundle.bundleIdentifier,
          let executable = bundle.executableURL else {
        throw SmokeFailure.failed("Not an executable application bundle: \(bundleURL.path)")
    }
    try require(identifier.contains("pr-sniper") || identifier.contains("prsniper"),
                "Refusing a bundle not identified as PR Sniper")
    try require(NSRunningApplication.runningApplications(withBundleIdentifier: identifier).isEmpty,
                "Another PR Sniper instance exists; coordinate its owner before testing")

    let fixture = FileManager.default.temporaryDirectory
        .appendingPathComponent("pr-sniper-native-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: fixture, withIntermediateDirectories: false)
    let process = Process()
    process.executableURL = executable
    process.environment = ProcessInfo.processInfo.environment.merging([
        "PR_SNIPER_DATA_DIR": fixture.path
    ]) { _, new in new }
    defer {
        if process.isRunning {
            process.terminate()
            let deadline = Date().addingTimeInterval(5)
            while process.isRunning && Date() < deadline {
                RunLoop.current.run(until: Date().addingTimeInterval(0.1))
            }
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
            process.waitUntilExit()
            print("CLEANUP terminated test-owned PID \(process.processIdentifier); not a Quit pass")
        }
        do {
            try FileManager.default.removeItem(at: fixture)
        } catch {
            fputs("CLEANUP failed for \(fixture.path): \(error)\n", stderr)
        }
    }
    try process.run()
    let pid = process.processIdentifier
    print("OBSERVE bundle=\(bundleURL.path) pid=\(pid) isolatedData=\(fixture.path)")
    let application = AXUIElementCreateApplication(pid)
    try waitFor("native application startup") {
        process.isRunning && descendants(application).count > 1
    }
    RunLoop.current.run(until: Date().addingTimeInterval(1))
    try require(process.isRunning && visibleWindows(pid).isEmpty,
                "Startup must retain the process without a visible main window")
    print("PASS launch has no persistent main window")

    try openTray(application)
    for title in ["Review Queue", "Settings", "Status", "Setup Doctor", "Quit PR Sniper"] {
        try require(menuItem(application, title: title) != nil, "Missing tray entry: \(title)")
    }
    print("PASS native tray exposes all foundation entry points")

    for title in ["Settings", "Review Queue"] {
        for _ in 0..<2 {
            try choose(application, title: title)
            let windowTitle = "PR Sniper - \(title)"
            try waitFor("\(title) visible") {
                visibleWindows(pid).contains {
                    ($0[kCGWindowName as String] as? String) == windowTitle
                }
            }
            guard let window = elements(application, kAXWindowsAttribute).first(where: {
                text($0, kAXTitleAttribute) == windowTitle
            }),
                  let closeValue = attribute(window, kAXCloseButtonAttribute),
                  CFGetTypeID(closeValue) == AXUIElementGetTypeID() else {
                throw SmokeFailure.failed("\(title) native close button unavailable")
            }
            try press(unsafeBitCast(closeValue, to: AXUIElement.self), "close \(title)")
            try waitFor("\(title) closed") { visibleWindows(pid).isEmpty }
            try require(process.isRunning, "Closing \(title) terminated the tray process")
        }
        print("PASS \(title) closes and reopens without ending PID \(pid)")
    }

    try choose(application, title: "Quit PR Sniper")
    try waitFor("tray Quit ends owned process") { !process.isRunning }
    try require(process.terminationStatus == 0, "Quit returned a nonzero exit status")
    print("PASS native tray Quit ends owned PID \(pid)")
    print("UNVERIFIED visual icon, diagnostics content, login-item state and child-work cleanup: use acceptance procedure")
}

do {
    try run()
} catch {
    fputs("FAIL \(error)\n", stderr)
    exit(1)
}
