import AppKit
import ApplicationServices
import Foundation
import Security

enum SmokeFailure: Error, CustomStringConvertible {
    case failed(String)
    var description: String {
        switch self { case .failed(let message): return message }
    }
}

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw SmokeFailure.failed(message) }
}

func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
    return value
}

func elements(_ element: AXUIElement, _ name: String) -> [AXUIElement] {
    attribute(element, name) as? [AXUIElement] ?? []
}

func element(_ parent: AXUIElement, _ name: String) -> AXUIElement? {
    guard let value = attribute(parent, name), CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
}

func text(_ element: AXUIElement, _ name: String) -> String {
    attribute(element, name) as? String ?? ""
}

func descendants(_ value: AXUIElement, depth: Int = 0) -> [AXUIElement] {
    if depth == 40 { return [] }
    var children = elements(value, kAXChildrenAttribute)
    if depth == 0 {
        for name in [kAXMenuBarAttribute, "AXExtrasMenuBar"] {
            if let extra = element(value, name) { children.append(extra) }
        }
    }
    return [value] + children.flatMap { descendants($0, depth: depth + 1) }
}

func waitFor(_ message: String, _ predicate: () -> Bool) throws {
    let deadline = Date().addingTimeInterval(10)
    while Date() < deadline {
        if predicate() { return }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    throw SmokeFailure.failed("Timed out: \(message)")
}

func press(_ value: AXUIElement, _ description: String) throws {
    let result = AXUIElementPerformAction(value, kAXPressAction as CFString)
    try require(result == .success, "\(description): Accessibility action failed (\(result.rawValue))")
}

func named(_ value: AXUIElement, _ title: String) -> Bool {
    [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].contains { text(value, $0) == title }
}

func button(_ window: AXUIElement, _ title: String) throws -> AXUIElement {
    var found: AXUIElement?
    try waitFor("button \(title)") {
        found = descendants(window).first { text($0, kAXRoleAttribute) == kAXButtonRole && named($0, title) }
        return found != nil
    }
    return found!
}

func visibleWindows(_ pid: pid_t) -> [[String: Any]] {
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements],
                                           kCGNullWindowID) as? [[String: Any]] ?? []
    return windows.filter {
        ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid)
            && ($0[kCGWindowLayer as String] as? Int).map {
                $0 >= 0 && $0 <= Int(CGWindowLevelForKey(.floatingWindow))
            } == true
    }
}

func center(_ value: AXUIElement) throws -> CGPoint {
    guard let position = attribute(value, kAXPositionAttribute),
          let size = attribute(value, kAXSizeAttribute),
          CFGetTypeID(position) == AXValueGetTypeID(), CFGetTypeID(size) == AXValueGetTypeID() else {
        throw SmokeFailure.failed("Owned native element has no observable frame")
    }
    var point = CGPoint.zero
    var extent = CGSize.zero
    try require(AXValueGetValue(unsafeBitCast(position, to: AXValue.self), .cgPoint, &point)
        && AXValueGetValue(unsafeBitCast(size, to: AXValue.self), .cgSize, &extent)
        && extent.width > 0 && extent.height > 0, "Owned native element has invalid geometry")
    return CGPoint(x: point.x + extent.width / 2, y: point.y + extent.height / 2)
}

func click(_ point: CGPoint, right: Bool = false) throws {
    let mouse: CGMouseButton = right ? .right : .left
    for type: CGEventType in right ? [.rightMouseDown, .rightMouseUp] : [.leftMouseDown, .leftMouseUp] {
        guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: mouse) else {
            throw SmokeFailure.failed("Cannot construct owned-target mouse event")
        }
        event.post(tap: .cghidEventTap)
    }
}

func key(_ pid: pid_t, _ code: CGKeyCode, command: Bool = false) throws {
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else {
            throw SmokeFailure.failed("Cannot construct owned-process key event")
        }
        if command { event.flags = .maskCommand }
        event.postToPid(pid)
    }
}

func trayItem(_ application: AXUIElement) throws -> AXUIElement {
    var found: AXUIElement?
    try waitFor("application-owned PR Sniper tray item") {
        let extras = element(application, "AXExtrasMenuBar")
        let candidates = descendants(extras ?? application).filter { text($0, kAXRoleAttribute) == kAXMenuBarItemRole }
        found = candidates.first { named($0, "PR Sniper") || text($0, kAXHelpAttribute) == "PR Sniper" }
        if found == nil && extras != nil && candidates.count == 1 { found = candidates[0] }
        return found != nil
    }
    return found!
}

func menuItem(_ application: AXUIElement, _ title: String) -> AXUIElement? {
    descendants(application).first { text($0, kAXRoleAttribute) == kAXMenuItemRole && named($0, title) }
}

func choose(_ application: AXUIElement, _ title: String) throws {
    try click(center(trayItem(application)), right: true)
    try waitFor("secondary tray menu") { menuItem(application, "Quit PR Sniper") != nil }
    for required in ["Review Queue", "Settings", "Status", "Diagnostics", "Check Now", "Quit PR Sniper"] {
        try require(menuItem(application, required) != nil, "Missing secondary tray entry: \(required)")
    }
    try press(menuItem(application, title)!, title)
}

func deleteKeychainService(_ service: String) {
    let status = SecItemDelete([kSecClass: kSecClassGenericPassword, kSecAttrService: service] as CFDictionary)
    if status != errSecSuccess && status != errSecItemNotFound {
        fputs("CLEANUP failed for owned Keychain service \(service): \(status)\n", stderr)
    }
}

func seed(_ bridge: URL, _ root: URL) throws {
    func invoke(_ command: String, _ args: [String: Any]) throws {
        let process = Process()
        let input = Pipe(), output = Pipe()
        process.executableURL = bridge
        process.arguments = [root.path]
        process.standardInput = input
        process.standardOutput = output
        try process.run()
        input.fileHandleForWriting.write(try JSONSerialization.data(withJSONObject: ["command": command, "args": args]))
        try input.fileHandleForWriting.close()
        let bytes = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        let result = try JSONSerialization.jsonObject(with: bytes) as? [String: Any]
        try require(process.terminationStatus == 0 && result?["error"] == nil && result?["ok"] != nil,
                    "Offline native fixture seeding failed")
    }
    let repo = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
    let assignment = "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
    let agent = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
    try invoke("seed_settings", [
        "launch_at_login": false, "doctrines": [],
        "agents": [["id": agent, "name": "Offline fixture", "model": "offline", "prompt": "Offline fixture", "signature": "fixture"]],
        "repositories": [["id": repo, "provider": "github", "name": "example/repo", "enabled": false,
                          "provider_account_id": "22", "provider_repository_id": "100",
                          "assignments": [["id": assignment, "agent_id": agent, "comment": false,
                                           "schedule": ["kind": "cron", "expression": "*/15 * * * *", "timezone": "UTC"]]]]]
    ])
    let jobs = [1, 2].map { number -> [String: Any] in
        ["assignment_id": assignment, "provider": "github", "account_id": "22", "account_login": "offline",
         "configuration_id": repo, "repository_id": "100", "repository_name": "example/repo",
         "pull_request_id": "\(number)", "number": number, "title": "Native fixture \(number)",
         "head_sha": String(repeating: "a", count: 40), "observed_base_sha": String(repeating: "b", count: 40),
         "trigger_policy": "[[],true]", "author_id": "11", "author_login": "offline-author",
         "watched_author": false, "all_authors": true, "requested_reviewer": false,
         "waiting": "human_start", "detected_at": 100]
    }
    try invoke("seed_queue_state", ["jobs": jobs, "reviews": [], "publications": [], "follow_ups": []])
}

func preflight() throws {
    try require(AXIsProcessTrusted(), "Accessibility permission absent; native smoke UNVERIFIED")
    try require(CGPreflightScreenCaptureAccess(), "Screen Recording permission absent; window observation UNVERIFIED")
}

func run() throws {
    if CommandLine.arguments.dropFirst() == ["--preflight"] {
        try preflight()
        print("PASS native automation permissions available (no app launched)")
        return
    }
    try require(CommandLine.arguments.count == 3,
                "Usage: native-smoke --preflight | /absolute/test-owned.app /absolute/settings_bridge")
    try preflight()
    let bundleURL = URL(fileURLWithPath: CommandLine.arguments[1]).resolvingSymlinksInPath()
    let bridge = URL(fileURLWithPath: CommandLine.arguments[2]).resolvingSymlinksInPath()
    guard let bundle = Bundle(url: bundleURL), let identifier = bundle.bundleIdentifier,
          let executable = bundle.executableURL else { throw SmokeFailure.failed("Not an executable application bundle") }
    try require(identifier.hasPrefix("com.jdylanmc.pr-sniper.tests."),
                "Refusing production identity; parent must supply a test-owned copy with unique test bundle ID")
    try require(!bundleURL.path.hasPrefix("/Applications/") && !bundleURL.path.hasPrefix(NSHomeDirectory() + "/Applications/"),
                "Refusing an installed application path")
    try require(bridge.lastPathComponent == "settings_bridge" && FileManager.default.isExecutableFile(atPath: bridge.path),
                "Provide the candidate's compiled offline settings_bridge")
    for identity in ["com.jdylanmc.pr-sniper", identifier] {
        try require(NSRunningApplication.runningApplications(withBundleIdentifier: identity).isEmpty,
                    "PR Sniper instance \(identity) exists; coordinate its owner before testing")
    }

    let fixture = FileManager.default.temporaryDirectory.appendingPathComponent("pr-sniper-native-\(UUID().uuidString)", isDirectory: true)
    let keychainService = "com.jdylanmc.pr-sniper.tests.native-\(UUID().uuidString)"
    try FileManager.default.createDirectory(at: fixture, withIntermediateDirectories: false)
    let process = Process()
    defer {
        if process.isRunning {
            process.terminate()
            let deadline = Date().addingTimeInterval(5)
            while process.isRunning && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.1)) }
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
            process.waitUntilExit()
            print("CLEANUP terminated owned PID \(process.processIdentifier); not a Quit pass")
        }
        do { try FileManager.default.removeItem(at: fixture) }
        catch { fputs("CLEANUP failed for \(fixture.path): \(error)\n", stderr) }
        deleteKeychainService(keychainService)
        deleteKeychainService("\(keychainService).legacy")
    }
    try seed(bridge, fixture)
    process.executableURL = executable
    process.environment = ProcessInfo.processInfo.environment.merging([
        "PR_SNIPER_DATA_DIR": fixture.path, "PR_SNIPER_KEYCHAIN_SERVICE": keychainService
    ]) { _, new in new }
    try process.run()
    let pid = process.processIdentifier
    print("OBSERVE bundle=\(bundleURL.path) pid=\(pid) isolatedData=\(fixture.path) isolatedKeychain=\(keychainService)")
    let application = AXUIElementCreateApplication(pid)
    _ = try trayItem(application)
    RunLoop.current.run(until: Date().addingTimeInterval(1))
    try require(process.isRunning && visibleWindows(pid).isEmpty, "Startup must have no visible main window")
    print("PASS hidden startup with owned live process")

    try click(center(trayItem(application)))
    try waitFor("single visible panel") { visibleWindows(pid).count == 1 }
    let windowID = visibleWindows(pid)[0][kCGWindowNumber as String] as? Int
    try require(windowID != nil, "Native panel window identity unavailable")
    var panel: AXUIElement?
    try waitFor("accessible panel") {
        panel = elements(application, kAXWindowsAttribute).first { named($0, "PR Sniper") }
        return panel != nil
    }
    let window = panel!
    func singlePanel() throws {
        let visible = visibleWindows(pid)
        try require(visible.count == 1 && visible[0][kCGWindowNumber as String] as? Int == windowID,
                    "Navigation created or replaced the native panel")
    }
    func reopen() throws {
        try click(center(trayItem(application)))
        try waitFor("retained panel reopened") { visibleWindows(pid).count == 1 }
        try singlePanel()
    }
    func hidden(_ reason: String) throws {
        try waitFor(reason) { visibleWindows(pid).isEmpty }
        try require(process.isRunning, "\(reason) terminated the owned host")
    }
    for tab in ["Queue", "Running", "Reviewed", "Settings"] {
        try press(button(window, tab), tab)
        try waitFor("destination \(tab)") {
            descendants(window).contains { named($0, tab) && [kAXHeadingRole, kAXStaticTextRole].contains(text($0, kAXRoleAttribute)) }
        }
        try singlePanel()
    }
    try press(button(window, "Queue"), "Queue")
    var rowButton: AXUIElement?
    try waitFor("exact fixture row") {
        guard var row = descendants(window).first(where: { named($0, "example/repo #2: Native fixture 2") }) else { return false }
        for _ in 0..<6 {
            if let match = descendants(row).first(where: { text($0, kAXRoleAttribute) == kAXButtonRole && named($0, "Evidence and actions") }) {
                rowButton = match; return true
            }
            guard let parent = element(row, kAXParentAttribute) else { return false }
            row = parent
        }
        return false
    }
    try press(rowButton!, "exact PR #2 detail")
    try waitFor("exact detail title") {
        descendants(window).contains { named($0, "example/repo #2: Native fixture 2") }
            && descendants(window).contains { named($0, "Saved evidence") }
    }
    try press(button(window, "Back"), "Back to exact row")
    try waitFor("exact row keyboard focus after Back") {
        element(application, kAXFocusedUIElementAttribute).map { CFEqual($0, rowButton!) } == true
    }
    print("PASS four destinations, one native window and exact Back row/focus")

    try click(center(trayItem(application)))
    try hidden("tray left-click closes without immediate reopen")
    RunLoop.current.run(until: Date().addingTimeInterval(1))
    try require(visibleWindows(pid).isEmpty, "Tray focus-loss race reopened the panel")
    try reopen()
    try key(pid, 53)
    try hidden("Escape hides")
    try reopen()
    try press(button(window, "Hide PR Sniper panel"), "custom Close")
    try hidden("custom Close hides")
    try reopen()

    // Exercise the host's CloseRequested separately from the renderer hide command.
    if let close = element(window, kAXCloseButtonAttribute) { try press(close, "native close") }
    else if AXUIElementPerformAction(window, "AXClose" as CFString) != .success { try key(pid, 13, command: true) }
    try hidden("native CloseRequested hides")
    try reopen()

    let outside = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 240, height: 120),
                           styleMask: [.titled], backing: .buffered, defer: false)
    outside.title = "PR Sniper smoke owned focus target"
    outside.isReleasedWhenClosed = false
    NSApplication.shared.setActivationPolicy(.regular)
    outside.makeKeyAndOrderFront(nil)
    NSApplication.shared.activate(ignoringOtherApps: true)
    defer { outside.close() }
    let point = CGPoint(x: outside.frame.midX, y: (NSScreen.screens.first?.frame.maxY ?? 0) - outside.frame.midY)
    try click(point)
    try hidden("outside owned-window click hides")
    outside.orderOut(nil)
    try reopen()
    print("PASS tray toggle, Escape, custom Close, native CloseRequested and outside dismissal keep PID \(pid)")

    for entry in ["Settings", "Status", "Diagnostics", "Review Queue"] {
        try choose(application, entry)
        try waitFor("secondary route \(entry)") { visibleWindows(pid).count == 1 }
        try singlePanel()
    }
    try choose(application, "Quit PR Sniper")
    try waitFor("tray Quit ends owned process") { !process.isRunning }
    try require(process.terminationStatus == 0, "Quit returned a nonzero exit status")
    print("PASS secondary routes retain panel; explicit Quit ends owned PID \(pid)")
    print("UNVERIFIED real notification delivery, mixed-monitor placement, provider auth and child-work teardown: use acceptance procedures")
}

do { try run() }
catch { fputs("FAIL \(error)\n", stderr); exit(1) }
