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

struct Budget {
    let start: TimeInterval
    let total: TimeInterval
    let cleanupReserve: TimeInterval

    init(start: TimeInterval = ProcessInfo.processInfo.systemUptime,
         total: TimeInterval = 300, cleanupReserve: TimeInterval = 10) {
        self.start = start
        self.total = total
        self.cleanupReserve = cleanupReserve
    }

    func remaining(at now: TimeInterval, cleanup: Bool = false) -> TimeInterval {
        max(0, start + total - (cleanup ? 0 : cleanupReserve) - now)
    }

    func check() throws {
        try require(remaining(at: ProcessInfo.processInfo.systemUptime) > 0,
                    "Total monotonic budget exhausted; reserving cleanup time")
    }

    func messagingTimeout(at now: TimeInterval) throws -> Float {
        let available = remaining(at: now)
        try require(now.isFinite && available.isFinite && available >= 0.1,
                    "Insufficient work budget for bounded AX call; reserving cleanup time")
        let selected = min(2, available / 2)
        let timeout = Float(selected)
        return Double(timeout) > selected ? timeout.nextDown : timeout
    }
}

let budget = Budget()

func boundedAXMessage<Reference, Result>(
    _ reference: Reference, limit: Budget = budget,
    now: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
    configure: (Reference, Float) -> Bool, operation: () -> Result
) throws -> Result {
    let timeout = try limit.messagingTimeout(at: now())
    try require(configure(reference, timeout), "Cannot establish per-reference AX messaging timeout")
    try require(limit.remaining(at: now()) > Double(timeout),
                "AX timeout no longer fits work budget after configuration")
    let result = operation()
    try require(limit.remaining(at: now()) > 0, "AX operation exhausted work budget")
    return result
}

func axMessage(_ element: AXUIElement, _ operation: () -> AXError) throws -> AXError {
    try boundedAXMessage(element, configure: {
        AXUIElementSetMessagingTimeout($0, $1) == .success
    }, operation: operation)
}

func attribute(_ element: AXUIElement, _ name: String) throws -> CFTypeRef? {
    var value: CFTypeRef?
    guard try axMessage(element, {
        AXUIElementCopyAttributeValue(element, name as CFString, &value)
    }) == .success else {
        return nil
    }
    return value
}

func typedElements(_ value: CFTypeRef?) -> [AXUIElement] {
    guard let value, CFGetTypeID(value) == CFArrayGetTypeID() else { return [] }
    let array = unsafeBitCast(value, to: CFArray.self)
    return (0..<CFArrayGetCount(array)).compactMap { index in
        guard let pointer = CFArrayGetValueAtIndex(array, index) else { return nil }
        let item = unsafeBitCast(pointer, to: CFTypeRef.self)
        guard CFGetTypeID(item) == AXUIElementGetTypeID() else {
            print("AX array contains unexpected type=\(CFGetTypeID(item)) index=\(index)")
            return nil
        }
        return unsafeBitCast(item, to: AXUIElement.self)
    }
}

func observedAXElements(_ value: CFTypeRef?) throws -> [AXUIElement] {
    guard let value, CFGetTypeID(value) == CFArrayGetTypeID() else {
        throw PollingSmokeFailure.failed("AX element snapshot unavailable or not an array")
    }
    let array = unsafeBitCast(value, to: CFArray.self)
    let decoded = typedElements(value)
    try require(decoded.count == CFArrayGetCount(array), "Malformed AX element snapshot")
    return decoded
}

func observedAXChildren(_ element: AXUIElement) throws -> [AXUIElement] {
    var value: CFTypeRef?
    let result = try axMessage(element) {
        AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &value)
    }
    if result == .attributeUnsupported || result == .noValue { return [] }
    try require(result == .success, "AX children unavailable (\(result.rawValue))")
    return try observedAXElements(value)
}

func extrasMenu(_ application: AXUIElement) throws -> AXUIElement? {
    guard let value = try attribute(application, "AXExtrasMenuBar"),
          CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
}

func elementPID(_ element: AXUIElement) throws -> pid_t? {
    var pid: pid_t = 0
    let result = try axMessage(element) { AXUIElementGetPid(element, &pid) }
    return result == .success && pid > 0 ? pid : nil
}

func sameProcess(_ element: AXUIElement, _ application: AXUIElement) throws -> Bool {
    guard let pid = try elementPID(element) else { return false }
    return try pid == elementPID(application)
}

func text(_ element: AXUIElement, _ name: String) throws -> String {
    try attribute(element, name) as? String ?? ""
}

func enableOwnedAccessibilityTree(_ application: AXUIElement) throws {
    for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
        try budget.check()
        let result = try axMessage(application) {
            AXUIElementSetAttributeValue(application, name as CFString, kCFBooleanTrue)
        }
        print("AX owned-app \(name)=true result=\(result.rawValue)")
    }
}

func descendants(_ element: AXUIElement) throws -> [AXUIElement] {
    var pending = [(element, 0)]
    var result: [AXUIElement] = []
    while let (next, depth) = pending.popLast() {
        try budget.check()
        try require(depth <= 24 && result.count < 2500, "AX traversal exceeded finite bound")
        if result.contains(where: { CFEqual($0, next) }) { continue }
        result.append(next)
        pending.append(contentsOf: try observedAXChildren(next).map { ($0, depth + 1) })
    }
    return result
}

func waitFor(_ message: String, seconds: TimeInterval = 15,
             _ predicate: () throws -> Bool) throws {
    let deadline = ProcessInfo.processInfo.systemUptime + seconds
    while ProcessInfo.processInfo.systemUptime < deadline {
        try budget.check()
        let satisfied = try predicate()
        try budget.check()
        if satisfied { return }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    throw PollingSmokeFailure.failed("Timed out: \(message)")
}

struct ActionTarget {
    let pid: pid_t?
    let role: String
    let title: String
    let enabled: Bool?
    let actions: [String]?
    let applicationAlias: Bool
}

func validateAction(_ targets: [ActionTarget], pid: pid_t, role: String,
                    title: String?, action: String) throws {
    try require(targets.count == 1, "Action target is missing or ambiguous: \(targets.count)")
    let target = targets[0]
    try require(pid > 0 && target.pid == pid && !target.applicationAlias
                    && target.role == role && (title == nil || target.title == title)
                    && target.enabled == true && target.actions?.contains(action) == true,
                "Action target ownership/role/title/enabled/supported-action guard failed")
}

func actionTarget(_ element: AXUIElement, application: AXUIElement) throws -> ActionTarget {
    let pid = try elementPID(element)
    var actions: CFArray?
    let copied = try axMessage(element) { AXUIElementCopyActionNames(element, &actions) }
    return try ActionTarget(pid: pid,
                        role: text(element, kAXRoleAttribute), title: text(element, kAXTitleAttribute),
                        enabled: attribute(element, kAXEnabledAttribute) as? Bool,
                        actions: copied == .success ? actions as? [String] : nil,
                        applicationAlias: CFEqual(element, application))
}

func perform(_ candidates: [AXUIElement], application: AXUIElement, role: String,
             title: String? = nil, action: String = kAXPressAction) throws {
    try budget.check()
    try require(candidates.count == 1, "Action target is missing or ambiguous: \(candidates.count)")
    guard let pid = try elementPID(application) else {
        throw PollingSmokeFailure.failed("Application PID unavailable")
    }
    let currentOwner = try processIdentity(pid)
    try require(launchedIdentity != nil && currentOwner == launchedIdentity
                    && currentOwner?.executable == launchedIdentity?.executable,
                "Owned process birth/executable identity changed before action")
    try validateAction(candidates.map { try actionTarget($0, application: application) },
                       pid: pid, role: role, title: title, action: action)
    let target = candidates[0]
    try validateAction([actionTarget(target, application: application)],
                       pid: pid, role: role, title: title, action: action)
    try budget.check()
    let result = try axMessage(target) { AXUIElementPerformAction(target, action as CFString) }
    try require(result == .success, "Owned \(title ?? role) action failed (\(result.rawValue))")
    print("ACTION pid=\(pid) role=\(role) title=\(title ?? "<close/tray>") action=\(action)")
}

func menuItems(_ application: AXUIElement, title: String) throws -> [AXUIElement] {
    guard let extras = try extrasMenu(application), try sameProcess(extras, application) else { return [] }
    return try descendants(extras).filter {
        try budget.check()
        return try sameProcess($0, application) &&
        text($0, kAXRoleAttribute) == kAXMenuItemRole && text($0, kAXTitleAttribute) == title
    }
}

func openTray(_ application: AXUIElement) throws {
    if try !menuItems(application, title: "Quit PR Sniper").isEmpty { return }
    guard let extras = try extrasMenu(application), try sameProcess(extras, application) else {
        throw PollingSmokeFailure.failed("No owned AXExtrasMenuBar")
    }
    let items = try descendants(extras).filter {
        try budget.check()
        return try sameProcess($0, application) && text($0, kAXRoleAttribute) == kAXMenuBarItemRole
    }
    try perform(items, application: application, role: kAXMenuBarItemRole)
    try waitFor("owned native tray menu") {
        try menuItems(application, title: "Quit PR Sniper").count == 1
    }
}

@discardableResult
func choose(_ application: AXUIElement, title: String, beforeAction: (() throws -> Void)? = nil) throws -> Int64 {
    try openTray(application)
    try beforeAction?()
    let started = Int64(Date().timeIntervalSince1970)
    try perform(menuItems(application, title: title), application: application,
                role: kAXMenuItemRole, title: title)
    return started
}

func ownedWindows(_ snapshot: [[String: Any]]?, pid: pid_t) throws -> [CGRect] {
    try require(pid > 0, "Invalid owned PID")
    guard let snapshot else { throw PollingSmokeFailure.failed("CG window snapshot unavailable") }
    var result: [CGRect] = []
    for row in snapshot {
        guard let owner = row[kCGWindowOwnerPID as String] as? Int,
              let layer = row[kCGWindowLayer as String] as? Int else {
            throw PollingSmokeFailure.failed("Malformed CG window identity")
        }
        if owner != Int(pid) || layer != 0 { continue }
        guard let bounds = row[kCGWindowBounds as String] as? NSDictionary,
              let rect = CGRect(dictionaryRepresentation: bounds),
              let alpha = row[kCGWindowAlpha as String] as? Double,
              let onscreen = row[kCGWindowIsOnscreen as String] as? Bool,
              validRect(rect), alpha.isFinite, (0...1).contains(alpha) else {
            throw PollingSmokeFailure.failed("Owned CG window metadata unavailable")
        }
        if onscreen && alpha > 0 { result.append(rect) }
    }
    return result
}

func screenWindows(_ application: AXUIElement) throws -> [CGRect] {
    guard let pid = try elementPID(application) else {
        throw PollingSmokeFailure.failed("Cannot identify owned application")
    }
    return try screenWindows(pid: pid)
}

func screenWindows(pid: pid_t) throws -> [CGRect] {
    try ownedWindows(CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]], pid: pid)
}

func validRect(_ rect: CGRect) -> Bool {
    [rect.origin.x, rect.origin.y, rect.width, rect.height].allSatisfy { $0.isFinite }
        && rect.width > 0 && rect.height > 0 && !rect.isNull
}

func whollyVisible(_ rect: CGRect?, within clips: [CGRect]) -> Bool {
    guard let rect, validRect(rect), !clips.isEmpty else { return false }
    return clips.allSatisfy { validRect($0) && $0.contains(rect) }
}

func frame(_ element: AXUIElement) throws -> CGRect? {
    guard let position = try attribute(element, kAXPositionAttribute),
          let size = try attribute(element, kAXSizeAttribute),
          CFGetTypeID(position) == AXValueGetTypeID(), CFGetTypeID(size) == AXValueGetTypeID() else { return nil }
    var point = CGPoint.zero
    var extent = CGSize.zero
    guard AXValueGetValue(unsafeBitCast(position, to: AXValue.self), .cgPoint, &point),
          AXValueGetValue(unsafeBitCast(size, to: AXValue.self), .cgSize, &extent) else { return nil }
    return CGRect(origin: point, size: extent)
}

func visibleWindow(_ application: AXUIElement, title: String) throws -> AXUIElement? {
    func matches(_ element: AXUIElement) throws -> Bool {
        try budget.check()
        return try !CFEqual(element, application) && sameProcess(element, application)
            && text(element, kAXRoleAttribute) == kAXWindowRole
            && text(element, kAXTitleAttribute) == "PR Sniper - \(title)"
            && attribute(element, kAXMinimizedAttribute) as? Bool == false
    }
    var value: CFTypeRef?
    try require(try axMessage(application, {
        AXUIElementCopyAttributeValue(application, kAXWindowsAttribute as CFString, &value)
    }) == .success,
                "AX windows snapshot unavailable")
    guard let value, CFGetTypeID(value) == CFArrayGetTypeID() else {
        throw PollingSmokeFailure.failed("AX windows snapshot is not an array")
    }
    let listed = try observedAXElements(value)
    var matching = try listed.filter(matches)
    if matching.isEmpty && listed.contains(where: { CFEqual($0, application) }) {
        var direct: CFArray?
        try require(try axMessage(application, {
            AXUIElementCopyAttributeValues(application, kAXWindowsAttribute as CFString, 0, 10, &direct)
        }) == .success && direct != nil,
                    "Direct AX windows snapshot unavailable")
        matching = try observedAXElements(direct).filter(matches)
    }
    try require(matching.count <= 1, "Ambiguous owned \(title) windows")
    let onscreen = try screenWindows(application)
    guard let window = matching.first, let bounds = try frame(window),
          onscreen.contains(where: {
              abs($0.minX - bounds.minX) < 2 && abs($0.minY - bounds.minY) < 2
                  && abs($0.width - bounds.width) < 2 && abs($0.height - bounds.height) < 2
          }) else { return nil }
    print("WINDOW owned title=\(title) AXWindow bounds=\(bounds) matching healthy CG metadata")
    return window
}

func assertHidden(_ application: AXUIElement, _ process: Process) throws {
    try require(process.isRunning, "Owned host exited during hidden interval")
    try require(try screenWindows(application).isEmpty, "Owned normal window is still on screen")
}

func closeWindow(_ application: AXUIElement, title: String) throws {
    guard let window = try visibleWindow(application, title: title),
          let value = try attribute(window, kAXCloseButtonAttribute),
          CFGetTypeID(value) == AXUIElementGetTypeID() else {
        throw PollingSmokeFailure.failed("No AX close button for \(title)")
    }
    let button = unsafeBitCast(value, to: AXUIElement.self)
    try require(try sameProcess(window, application), "Window identity changed before close")
    try require(try text(button, kAXSubroleAttribute) == kAXCloseButtonSubrole,
                "Window close attribute is not a semantic close button")
    try perform([button], application: application, role: kAXButtonRole)
    try waitFor("\(title) no longer on screen (healthy CG snapshot)") {
        try screenWindows(application).isEmpty
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

func completedAttempt(_ health: Health, after previous: Int64?) -> Bool {
    guard let attempt = health.last_attempt, let success = health.last_success else { return false }
    return (previous == nil || attempt > previous!) && success >= attempt
        && !health.in_flight && health.last_failure == nil
}

func validateImmediate(_ current: Health, baseline: Health, action: Int64, observed: Int64) throws {
    try require(completedAttempt(current, after: baseline.last_attempt),
                "Check Now did not produce a distinct successful attempt")
    try require(action < baseline.next_run && observed < baseline.next_run
                    && current.last_attempt! >= action && current.last_attempt! < baseline.next_run
                    && current.last_success! <= observed && current.last_success! < baseline.next_run
                    && (baseline.last_attempt == nil || action > baseline.last_attempt!)
                    && current.repository_id == baseline.repository_id
                    && (baseline.last_success == nil || current.last_success! > baseline.last_success!)
                    && current.next_run == baseline.next_run,
                "Check Now crossed original due time, retained stale success, or changed cadence")
}

struct ExpectedQueue: Decodable {
    let repository_id: String
    let repository_name: String
    let pull_request_id: String
    let number: Int
    let title: String
    let head_sha: String
    let account_id: String
    let watched_author_id: String
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

func expectedRow(_ bytes: [String: Data], _ expected: ExpectedQueue) throws -> QueueRow {
    guard let data = bytes["state/queue.json"] else {
        throw PollingSmokeFailure.failed("Queue snapshot missing")
    }
    let rows = try JSONDecoder().decode([QueueRow].self, from: data)
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

func validateRetainedRow(_ repeated: QueueRow, original: QueueRow) throws {
    try require(repeated.provider == original.provider && repeated.repository_id == original.repository_id
                    && repeated.pull_request_id == original.pull_request_id && repeated.head_sha == original.head_sha
                    && repeated.trigger_policy == original.trigger_policy && repeated.detected_at == original.detected_at,
                "Repeat poll replaced or duplicated the same revision-policy job")
}

func assertQueueVisible(_ application: AXUIElement, _ expected: ExpectedQueue) throws {
    try choose(application, title: "Review Queue")
    try waitFor("expected live revision rendered in native Queue", seconds: 30) {
        guard let window = try visibleWindow(application, title: "Review Queue") else { return false }
        let strings = try visibleStrings(window, application: application)
        return strings.contains("\(expected.repository_name) #\(expected.number): \(expected.title)")
            && strings.contains("Head \(expected.head_sha). Trigger: watched author. Trust confirmation required; execution unavailable.")
    }
    try closeWindow(application, title: "Review Queue")
}

func visibleStrings(_ window: AXUIElement, application: AXUIElement) throws -> [String] {
    guard let bounds = try frame(window), validRect(bounds) else {
        throw PollingSmokeFailure.failed("Visible window geometry unavailable")
    }
    var pending: [(AXUIElement, [CGRect], Int)] = [(window, [bounds], 0)]
    var visited: [AXUIElement] = []
    var result: [String] = []
    while let (element, clips, depth) = pending.popLast() {
        try budget.check()
        try require(depth <= 24 && visited.count < 2500, "Visible AX traversal exceeded bound")
        if visited.contains(where: { CFEqual($0, element) }) { continue }
        visited.append(element)
        try require(try sameProcess(element, application), "Visible AX descendant changed owner")
        if try attribute(element, "AXHidden") as? Bool == true { continue }
        let role = try text(element, kAXRoleAttribute)
        var childClips = clips
        if ["AXScrollArea", "AXWebArea"].contains(role) {
            guard let clip = try frame(element), validRect(clip) else {
                throw PollingSmokeFailure.failed("Visible content clip unavailable")
            }
            childClips.append(clip)
        }
        // Only text leaves count: a container's accessible name can contain offscreen descendants.
        if try role == kAXStaticTextRole && whollyVisible(frame(element), within: childClips) {
            let strings = try [kAXValueAttribute, kAXTitleAttribute].map { try text(element, $0) }
                .filter { !$0.isEmpty }
            result.append(contentsOf: strings)
            if !strings.isEmpty { print("VISIBLE text=\(strings) bounds=\(String(describing: try frame(element)))") }
        }
        pending.append(contentsOf: try observedAXChildren(element).map { ($0, childClips, depth + 1) })
    }
    return result
}

func normalizedText(_ value: String) -> String {
    value.replacingOccurrences(of: "\u{202f}", with: " ")
        .replacingOccurrences(of: "\u{00a0}", with: " ")
}

func renderedTime(_ seconds: Int64) -> String {
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.timeZone = .current
    formatter.dateFormat = "M/d/yyyy, h:mm:ss a"
    return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(seconds)))
}

func assertStatusVisible(_ application: AXUIElement, name: String, health: Health) throws {
    guard let attempted = health.last_attempt, let succeeded = health.last_success else {
        throw PollingSmokeFailure.failed("Status requires a completed health snapshot")
    }
    try choose(application, title: "Status")
    try waitFor("visible Status matches completed health snapshot") {
        guard let window = try visibleWindow(application, title: "Status") else { return false }
        let strings = try visibleStrings(window, application: application).map(normalizedText)
        let expected = "\(name): Waiting. Last attempt: \(renderedTime(attempted)). Last success: \(renderedTime(succeeded)). Next run: \(renderedTime(health.next_run)). Last failure: None."
        return strings.contains(normalizedText(expected))
    }
    try closeWindow(application, title: "Status")
}

let stableFiles = ["config/settings.json", "state/polling.json", "state/queue.json", "state/poll-cursors.json"]

func validateStableState(_ actual: [String: Data], _ expected: [String: Data]) throws {
    try require(Set(actual.keys) == Set(stableFiles) && Set(expected.keys) == Set(stableFiles)
                    && actual == expected,
                "Polling/queue/cursor/config snapshot missing or changed")
}

func stateBytes(_ root: URL) throws -> [String: Data] {
    try Dictionary(uniqueKeysWithValues: stableFiles.map {
        ($0, try Data(contentsOf: root.appendingPathComponent($0)))
    })
}

func preserveState(_ root: URL, evidence: URL, label: String, config: Data) throws -> [String: Data] {
    let bytes = try stateBytes(root)
    try require(bytes["config/settings.json"] == config, "Saved configuration changed")
    let folder = evidence.appendingPathComponent(label, isDirectory: true)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
    for (name, data) in bytes {
        try data.write(to: folder.appendingPathComponent(name.replacingOccurrences(of: "/", with: "-")))
    }
    try Data(contentsOf: root.appendingPathComponent("state/diagnostics.jsonl"))
        .write(to: folder.appendingPathComponent("diagnostics.jsonl"))
    try validateStableState(stateBytes(root), bytes)
    print("SNAPSHOT \(label) path=\(folder.path)")
    return bytes
}

struct Cursor: Decodable {
    let name: String
    let account_id: String
    let remote_id: String
    let trigger_policy: String
    let updated_after: String?
}

func validateIdentity(_ bytes: [String: Data], localID: String, expected: ExpectedQueue,
                      row: QueueRow) throws {
    guard let data = bytes["state/poll-cursors.json"],
          let cursor = try JSONDecoder().decode([String: Cursor].self, from: data)[localID] else {
        throw PollingSmokeFailure.failed("Expected configured repository cursor missing")
    }
    try require(cursor.name == expected.repository_name && cursor.account_id == expected.account_id
                    && cursor.remote_id == expected.repository_id && cursor.trigger_policy == row.trigger_policy,
                "Cursor account/repository/policy identity mismatch")
    let policy = try JSONSerialization.jsonObject(with: Data(cursor.trigger_policy.utf8)) as? NSArray
    let expectedPolicy: NSArray = [[expected.watched_author_id], false, expected.account_id]
    try require(policy?.isEqual(expectedPolicy) == true && cursor.updated_after != nil,
                "Cursor trigger policy or incremental watermark not established")
}

struct OwnedProcess: Equatable {
    let pid: pid_t
    let parent: pid_t
    let seconds: UInt64
    let micros: UInt64
    let executable: String

    // Reparenting and exec do not end a recorded child's lifetime; PID reuse does.
    static func == (lhs: OwnedProcess, rhs: OwnedProcess) -> Bool {
        lhs.pid == rhs.pid && lhs.seconds == rhs.seconds && lhs.micros == rhs.micros
    }
}

var launchedIdentity: OwnedProcess?

func processIdentity(_ pid: pid_t) throws -> OwnedProcess? {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    let copied = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size)
    if copied == 0 && errno == ESRCH { return nil }
    try require(copied == size, "Process identity unavailable for PID \(pid), errno=\(errno)")
    // PROC_PIDPATHINFO_MAXSIZE is an unimportable C macro: 4 * MAXPATHLEN.
    var path = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
    let pathBytes = proc_pidpath(pid, &path, UInt32(path.count))
    if pathBytes <= 0 && errno == ESRCH { return nil }
    try require(pathBytes > 0, "Process executable unavailable for PID \(pid), errno=\(errno)")
    return OwnedProcess(pid: pid, parent: pid_t(info.pbi_ppid),
                        seconds: info.pbi_start_tvsec, micros: info.pbi_start_tvusec,
                        executable: String(cString: path))
}

func childPIDs(_ pids: [pid_t], count: Int32, error: Int32) throws -> [pid_t] {
    try require(error == 0 && count >= 0 && Int(count) < pids.count,
                "Owned child snapshot unavailable or truncated: count=\(count) capacity=\(pids.count) errno=\(error)")
    let children = Array(pids.prefix(Int(count)))
    try require(children.allSatisfy { $0 > 0 } && Set(children).count == children.count,
                "Owned child snapshot contains invalid or duplicate PIDs")
    return children
}

func readChildPIDs(_ parent: pid_t) throws -> [pid_t] {
    var pids = [pid_t](repeating: 0, count: 256)
    errno = 0
    let count = proc_listchildpids(parent, &pids, Int32(pids.count * MemoryLayout<pid_t>.size))
    let error = errno
    print("CHILD snapshot parent=\(parent) count=\(count) capacity=\(pids.count) errno=\(error)")
    return try childPIDs(pids, count: count, error: error)
}

func ownedChildren(_ parent: OwnedProcess, identity: (pid_t) throws -> OwnedProcess?,
                   enumerate: (pid_t) throws -> [pid_t],
                   check: () throws -> Void) throws -> [OwnedProcess] {
    func validateParent() throws {
        try check()
        try require(try identity(parent.pid) == parent,
                    "Parent exited or changed lifetime during child acquisition: \(parent.pid)")
    }
    try validateParent()
    let pids = try enumerate(parent.pid)
    try validateParent()
    var result: [OwnedProcess] = []
    for pid in pids {
        try check()
        guard let child = try identity(pid) else { continue }
        try require(child.pid == pid && child.parent == parent.pid && child != parent,
                    "Child ancestry changed during observation")
        result.append(child)
    }
    // These userspace checks are not atomic with enumeration or later signaling.
    try validateParent()
    return result
}

func sampleDescendants(_ owner: OwnedProcess, observed: inout [OwnedProcess],
                       identity: (pid_t) throws -> OwnedProcess?,
                       enumerate: (pid_t) throws -> [pid_t],
                       check: () throws -> Void) throws {
    var pending = [owner]
    var seen: [OwnedProcess] = []
    while let parent = pending.popLast() {
        try check()
        try require(seen.count < 128, "Owned descendant observation exceeded bound")
        if seen.contains(parent) { continue }
        seen.append(parent)
        let children = try ownedChildren(parent, identity: identity, enumerate: enumerate, check: check)
        for child in children {
            if !observed.contains(child) {
                observed.append(child)
                print("CHILD observed pid=\(child.pid) parent=\(child.parent) birth=\(child.seconds).\(child.micros)")
            }
            pending.append(child)
        }
    }
}

func readHealth(_ root: URL, repositoryID: String) throws -> Health? {
    let path = root.appendingPathComponent("state/polling.json")
    if !FileManager.default.fileExists(atPath: path.path) { return nil }
    let rows = try JSONDecoder().decode([Health].self, from: Data(contentsOf: path))
    try require(rows.count == 1 && rows[0].repository_id == repositoryID,
                "Polling snapshot does not uniquely identify the configured repository")
    return rows[0]
}

func run() throws {
    try budget.check()
    if CommandLine.arguments.dropFirst() == ["--preflight"] {
        try preflight()
        print("PASS AX-only preflight; no app launched or permission changed")
        return
    }
    try require([3, 4].contains(CommandLine.arguments.count),
                "Usage: macos-polling-smoke /absolute/PR Sniper.app /absolute/owned-data-fixture [/absolute/expected-queue.json]")
    try preflight()
    try require(CommandLine.arguments[1].hasPrefix("/") && CommandLine.arguments[2].hasPrefix("/"),
                "Bundle and owned fixture paths must be absolute")
    let bundleURL = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
    let root = URL(fileURLWithPath: CommandLine.arguments[2]).standardizedFileURL
    try require(root == root.resolvingSymlinksInPath(),
                "Supply the resolved owned fixture path, not a symlink")
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
    for name in stableFiles.dropFirst() + ["state/diagnostics.jsonl", "state/diagnostics.previous.jsonl"] {
        try require(!FileManager.default.fileExists(atPath: root.appendingPathComponent(name).path),
                    "Supply a fresh owned fixture; existing \(name) will not be overwritten")
    }
    let settingsBytes = try Data(contentsOf: root.appendingPathComponent("config/settings.json"))
    guard let settings = try JSONSerialization.jsonObject(with: settingsBytes) as? [String: Any],
          settings["launch_at_login"] as? Bool == false,
          let defaults = settings["defaults"] as? [String: Any],
          let repositories = settings["repositories"] as? [[String: Any]],
          repositories.count == 1,
          let repository = repositories.first,
          repository["enabled"] as? Bool == true,
          let repositoryID = repository["id"] as? String,
          let repositoryName = repository["name"] as? String else {
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
    if let expected {
        guard let authors = (overrides["watched_authors"] ?? defaults["watched_authors"]) as? [[String: Any]] else {
            throw PollingSmokeFailure.failed("Fixture watched authors unavailable")
        }
        try require(repositoryName == expected.repository_name && authors.count == 1
                        && authors[0]["id"] as? String == expected.watched_author_id
                        && (overrides["reviewer_assignment"] ?? defaults["reviewer_assignment"]) as? Bool == false,
                    "Fixture must select exactly the approved repository/author and disable reviewer trigger")
    }
    let evidence = root.appendingPathComponent("polling-smoke-evidence", isDirectory: true)
    try require(!FileManager.default.fileExists(atPath: evidence.path),
                "Existing harness evidence must not be overwritten")
    try FileManager.default.createDirectory(at: evidence, withIntermediateDirectories: false)
    try settingsBytes.write(to: evidence.appendingPathComponent("settings-before.json"))

    let process = Process()
    process.executableURL = executable
    process.environment = ProcessInfo.processInfo.environment.merging([
        "PR_SNIPER_DATA_DIR": root.path
    ]) { _, new in new }
    var owner: OwnedProcess?
    var observedChildren: [OwnedProcess] = []
    func survivingChildren() throws -> [OwnedProcess] {
        try observedChildren.filter { try processIdentity($0.pid) == $0 }
    }
    func resourcesAlive() throws -> Bool {
        if process.isRunning { return true }
        return try !survivingChildren().isEmpty
    }
    func sampleChildren() throws {
        try require(process.isRunning, "Owned process exited before child observation")
        guard let owner else {
            throw PollingSmokeFailure.failed("Owned process birth identity unavailable")
        }
        try sampleDescendants(owner, observed: &observedChildren, identity: processIdentity,
                              enumerate: readChildPIDs, check: budget.check)
    }
    defer {
        let cleanupEnd = min(budget.start + budget.total, ProcessInfo.processInfo.systemUptime + 8)
        var cleanupFailed = false
        do {
            if process.isRunning {
                try require(owner != nil && owner == processIdentity(process.processIdentifier),
                            "Cleanup refused: owned process identity changed/unavailable")
                process.terminate()
                print("CLEANUP SIGTERM owned PID \(process.processIdentifier); never natural Quit")
            }
            for child in observedChildren {
                if try processIdentity(child.pid) == child {
                    try require(kill(child.pid, SIGTERM) == 0 || errno == ESRCH,
                                "Cannot stop recorded owned child \(child.pid)")
                    print("CLEANUP SIGTERM recorded child \(child.pid)")
                }
            }
            while ProcessInfo.processInfo.systemUptime < cleanupEnd - 2 {
                if try !resourcesAlive() { break }
                RunLoop.current.run(until: Date().addingTimeInterval(0.05))
            }
            if process.isRunning {
                try require(owner == processIdentity(process.processIdentifier),
                            "Cleanup refused: PID identity changed before SIGKILL")
                try require(kill(process.processIdentifier, SIGKILL) == 0 || errno == ESRCH,
                            "Cannot kill exact owned app")
            }
            for child in observedChildren {
                if try processIdentity(child.pid) == child {
                    try require(kill(child.pid, SIGKILL) == 0 || errno == ESRCH,
                                "Cannot kill recorded child \(child.pid)")
                }
            }
            while ProcessInfo.processInfo.systemUptime < cleanupEnd {
                if try !resourcesAlive() { break }
                RunLoop.current.run(until: Date().addingTimeInterval(0.05))
            }
            try require(!process.isRunning, "Owned app still running after bounded cleanup")
            for child in observedChildren {
                try require(try processIdentity(child.pid) != child,
                            "Recorded child remains after cleanup: \(child.pid)")
            }
            print("CLEANUP exact owned process/observed descendants absent")
        } catch {
            cleanupFailed = true
            fputs("CLEANUP FAILED \(error); preserve custody and stop the exact recorded resources\n", stderr)
        }
        print("PRESERVED owned data and evidence at \(root.path)")
        if cleanupFailed { exit(1) }
    }
    try budget.check()
    try process.run()
    owner = try processIdentity(process.processIdentifier)
    try require(owner != nil, "Launched process identity unavailable")
    try require(owner?.executable == executable.resolvingSymlinksInPath().path,
                "Launched executable does not match supplied immutable bundle")
    launchedIdentity = owner
    print("OBSERVE bundle=\(bundleURL.path) executable=\(executable.path) pid=\(process.processIdentifier) data=\(root.path)")
    let application = AXUIElementCreateApplication(process.processIdentifier)
    try waitFor("native startup and persisted schedule") {
        let health = try readHealth(root, repositoryID: repositoryID)
        return try process.isRunning && extrasMenu(application) != nil && health != nil
    }
    try enableOwnedAccessibilityTree(application)
    try assertHidden(application, process)
    try sampleChildren()

    for title in ["Settings", "Review Queue"] {
        try choose(application, title: title)
        try waitFor("\(title) native AX window") {
            try visibleWindow(application, title: title) != nil
        }
        try closeWindow(application, title: title)
        try require(process.isRunning, "Closing \(title) stopped the host")
    }
    try assertHidden(application, process)
    let initialAttempt = try readHealth(root, repositoryID: repositoryID)?.last_attempt
    var hiddenSamples = 0
    try waitFor("scheduled check with all windows closed", seconds: 95) {
        try assertHidden(application, process)
        try sampleChildren()
        hiddenSamples += 1
        guard let health = try readHealth(root, repositoryID: repositoryID) else { return false }
        print("HIDDEN sample=\(hiddenSamples) monotonic=\(ProcessInfo.processInfo.systemUptime) pid=\(process.processIdentifier) normalWindows=0 attempt=\(String(describing: health.last_attempt)) success=\(String(describing: health.last_success)) inFlight=\(health.in_flight)")
        try require(health.last_failure == nil, "Hidden scheduled provider check failed")
        return completedAttempt(health, after: initialAttempt)
    }
    guard let scheduled = try readHealth(root, repositoryID: repositoryID) else {
        throw PollingSmokeFailure.failed("Scheduled health disappeared")
    }
    try require(completedAttempt(scheduled, after: initialAttempt),
                "Scheduled provider check failed; successful native polling remains UNVERIFIED")
    try assertHidden(application, process)
    let scheduledBytes = try preserveState(root, evidence: evidence, label: "scheduled", config: settingsBytes)
    print("PASS scheduled attempt=\(scheduled.last_attempt!) success=\(scheduled.last_success!) hidden samples=\(hiddenSamples); sampled, not continuous observation")
    var firstQueueRow: QueueRow?
    if let expected {
        firstQueueRow = try expectedRow(scheduledBytes, expected)
        try validateIdentity(scheduledBytes, localID: repositoryID, expected: expected, row: firstQueueRow!)
        try assertQueueVisible(application, expected)
        print("PASS expected live revision persisted and rendered with watched-author/trust-confirmation state")
    }
    try assertStatusVisible(application, name: repositoryName, health: scheduled)

    var beforeImmediate = scheduled
    try waitFor("idle cadence with time for Check Now", seconds: 95) {
        guard let health = try readHealth(root, repositoryID: repositoryID),
              let attempted = health.last_attempt else { return false }
        let now = Int64(Date().timeIntervalSince1970)
        try assertHidden(application, process)
        guard !health.in_flight && now > attempted && health.next_run - now > 30 else { return false }
        beforeImmediate = health
        return true
    }
    let action = try choose(application, title: "Check Now", beforeAction: {
        guard let latest = try readHealth(root, repositoryID: repositoryID) else {
            throw PollingSmokeFailure.failed("Health vanished before Check Now")
        }
        let now = Int64(Date().timeIntervalSince1970)
        try require(!latest.in_flight && latest.last_attempt == beforeImmediate.last_attempt
                        && latest.next_run == beforeImmediate.next_run && latest.next_run - now > 25,
                    "Check Now baseline changed or too near original due; stop, do not attribute scheduled work")
    })
    try waitFor("Check Now completes successfully before original due", seconds: 30) {
        try assertHidden(application, process)
        try sampleChildren()
        try require(Int64(Date().timeIntervalSince1970) < beforeImmediate.next_run,
                    "Check Now crossed original due; native attribution UNVERIFIED")
        guard let health = try readHealth(root, repositoryID: repositoryID) else { return false }
        try require(health.last_failure == nil && health.next_run == beforeImmediate.next_run,
                    "Check Now failed or cadence changed")
        return completedAttempt(health, after: beforeImmediate.last_attempt)
    }
    guard let immediate = try readHealth(root, repositoryID: repositoryID) else {
        throw PollingSmokeFailure.failed("Check Now health disappeared")
    }
    try validateImmediate(immediate, baseline: beforeImmediate, action: action,
                          observed: Int64(Date().timeIntervalSince1970))
    let immediateBytes = try preserveState(root, evidence: evidence, label: "immediate", config: settingsBytes)
    print("PASS native Check Now action=\(action) attempt=\(immediate.last_attempt!) success=\(immediate.last_success!) original-next-run=\(beforeImmediate.next_run)")

    for title in ["Settings", "Review Queue"] {
        try choose(application, title: title)
        try waitFor("\(title) reopens") { try visibleWindow(application, title: title) != nil }
        try closeWindow(application, title: title)
    }
    if let expected, let firstQueueRow {
        let repeated = try expectedRow(immediateBytes, expected)
        try validateIdentity(immediateBytes, localID: repositoryID, expected: expected, row: repeated)
        try validateRetainedRow(repeated, original: firstQueueRow)
        try assertQueueVisible(application, expected)
        print("PASS repeat poll retains one revision-policy job and visible Queue; incremental cursor may omit the server row on repeat")
    }
    try assertStatusVisible(application, name: repositoryName, health: immediate)
    guard let beforeQuit = try readHealth(root, repositoryID: repositoryID) else {
        throw PollingSmokeFailure.failed("Final health disappeared")
    }
    let quitNow = Int64(Date().timeIntervalSince1970)
    try require(!beforeQuit.in_flight && beforeQuit.next_run - quitNow > 5,
                "Quit cannot be attributed safely near a scheduled crossover")
    let beforeQuitBytes = try preserveState(root, evidence: evidence, label: "before-quit", config: settingsBytes)
    try sampleChildren()
    let quitAction = try choose(application, title: "Quit PR Sniper", beforeAction: {
        try validateStableState(stateBytes(root), beforeQuitBytes)
        try require(beforeQuit.next_run - Int64(Date().timeIntervalSince1970) > 3,
                    "Original due too near immediately before Quit action")
    })
    try waitFor("actual native Quit exits owned process") { !process.isRunning }
    try require(process.terminationReason == .exit && process.terminationStatus == 0,
                "Native Quit did not exit normally with status zero")
    for child in observedChildren {
        try require(try processIdentity(child.pid) != child, "Observed owned child survived Quit: \(child.pid)")
    }
    let diagnostics = try String(contentsOf: root.appendingPathComponent("state/diagnostics.jsonl"), encoding: .utf8)
    let events = try diagnostics.split(separator: "\n").map {
        try JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any]
    }
    try require(events.contains {
        $0?["event"] as? String == "quit_requested"
            && ($0?["timestamp_secs"] as? Int64).map { $0 >= quitAction } == true
    }, "Fresh quit_requested diagnostic missing")
    try require(try screenWindows(pid: process.processIdentifier).isEmpty, "Owned normal window survived Quit")
    print("UNVERIFIED physical tray disappearance; no AX message sent to the exited process")
    let afterQuitBytes = try preserveState(root, evidence: evidence, label: "after-quit", config: settingsBytes)
    try validateStableState(afterQuitBytes, beforeQuitBytes)
    let interval = TimeInterval(beforeQuit.next_run + 2) - Date().timeIntervalSince1970
    try require(interval > 0 && interval < 90
                    && interval < budget.remaining(at: ProcessInfo.processInfo.systemUptime),
                "Insufficient bounded time to observe across captured next due")
    let postQuitDeadline = ProcessInfo.processInfo.systemUptime + interval
    while ProcessInfo.processInfo.systemUptime < postQuitDeadline {
        try budget.check()
        try require(!process.isRunning, "Owned process running after Quit")
        try validateStableState(stateBytes(root), beforeQuitBytes)
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    try require(Date().timeIntervalSince1970 >= TimeInterval(beforeQuit.next_run + 2),
                "Wall clock changed; captured next due was not crossed")
    let finalBytes = try preserveState(root, evidence: evidence, label: "after-due", config: settingsBytes)
    try validateStableState(finalBytes, beforeQuitBytes)
    try require(try screenWindows(pid: process.processIdentifier).isEmpty, "Owned window present after next due")
    print("PASS natural Quit exact PID exit and state stable through next_due+2; observed child identities=\(observedChildren.count)")
    print("UNVERIFIED unsampled transient/reparented children, physical tray icon disappearance and occlusion by other applications")
    if expected == nil {
        print("UNVERIFIED eligible-live queue contents: no expected live fixture supplied")
    }
    print("UNVERIFIED icon appearance and review execution; no review or provider mutation performed")
}

#if !POLLING_GUARD_TESTS
@main
struct PollingSmoke {
    static func main() {
        do {
            try run()
        } catch {
            fputs("FAIL \(error)\n", stderr)
            exit(1)
        }
    }
}
#endif
