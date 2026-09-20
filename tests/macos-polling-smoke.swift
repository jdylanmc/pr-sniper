import AppKit
import ApplicationServices
import Foundation

enum PollingSmokeFailure: Error, CustomStringConvertible {
    case failed(String)

    var description: String {
        switch self {
        case .failed(let message): return message
        }
    }
}

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw PollingSmokeFailure.failed(message) }
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
    if depth == 16 { return [] }
    var children = elements(element, kAXChildrenAttribute)
    if depth == 0 {
        for name in [kAXMenuBarAttribute, "AXExtrasMenuBar"] {
            if let value = attribute(element, name),
               CFGetTypeID(value) == AXUIElementGetTypeID() {
                children.append(unsafeBitCast(value, to: AXUIElement.self))
            }
        }
    }
    return [element] + children.flatMap { descendants($0, depth: depth + 1) }
}

func waitFor(_ message: String, seconds: TimeInterval = 15,
             _ predicate: () throws -> Bool) throws {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if try predicate() { return }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    throw PollingSmokeFailure.failed("Timed out: \(message)")
}

func press(_ element: AXUIElement, _ description: String) throws {
    let result = AXUIElementPerformAction(element, kAXPressAction as CFString)
    try require(result == .success, "\(description): AX action failed (\(result.rawValue))")
}

func menuItem(_ application: AXUIElement, title: String) -> AXUIElement? {
    descendants(application).first {
        text($0, kAXRoleAttribute) == kAXMenuItemRole && text($0, kAXTitleAttribute) == title
    }
}

func openTray(_ application: AXUIElement) throws {
    if menuItem(application, title: "Quit PR Sniper") != nil { return }
    for item in descendants(application).filter({
        text($0, kAXRoleAttribute) == kAXMenuBarItemRole
    }).reversed() {
        try press(item, "open native tray")
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        if menuItem(application, title: "Quit PR Sniper") != nil { return }
        _ = AXUIElementPerformAction(item, kAXCancelAction as CFString)
    }
    throw PollingSmokeFailure.failed("No owned native tray with Quit PR Sniper")
}

func choose(_ application: AXUIElement, title: String) throws {
    try openTray(application)
    guard let item = menuItem(application, title: title) else {
        throw PollingSmokeFailure.failed("Missing native tray item: \(title)")
    }
    try require(attribute(item, kAXEnabledAttribute) as? Bool == true,
                "Disabled native tray item: \(title)")
    try press(item, title)
}

func visibleWindow(_ application: AXUIElement, title: String) -> AXUIElement? {
    elements(application, kAXWindowsAttribute).first {
        text($0, kAXTitleAttribute) == "PR Sniper - \(title)"
            && attribute($0, kAXMinimizedAttribute) as? Bool != true
    }
}

func closeWindow(_ application: AXUIElement, title: String) throws {
    guard let window = visibleWindow(application, title: title),
          let value = attribute(window, kAXCloseButtonAttribute),
          CFGetTypeID(value) == AXUIElementGetTypeID() else {
        throw PollingSmokeFailure.failed("No AX close button for \(title)")
    }
    try press(unsafeBitCast(value, to: AXUIElement.self), "close \(title)")
    try waitFor("\(title) absent from AX windows") {
        visibleWindow(application, title: title) == nil
    }
}

func preflight() throws {
    try require(AXIsProcessTrusted(), "Accessibility absent; native polling UNVERIFIED")
}

struct Health: Decodable {
    let repository_id: String
    let last_attempt: Int64?
    let last_success: Int64?
    let next_run: Int64
    let last_failure: String?
    let in_flight: Bool
}

struct ExpectedQueue: Decodable {
    let repository_id: String
    let repository_name: String
    let pull_request_id: String
    let number: Int
    let title: String
    let head_sha: String
}

struct QueueRow: Decodable {
    let provider: String
    let repository_id: String
    let pull_request_id: String
    let head_sha: String
    let trigger_policy: String
    let watched_author: Bool
    let waiting: String
    let detected_at: Int64
}

func expectedRow(_ root: URL, _ expected: ExpectedQueue) throws -> QueueRow {
    let rows = try JSONDecoder().decode(
        [QueueRow].self, from: Data(contentsOf: root.appendingPathComponent("state/queue.json")))
    let matching = rows.filter {
        $0.provider == "github" && $0.repository_id == expected.repository_id
            && $0.pull_request_id == expected.pull_request_id && $0.head_sha == expected.head_sha
    }
    try require(matching.count == 1, "Expected exactly one persisted live revision; found \(matching.count)")
    let row = matching[0]
    try require(row.watched_author && row.waiting == "trust_confirmation",
                "Live fork must show watched-author eligibility and trust-confirmation waiting")
    return row
}

func assertQueueVisible(_ application: AXUIElement, _ expected: ExpectedQueue) throws {
    try choose(application, title: "Review Queue")
    try waitFor("expected live revision rendered in native Queue", seconds: 30) {
        guard let window = visibleWindow(application, title: "Review Queue") else { return false }
        let visibleText = descendants(window).flatMap { element in
            [kAXTitleAttribute, kAXValueAttribute, kAXDescriptionAttribute].map { text(element, $0) }
        }.joined(separator: "\n")
        return visibleText.contains("\(expected.repository_name) #\(expected.number): \(expected.title)")
            && visibleText.contains(expected.head_sha)
            && visibleText.contains("watched author")
            && visibleText.contains("Trust confirmation required")
    }
    try closeWindow(application, title: "Review Queue")
}

func readHealth(_ root: URL, repositoryID: String) throws -> Health? {
    let path = root.appendingPathComponent("state/polling.json")
    if !FileManager.default.fileExists(atPath: path.path) { return nil }
    let rows = try JSONDecoder().decode([Health].self, from: Data(contentsOf: path))
    return rows.first { $0.repository_id == repositoryID }
}

func run() throws {
    if CommandLine.arguments.dropFirst() == ["--preflight"] {
        try preflight()
        print("PASS AX-only preflight; no app launched or permission changed")
        return
    }
    try require([3, 4].contains(CommandLine.arguments.count),
                "Usage: swift tests/macos-polling-smoke.swift /absolute/PR Sniper.app /absolute/owned-data-fixture [/absolute/expected-queue.json]")
    try preflight()
    try require(CommandLine.arguments[1].hasPrefix("/") && CommandLine.arguments[2].hasPrefix("/"),
                "Bundle and owned fixture paths must be absolute")
    let bundleURL = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
    let root = URL(fileURLWithPath: CommandLine.arguments[2]).standardizedFileURL
    let expected: ExpectedQueue?
    if CommandLine.arguments.count == 4 {
        try require(CommandLine.arguments[3].hasPrefix("/"), "Expected-queue path must be absolute")
        expected = try JSONDecoder().decode(
            ExpectedQueue.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[3])))
    } else {
        expected = nil
    }
    guard let bundle = Bundle(url: bundleURL),
          let identifier = bundle.bundleIdentifier,
          let executable = bundle.executableURL else {
        throw PollingSmokeFailure.failed("No executable application bundle at supplied path")
    }
    try require(identifier.contains("pr-sniper") || identifier.contains("prsniper"),
                "Refusing an application not identified as PR Sniper")
    try require(NSRunningApplication.runningApplications(withBundleIdentifier: identifier).isEmpty,
                "Another PR Sniper instance exists; coordinate the native-run owner")
    try require(!FileManager.default.fileExists(atPath: root.appendingPathComponent("state/polling.json").path),
                "Supply a fresh owned fixture; existing polling evidence will not be overwritten")
    let settingsBytes = try Data(contentsOf: root.appendingPathComponent("config/settings.json"))
    guard let settings = try JSONSerialization.jsonObject(with: settingsBytes) as? [String: Any],
          settings["launch_at_login"] as? Bool == false,
          let defaults = settings["defaults"] as? [String: Any],
          let repositories = settings["repositories"] as? [[String: Any]],
          repositories.count == 1,
          let repository = repositories.first,
          repository["enabled"] as? Bool == true,
          let repositoryID = repository["id"] as? String else {
        throw PollingSmokeFailure.failed("Fixture must contain exactly one enabled repository and login disabled")
    }
    let overrides = repository["overrides"] as? [String: Any] ?? [:]
    guard let schedule = (overrides["schedule"] ?? defaults["schedule"]) as? [String: Any],
          schedule["kind"] as? String == "interval",
          schedule["minutes"] as? Int == 1,
          (overrides["automatic_agent_start"] ?? defaults["automatic_agent_start"]) as? Bool == false,
          (overrides["automatic_comment_publication"] ?? defaults["automatic_comment_publication"]) as? Bool == false else {
        throw PollingSmokeFailure.failed("Fixture needs a one-minute interval and both automation gates off")
    }

    let process = Process()
    process.executableURL = executable
    process.environment = ProcessInfo.processInfo.environment.merging([
        "PR_SNIPER_DATA_DIR": root.path
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
            print("CLEANUP terminated owned PID \(process.processIdentifier); not a Quit pass")
        }
        print("PRESERVED owned data and evidence at \(root.path)")
    }
    try process.run()
    print("OBSERVE bundle=\(bundleURL.path) executable=\(executable.path) pid=\(process.processIdentifier) data=\(root.path)")
    let application = AXUIElementCreateApplication(process.processIdentifier)
    try waitFor("native startup and persisted schedule") {
        let health = try readHealth(root, repositoryID: repositoryID)
        return process.isRunning && descendants(application).count > 1 && health != nil
    }
    try require(elements(application, kAXWindowsAttribute).isEmpty,
                "Startup unexpectedly exposes a persistent window")

    for title in ["Settings", "Review Queue"] {
        try choose(application, title: title)
        try waitFor("\(title) native AX window") {
            visibleWindow(application, title: title) != nil
        }
        try closeWindow(application, title: title)
        try require(process.isRunning, "Closing \(title) stopped the host")
    }
    try require(elements(application, kAXWindowsAttribute).isEmpty,
                "Native windows remain after closing both surfaces")
    let initialAttempt = try readHealth(root, repositoryID: repositoryID)?.last_attempt
    try waitFor("scheduled check with all windows closed", seconds: 95) {
        guard let health = try readHealth(root, repositoryID: repositoryID) else { return false }
        return health.last_attempt != nil && health.last_attempt != initialAttempt && !health.in_flight
    }
    guard let scheduled = try readHealth(root, repositoryID: repositoryID) else {
        throw PollingSmokeFailure.failed("Scheduled health disappeared")
    }
    try require(scheduled.last_success != nil && scheduled.last_failure == nil,
                "Scheduled provider check failed; successful native polling remains UNVERIFIED")
    try require(elements(application, kAXWindowsAttribute).isEmpty && process.isRunning,
                "Scheduled polling did not retain a windowless host")
    print("PASS scheduled attempt=\(scheduled.last_attempt!) succeeded with Settings and Queue closed")
    var firstQueueRow: QueueRow?
    if let expected {
        firstQueueRow = try expectedRow(root, expected)
        try assertQueueVisible(application, expected)
        print("PASS expected live revision persisted and rendered with watched-author/trust-confirmation state")
    }

    var beforeImmediate = scheduled
    try waitFor("idle cadence with time for Check Now", seconds: 95) {
        guard let health = try readHealth(root, repositoryID: repositoryID),
              let attempted = health.last_attempt else { return false }
        let now = Int64(Date().timeIntervalSince1970)
        guard !health.in_flight && now > attempted && health.next_run - now > 10 else { return false }
        beforeImmediate = health
        return true
    }
    try choose(application, title: "Check Now")
    try waitFor("immediate check before the next scheduled run") {
        guard let health = try readHealth(root, repositoryID: repositoryID) else { return false }
        return health.last_attempt != beforeImmediate.last_attempt
    }
    guard let immediate = try readHealth(root, repositoryID: repositoryID) else {
        throw PollingSmokeFailure.failed("Check Now health disappeared")
    }
    try require(immediate.next_run == beforeImmediate.next_run,
                "Check Now changed the established next scheduled run")
    print("PASS native Check Now preserves next_run=\(immediate.next_run)")

    for title in ["Settings", "Review Queue"] {
        try choose(application, title: title)
        try waitFor("\(title) reopens") { visibleWindow(application, title: title) != nil }
        try closeWindow(application, title: title)
    }
    try waitFor("immediate read finishes", seconds: 60) {
        guard let health = try readHealth(root, repositoryID: repositoryID) else { return false }
        return !health.in_flight
    }
    guard let beforeQuit = try readHealth(root, repositoryID: repositoryID) else {
        throw PollingSmokeFailure.failed("Final health disappeared")
    }
    try require(beforeQuit.last_failure == nil && beforeQuit.last_success != nil,
                "Repeat check failed; repeat-poll queue deduplication remains UNVERIFIED")
    if let expected, let firstQueueRow {
        let repeated = try expectedRow(root, expected)
        try require(repeated.trigger_policy == firstQueueRow.trigger_policy
                        && repeated.detected_at == firstQueueRow.detected_at,
                    "Repeat poll replaced or duplicated the same revision-policy job")
        try assertQueueVisible(application, expected)
        print("PASS repeat poll preserves exactly one expected revision-policy job and native Queue content")
    }
    try choose(application, title: "Quit PR Sniper")
    try waitFor("actual native Quit exits owned process") { !process.isRunning }
    try require(process.terminationStatus == 0, "Native Quit exited unsuccessfully")
    let healthURL = root.appendingPathComponent("state/polling.json")
    let finalBytes = try Data(contentsOf: healthURL)
    let afterDue = Date(timeIntervalSince1970: TimeInterval(beforeQuit.next_run + 2))
    try require(afterDue.timeIntervalSinceNow < 90, "Unexpected cadence prevents bounded Quit observation")
    while Date() < afterDue {
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    try require(try Data(contentsOf: healthURL) == finalBytes,
                "Polling health changed after Quit across the next due time")
    try require(try Data(contentsOf: root.appendingPathComponent("config/settings.json")) == settingsBytes,
                "Native polling changed saved configuration")
    print("PASS native Quit stops owned PID and leaves health unchanged across next due time")
    if expected == nil {
        print("UNVERIFIED eligible-live queue contents: no expected live fixture supplied")
    }
    print("UNVERIFIED icon appearance and review execution; no review or provider mutation performed")
}

do {
    try run()
} catch {
    fputs("FAIL \(error)\n", stderr)
    exit(1)
}
