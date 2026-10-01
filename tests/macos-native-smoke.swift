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
    let children = elements(value, kAXChildrenAttribute)
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

func evidenceActionRole(_ role: String) -> Bool {
    // WebKit exposes an HTML aria-pressed button as AXCheckBox.
    [kAXButtonRole, kAXCheckBoxRole].contains(role)
}

func rowActionButton(_ window: AXUIElement, _ identity: String) -> AXUIElement? {
    let groups = descendants(window).filter {
        text($0, kAXRoleAttribute) == kAXGroupRole && named($0, identity)
    }
    guard groups.count == 1 else { return nil }
    let buttons = descendants(groups[0]).filter {
        evidenceActionRole(text($0, kAXRoleAttribute)) && named($0, "Evidence and actions")
    }
    return buttons.count == 1 ? buttons[0] : nil
}

func visibleApplicationWindows(_ windows: [[String: Any]], _ pid: pid_t) -> [[String: Any]] {
    return windows.filter {
        ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid)
            && ($0[kCGWindowIsOnscreen as String] as? Bool) == true
            && ($0[kCGWindowAlpha as String] as? Double ?? 0) > 0
            && ($0[kCGWindowLayer as String] as? Int).map {
                // Tao's floating panel uses level 5, not Core Graphics' floating level 3.
                $0 >= 0 && $0 < Int(CGWindowLevelForKey(.mainMenuWindow))
            } == true
    }
}

func visibleWindows(_ pid: pid_t) -> [[String: Any]] {
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements],
                                           kCGNullWindowID) as? [[String: Any]] ?? []
    return visibleApplicationWindows(windows, pid)
}

func observeOwnedWindows(_ pid: pid_t) {
    let windows = CGWindowListCopyWindowInfo(.excludeDesktopElements, kCGNullWindowID) as? [[String: Any]] ?? []
    for window in windows where (window[kCGWindowOwnerPID as String] as? Int) == Int(pid) {
        print("OBSERVE pid=\(pid) window=\(window[kCGWindowNumber as String] ?? "?") layer=\(window[kCGWindowLayer as String] ?? "?") onscreen=\(window[kCGWindowIsOnscreen as String] ?? "?") bounds=\(window[kCGWindowBounds as String] ?? "?")")
    }
    let application = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(application, 1)
    for window in elements(application, kAXWindowsAttribute) {
        print("OBSERVE pid=\(pid) AX window title=\(text(window, kAXTitleAttribute)) position=\(String(describing: attribute(window, kAXPositionAttribute))) size=\(String(describing: attribute(window, kAXSizeAttribute)))")
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

func mouseEvent(_ point: CGPoint, _ type: CGEventType, _ mouse: CGMouseButton) throws -> CGEvent {
    guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: mouse) else {
        throw SmokeFailure.failed("Cannot construct owned-target mouse event")
    }
    event.flags.subtract([.maskCommand, .maskAlternate, .maskControl, .maskShift])
    event.setIntegerValueField(.mouseEventClickState, value: 1)
    return event
}

func click(_ point: CGPoint, right: Bool = false) throws {
    let mouse: CGMouseButton = right ? .right : .left
    for type: CGEventType in right ? [.rightMouseDown, .rightMouseUp] : [.leftMouseDown, .leftMouseUp] {
        try mouseEvent(point, type, mouse).post(tap: .cghidEventTap)
    }
}

func key(_ pid: pid_t, _ code: CGKeyCode, command: Bool = false, shift: Bool = false) throws {
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else {
            throw SmokeFailure.failed("Cannot construct owned-process key event")
        }
        event.flags = []
        if down && command { event.flags = .maskCommand }
        if down && shift { event.flags.insert(.maskShift) }
        try require(NSWorkspace.shared.frontmostApplication?.processIdentifier == pid,
                    "Refusing key event: the owned app is not frontmost")
        event.post(tap: .cghidEventTap)
    }
}

func typeText(_ pid: pid_t, _ value: String) throws {
    let characters = Array(value.utf16)
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down) else {
            throw SmokeFailure.failed("Cannot construct owned-process text event")
        }
        event.flags = []
        event.keyboardSetUnicodeString(stringLength: characters.count, unicodeString: characters)
        try require(NSWorkspace.shared.frontmostApplication?.processIdentifier == pid,
                    "Refusing text event: the owned app is not frontmost")
        event.post(tap: .cghidEventTap)
    }
}

func focus(_ value: AXUIElement) throws {
    try require(AXUIElementSetAttributeValue(value, kAXFocusedAttribute as CFString, kCFBooleanTrue) == .success,
                "Cannot focus owned control")
    var pid: pid_t = 0
    try require(AXUIElementGetPid(value, &pid) == .success, "Owned control PID unavailable")
    let application = AXUIElementCreateApplication(pid)
    try waitFor("owned control keyboard focus") {
        guard let focused = element(application, kAXFocusedUIElementAttribute) else { return false }
        return CFEqual(focused, value)
    }
}

func trayItem(_ application: AXUIElement) throws -> AXUIElement {
    var found: AXUIElement?
    try waitFor("application-owned PR Sniper tray item") {
        guard let extras = element(application, "AXExtrasMenuBar") else { return false }
        let candidates = descendants(extras).filter { text($0, kAXRoleAttribute) == kAXMenuBarItemRole }
        found = candidates.first { named($0, "PR Sniper") || text($0, kAXHelpAttribute) == "PR Sniper" }
        if found == nil && candidates.count == 1 { found = candidates[0] }
        return found != nil
    }
    return found!
}

func menuItem(_ application: AXUIElement, _ title: String) -> AXUIElement? {
    guard let extras = element(application, "AXExtrasMenuBar") else { return nil }
    return descendants(extras).first { text($0, kAXRoleAttribute) == kAXMenuItemRole && named($0, title) }
}

func choose(_ application: AXUIElement, _ title: String) throws {
    try click(center(trayItem(application)), right: true)
    try waitFor("secondary tray menu") { menuItem(application, "Quit PR Sniper") != nil }
    for required in ["Review Queue", "Settings", "Status", "Diagnostics", "Check Now", "Close Panel", "Quit PR Sniper"] {
        try require(menuItem(application, required) != nil, "Missing secondary tray entry: \(required)")
    }
    try press(menuItem(application, title)!, title)
}

func deleteKeychainService(_ service: String) {
    let status = SecItemDelete([kSecClass: kSecClassGenericPassword, kSecAttrService: service] as CFDictionary)
    if status != errSecSuccess && status != errSecItemNotFound {
        fputs("CLEANUP failed for owned Keychain service \(service): \(status)\n", stderr)
    } else {
        print("CLEANUP owned Keychain service absent: \(service)")
    }
}

func diagnosticEvents(_ root: URL) throws -> [String] {
    let data = try Data(contentsOf: root.appendingPathComponent("state/diagnostics.jsonl"))
    return try data.split(separator: 10).map {
        guard let entry = try JSONSerialization.jsonObject(with: Data($0)) as? [String: Any],
              let event = entry["event"] as? String,
              entry.count == 2, entry["timestamp_secs"] is NSNumber else {
            throw SmokeFailure.failed("Invalid owned host diagnostic record")
        }
        return event
    }
}

func selfTest() throws {
    let pid: pid_t = 123
    func window(_ owner: Int = 123, _ layer: Int = 5, _ visible: Bool = true, _ alpha: Double = 1) -> [String: Any] {
        [kCGWindowOwnerPID as String: owner, kCGWindowLayer as String: layer,
         kCGWindowIsOnscreen as String: visible, kCGWindowAlpha as String: alpha]
    }
    try require(visibleApplicationWindows([window()], pid).count == 1, "Rejects real layer-5 panel")
    try require(visibleApplicationWindows([window(), window()], pid).count == 2, "Hides duplicate panels")
    for excluded in [window(999), window(123, 103), window(123, 5, false), window(123, 5, true, 0)] {
        try require(visibleApplicationWindows([excluded], pid).isEmpty, "Counts foreign, tooltip, hidden or transparent window")
    }
    for (type, mouse) in [(CGEventType.leftMouseDown, CGMouseButton.left), (.leftMouseUp, .left),
                          (.rightMouseDown, .right), (.rightMouseUp, .right)] {
        let event = try mouseEvent(CGPoint(x: 1, y: 1), type, mouse)
        try require(event.type == type && event.getIntegerValueField(.mouseEventClickState) == 1,
                    "Synthetic event must be an actual single click")
    }
    try require(evidenceActionRole(kAXButtonRole) && evidenceActionRole(kAXCheckBoxRole)
        && !evidenceActionRole(kAXGroupRole), "Wrong native evidence-action roles")
    print("PASS harness regression checks (no app launched; not native acceptance)")
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
         "pull_request_id": "\(number)", "number": number, "title": "Same title",
         "work": ["id": "native-work-\(number)", "item_id": "native-item-\(number)",
                  "iteration_id": "native-iteration-\(number)", "iteration": 1,
                  "agent_id": agent, "enqueue_order": number, "pass_ordinal": 1,
                  "trigger": "admission",
                  "admission": ["watched_author": false, "all_authors": true, "requested_reviewer": false]],
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
    setbuf(stdout, nil)
    if CommandLine.arguments.dropFirst() == ["--self-test"] {
        try selfTest()
        return
    }
    if CommandLine.arguments.dropFirst() == ["--preflight"] {
        try preflight()
        print("PASS native automation permissions available (no app launched)")
        return
    }
    try require(CommandLine.arguments.count == 3,
                "Usage: native-smoke --self-test | --preflight | /absolute/test-owned.app /absolute/settings_bridge (with absolute owned TMPDIR)")
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

    guard let temporaryPath = ProcessInfo.processInfo.environment["TMPDIR"], temporaryPath.hasPrefix("/") else {
        throw SmokeFailure.failed("Provide an absolute owned TMPDIR for native fixture artifacts")
    }
    let fixture = URL(fileURLWithPath: temporaryPath, isDirectory: true)
        .appendingPathComponent("pr-sniper-native-\(UUID().uuidString)", isDirectory: true)
    let keychainService = "com.jdylanmc.pr-sniper.tests.native-\(UUID().uuidString)"
    try FileManager.default.createDirectory(at: fixture, withIntermediateDirectories: false)
    let process = Process()
    defer {
        if process.isRunning {
            observeOwnedWindows(process.processIdentifier)
            process.terminate()
            let deadline = Date().addingTimeInterval(5)
            while process.isRunning && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.1)) }
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
            process.waitUntilExit()
            print("CLEANUP terminated owned PID \(process.processIdentifier); not a Quit pass")
        }
        do {
            try FileManager.default.removeItem(at: fixture)
            print("CLEANUP owned profile absent: \(fixture.path)")
        }
        catch { fputs("CLEANUP failed for \(fixture.path): \(error)\n", stderr) }
        deleteKeychainService(keychainService)
        deleteKeychainService("\(keychainService).legacy")
        if process.processIdentifier > 0 {
            print("CLEANUP owned PID \(process.processIdentifier) absent=\(kill(process.processIdentifier, 0) == -1 && errno == ESRCH)")
        }
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

    let trayCenter = try center(trayItem(application))
    print("OBSERVE pid=\(pid) trayCenter=\(trayCenter)")
    try click(trayCenter)
    try waitFor("single visible panel") { visibleWindows(pid).count == 1 }
    let windowID = visibleWindows(pid)[0][kCGWindowNumber as String] as? Int
    try require(windowID != nil, "Native panel window identity unavailable")
    var panel: AXUIElement?
    try waitFor("accessible panel") {
        panel = elements(application, kAXWindowsAttribute).first { named($0, "PR Sniper") }
        return panel != nil
    }
    var window = panel!
    observeOwnedWindows(pid)
    print("PASS visible accessible panel window=\(windowID!)")
    func focusedPanel() throws {
        try waitFor("panel owns keyboard focus") {
            guard let focused = element(application, kAXFocusedWindowAttribute) else { return false }
            return CFEqual(focused, window)
        }
    }
    try focusedPanel()
    func singlePanel() throws {
        let visible = visibleWindows(pid)
        try require(visible.count == 1 && visible[0][kCGWindowNumber as String] as? Int == windowID,
                    "Navigation created or replaced the native panel")
    }
    var lastHiddenAt: Date?
    func reacquirePanel() throws {
        try waitFor("reopened accessible panel") {
            guard let current = elements(application, kAXWindowsAttribute).first(where: { named($0, "PR Sniper") }),
                  descendants(current).contains(where: { named($0, "Hide PR Sniper panel") }) else { return false }
            window = current
            return true
        }
        try focusedPanel()
    }
    func reopen() throws {
        if let hiddenAt = lastHiddenAt {
            print("OBSERVE tray reopen \(Int(Date().timeIntervalSince(hiddenAt) * 1000))ms after hidden observation")
        }
        try click(center(trayItem(application)))
        try waitFor("retained panel reopened") { visibleWindows(pid).count == 1 }
        try singlePanel()
        try reacquirePanel()
    }
    func hidden(_ reason: String) throws {
        try waitFor(reason) { visibleWindows(pid).isEmpty }
        try require(process.isRunning, "\(reason) terminated the owned host")
        lastHiddenAt = Date()
        print("PASS \(reason); owned PID \(pid) retained")
    }
    for tab in ["Queue", "Running", "Reviewed", "Settings"] {
        try press(button(window, tab), tab)
        try waitFor("destination \(tab)") {
            descendants(window).contains { named($0, tab) && [kAXHeadingRole, kAXStaticTextRole].contains(text($0, kAXRoleAttribute)) }
        }
        try singlePanel()
    }
    try press(button(window, "Queue"), "Queue")
    let rowIdentity = "Actions for example/repo #2; account 22; item native-item-2"
    let otherRowIdentity = "Actions for example/repo #1; account 22; item native-item-1"
    var rowButton: AXUIElement?
    try waitFor("exact fixture row") {
        rowButton = rowActionButton(window, rowIdentity)
        return rowButton != nil && rowActionButton(window, otherRowIdentity) != nil
    }
    try press(rowButton!, "exact PR #2 detail")
    try waitFor("exact detail title") {
        descendants(window).contains { named($0, "example/repo #2: Same title") }
            && descendants(window).contains { named($0, "Saved evidence") }
    }
    try press(button(window, "Back"), "Back to exact row")
    try waitFor("exact row keyboard focus after Back") {
        guard let currentButton = rowActionButton(window, rowIdentity),
              let otherButton = rowActionButton(window, otherRowIdentity),
              let focused = element(application, kAXFocusedUIElementAttribute) else { return false }
        return CFEqual(focused, currentButton) && !CFEqual(focused, otherButton)
    }
    try singlePanel()
    print("PASS four destinations, one native window and exact Back row/focus")

    try press(button(window, "Settings"), "Settings for native picker")
    try press(button(window, "Choose folder..."), "native folder picker")
    var picker: AXUIElement?
    try waitFor("owned native folder sheet") {
        picker = descendants(window).first { text($0, kAXRoleAttribute) == kAXSheetRole }
        return picker != nil
    }
    try require(visibleWindows(pid).contains { $0[kCGWindowNumber as String] as? Int == windowID },
                "Native folder picker hid the retained panel")
    let pickerFolder = fixture.appendingPathComponent("pr-sniper-owned-picker", isDirectory: true)
    try FileManager.default.createDirectory(at: pickerFolder, withIntermediateDirectories: false)
    try key(pid, 5, command: true, shift: true)
    try waitFor("owned Go to Folder text focus") {
        guard let focused = element(application, kAXFocusedUIElementAttribute) else { return false }
        return [kAXTextFieldRole, kAXComboBoxRole].contains(text(focused, kAXRoleAttribute))
    }
    try typeText(pid, pickerFolder.path)
    try waitFor("owned folder path entered") {
        guard let focused = element(application, kAXFocusedUIElementAttribute) else { return false }
        return text(focused, kAXValueAttribute) == pickerFolder.path
    }
    try key(pid, 36)
    try waitFor("picker at owned folder") {
        descendants(picker!).contains { named($0, pickerFolder.lastPathComponent) }
    }
    try press(button(picker!, "Cancel"), "cancel native folder picker")
    try waitFor("native picker dismissed") {
        !descendants(window).contains { text($0, kAXRoleAttribute) == kAXSheetRole }
    }
    try singlePanel()
    try focusedPanel()
    print("PASS native picker visits only owned folder, cancels and restores panel focus")

    var settingsSection: AXUIElement?
    try waitFor("Settings section selector") {
        settingsSection = descendants(window).first {
            text($0, kAXRoleAttribute) == kAXPopUpButtonRole && named($0, "Settings section")
        }
        return settingsSection != nil
    }
    try press(settingsSection!, "Settings section selector")
    try key(pid, 125)
    try key(pid, 36)
    try press(button(window, "New doctrine"), "New doctrine draft")
    func draftField(_ name: String, _ role: String) throws -> AXUIElement {
        var field: AXUIElement?
        try waitFor("draft \(name)") {
            field = descendants(window).first { text($0, kAXRoleAttribute) == role && named($0, name) }
            return field != nil
        }
        return field!
    }
    func enterDraft(_ name: String, _ role: String, _ value: String) throws {
        let field = try draftField(name, role)
        try focus(field)
        try typeText(pid, value)
        try waitFor("native typing delivered to \(name)") { text(field, kAXValueAttribute) == value }
    }
    try enterDraft("Title", kAXTextFieldRole, "Native retained draft")
    try enterDraft("Principles", kAXTextAreaRole, "Keep this offline unsaved draft.")
    func retainedDraft() throws {
        let title = try draftField("Title", kAXTextFieldRole)
        let body = try draftField("Principles", kAXTextAreaRole)
        try waitFor("unchanged native draft") {
            text(title, kAXValueAttribute) == "Native retained draft"
                && text(body, kAXValueAttribute) == "Keep this offline unsaved draft."
        }
        try singlePanel()
    }
    try retainedDraft()
    for tab in ["Queue", "Running", "Reviewed", "Settings"] {
        try press(button(window, tab), "draft tab \(tab)")
        try waitFor("draft destination \(tab)") {
            descendants(window).contains { text($0, kAXRoleAttribute) == kAXHeadingRole && named($0, tab) }
        }
        try singlePanel()
    }
    try retainedDraft()
    try focus(draftField("Principles", kAXTextAreaRole))
    var reachedNavigation = false
    for _ in 0..<16 {
        try key(pid, 48)
        RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        guard let focused = element(application, kAXFocusedUIElementAttribute) else {
            throw SmokeFailure.failed("Keyboard focus disappeared from owned editor")
        }
        try require(descendants(window).contains { CFEqual($0, focused) }, "Keyboard focus escaped the owned panel")
        if text(focused, kAXRoleAttribute) == kAXButtonRole && named(focused, "Queue") { reachedNavigation = true }
    }
    try require(reachedNavigation, "Draft keyboard navigation never reaches global destinations")
    try retainedDraft()
    print("PASS unsaved draft survives four destinations; real Tab reaches global navigation")

    try click(center(trayItem(application)))
    try hidden("tray left-click closes without immediate reopen")
    RunLoop.current.run(until: Date().addingTimeInterval(1))
    try require(visibleWindows(pid).isEmpty, "Tray focus-loss race reopened the panel")
    try reopen()
    try retainedDraft()
    try key(pid, 53)
    try hidden("Escape hides")
    try reopen()
    try retainedDraft()
    try press(button(window, "Hide PR Sniper panel"), "custom Close")
    try hidden("custom Close hides")
    try reopen()
    try retainedDraft()

    // Exercise the host's CloseRequested separately from the renderer hide command.
    let closeRequests = try diagnosticEvents(fixture).filter { $0 == "window_close_requested" }.count
    try choose(application, "Close Panel")
    try hidden("native CloseRequested hides")
    try require(try diagnosticEvents(fixture).filter { $0 == "window_close_requested" }.count == closeRequests + 1,
                "Native CloseRequested callback did not run; blur alone is not a Close pass")
    print("PASS durable window_close_requested callback evidence (not merely tray-menu blur)")
    try reopen()
    try retainedDraft()

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
    try retainedDraft()
    print("PASS tray toggle, Escape, custom Close, native CloseRequested and outside dismissal keep PID \(pid)")

    for entry in ["Settings", "Status", "Diagnostics", "Review Queue"] {
        try choose(application, entry)
        try waitFor("secondary route \(entry)") { visibleWindows(pid).count == 1 }
        try singlePanel()
        try reacquirePanel()
        let heading = entry == "Review Queue" ? "Queue" : entry
        try waitFor("exact secondary destination \(entry)") {
            descendants(window).contains { text($0, kAXRoleAttribute) == kAXHeadingRole && named($0, heading) }
        }
    }
    try choose(application, "Settings")
    try reacquirePanel()
    try retainedDraft()
    print("PASS draft retained through every dismissal and exact secondary routes")
    try choose(application, "Quit PR Sniper")
    try waitFor("tray Quit ends owned process") { !process.isRunning }
    try require(process.terminationStatus == 0, "Quit returned a nonzero exit status")
    try require(try diagnosticEvents(fixture).contains("quit_requested"), "Owned host did not record explicit Quit")
    print("PASS secondary routes retain panel; explicit Quit ends owned PID \(pid)")
    print("UNVERIFIED real notification delivery, mixed-monitor placement, provider auth and child-work teardown: use acceptance procedures")
}

do { try run() }
catch { fputs("FAIL \(error)\n", stderr); exit(1) }
