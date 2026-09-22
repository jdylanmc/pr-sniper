import Foundation

func rejects(_ name: String, _ body: () throws -> Void) throws {
    do {
        try body()
    } catch is PollingSmokeFailure {
        print("PASS \(name)")
        return
    }
    throw PollingSmokeFailure.failed("Guard accepted invalid observation: \(name)")
}

@main
struct PollingGuardTests {
    static func check() throws {
        let stale = Health(repository_id: "local", last_attempt: 120, last_success: 100,
                           next_run: 160, last_failure: nil, in_flight: false)
        try require(!completedAttempt(stale, after: 100),
                    "A stale last_success must not prove the new attempt completed successfully")
        print("PASS stale success rejected")

        let baseline = Health(repository_id: "local", last_attempt: 100, last_success: 101,
                              next_run: 160, last_failure: nil, in_flight: false)
        func health(attempt: Int64? = 120, success: Int64? = 121, due: Int64 = 160,
                    failure: String? = nil, active: Bool = false) -> Health {
            Health(repository_id: "local", last_attempt: attempt, last_success: success,
                   next_run: due, last_failure: failure, in_flight: active)
        }
        try validateImmediate(health(), baseline: baseline, action: 119, observed: 122)
        print("PASS distinct Check Now completion before original due")
        for (name, value) in [
            ("missing attempt", health(attempt: nil)),
            ("missing success", health(success: nil)),
            ("same attempt", health(attempt: 100)),
            ("older attempt", health(attempt: 99)),
            ("stale success", health(success: 101)),
            ("in-flight", health(active: true)),
            ("provider failure", health(failure: "network")),
            ("changed cadence", health(due: 220)),
            ("future success", health(success: 123)),
            ("scheduled crossover attempt", health(attempt: 160, success: 161)),
            ("completion at original due", health(success: 160))
        ] {
            try rejects(name) {
                try validateImmediate(value, baseline: baseline, action: 119, observed: 122)
            }
        }
        try rejects("observation after due cannot be Check Now proof") {
            try validateImmediate(health(), baseline: baseline, action: 119, observed: 160)
        }
        try rejects("attempt before action cannot be Check Now proof") {
            try validateImmediate(health(), baseline: baseline, action: 121, observed: 122)
        }
        try rejects("action before baseline cannot prove Check Now") {
            try validateImmediate(health(), baseline: baseline, action: 99, observed: 122)
        }

        func action(pid: Int32? = 42, role: String = "AXMenuItem", title: String = "Check Now",
                    enabled: Bool? = true, supported: [String]? = ["AXPress"], alias: Bool = false) -> ActionTarget {
            ActionTarget(pid: pid, role: role, title: title, enabled: enabled,
                         actions: supported, applicationAlias: alias)
        }
        func press(_ targets: [ActionTarget]) throws {
            try validateAction(targets, pid: 42, role: "AXMenuItem", title: "Check Now", action: "AXPress")
        }
        try press([action()])
        print("PASS unique enabled same-PID supported semantic action")
        for (name, targets) in [
            ("missing target", []),
            ("ambiguous target", [action(), action()]),
            ("foreign PID", [action(pid: 43)]),
            ("unavailable PID", [action(pid: nil)]),
            ("application alias", [action(alias: true)]),
            ("wrong role", [action(role: "AXApplication")]),
            ("wrong title", [action(title: "Quit PR Sniper")]),
            ("disabled target", [action(enabled: false)]),
            ("unknown enabled state", [action(enabled: nil)]),
            ("unsupported action", [action(supported: ["AXCancel"])]),
            ("unknown actions", [action(supported: nil)])
        ] {
            try rejects(name) { try press(targets) }
        }

        try rejects("nil CG snapshot is unavailable, not hidden") {
            _ = try ownedWindows(nil, pid: 42)
        }
        try rejects("malformed CG identity is not empty success") {
            _ = try ownedWindows([[:]], pid: 42)
        }
        try rejects("invalid PID cannot prove hidden") { _ = try ownedWindows([], pid: 0) }
        try require(try ownedWindows([], pid: 42).isEmpty, "Healthy empty snapshot must be allowed")
        let rectangle = CGRect(x: 10, y: 20, width: 300, height: 200)
        let nativeRow: [String: Any] = [
            "kCGWindowOwnerPID": 42, "kCGWindowLayer": 0,
            "kCGWindowBounds": rectangle.dictionaryRepresentation,
            "kCGWindowAlpha": 1.0, "kCGWindowIsOnscreen": true
        ]
        try require(try ownedWindows([nativeRow], pid: 42) == [rectangle],
                    "Visible owned normal window must prevent hidden assertion")
        try require(try ownedWindows([nativeRow], pid: 43).isEmpty, "Other PID must not become owned")
        var missingBounds = nativeRow
        missingBounds.removeValue(forKey: "kCGWindowBounds")
        try rejects("missing owned geometry") { _ = try ownedWindows([missingBounds], pid: 42) }
        print("PASS healthy visible/hidden snapshots distinguish process ownership")

        let text = CGRect(x: 20, y: 30, width: 100, height: 20)
        try require(whollyVisible(text, within: [rectangle]), "Contained text should be visible")
        try require(!whollyVisible(nil, within: [rectangle]), "Unknown geometry must fail")
        try require(!whollyVisible(text, within: []), "Missing window clip must fail")
        try require(!whollyVisible(.zero, within: [rectangle]), "Zero-area text must fail")
        try require(!whollyVisible(CGRect(x: 20, y: 210, width: 100, height: 20), within: [rectangle]),
                    "Partly clipped text must not prove visible content")
        try require(!whollyVisible(text, within: [rectangle, CGRect(x: 20, y: 100, width: 100, height: 30)]),
                    "Text outside nested scroll viewport must fail")
        print("PASS visible text requires full window/scroll clipping geometry")

        let expected = ExpectedQueue(repository_id: "900", repository_name: "sample/project",
                                     pull_request_id: "901", number: 7, title: "Fixture",
                                     head_sha: "abcdef", account_id: "70", watched_author_id: "80")
        let queue = """
        [{"provider":"github","repository_id":"900","pull_request_id":"901","head_sha":"abcdef",
        "trigger_policy":"[[\\"80\\"],false,\\"70\\"]","watched_author":true,
        "waiting":"trust_confirmation","detected_at":100}]
        """
        let cursor = """
        {"local":{"name":"sample/project","account_id":"70","remote_id":"900",
        "trigger_policy":"[[\\"80\\"],false,\\"70\\"]","updated_after":"2026-09-22T12:00:00Z"}}
        """
        let state = ["config/settings.json": Data("settings".utf8), "state/polling.json": Data("health".utf8),
                     "state/queue.json": Data(queue.utf8), "state/poll-cursors.json": Data(cursor.utf8)]
        let row = try expectedRow(state, expected)
        try validateIdentity(state, localID: "local", expected: expected, row: row)
        try validateRetainedRow(row, original: row)
        try validateStableState(state, state)
        print("PASS queue/cursor binds approved account, remote, head and trigger policy")
        for file in stableFiles {
            var changed = state
            changed[file] = Data("changed".utf8)
            try rejects("changed \(file) after Quit") { try validateStableState(changed, state) }
            changed.removeValue(forKey: file)
            try rejects("missing \(file) after Quit") { try validateStableState(changed, state) }
        }
        try rejects("two empty snapshots cannot prove stable state") { try validateStableState([:], [:]) }
        var duplicate = state
        duplicate["state/queue.json"] = Data(("[" + queue.dropFirst().dropLast() + "," + queue.dropFirst().dropLast() + "]").utf8)
        try rejects("duplicate same revision/policy") { _ = try expectedRow(duplicate, expected) }
        for (name, original, replacement) in [
            ("account changed", "\"account_id\":\"70\"", "\"account_id\":\"71\""),
            ("remote changed", "\"remote_id\":\"900\"", "\"remote_id\":\"902\""),
            ("repository retargeted", "sample/project", "sample/other"),
            ("cursor policy changed", "[[\\\"80\\\"],false,\\\"70\\\"]", "[[\\\"81\\\"],false,\\\"70\\\"]"),
            ("cursor watermark missing", "\"2026-09-22T12:00:00Z\"", "null")
        ] {
            var wrongIdentity = state
            wrongIdentity["state/poll-cursors.json"] = Data(cursor.replacingOccurrences(of: original, with: replacement).utf8)
            try rejects(name) { try validateIdentity(wrongIdentity, localID: "local", expected: expected, row: row) }
        }
        for (name, original, replacement) in [
            ("expected head absent", "abcdef", "different"),
            ("watched-author reason absent", "\"watched_author\":true", "\"watched_author\":false"),
            ("trust wait missing", "trust_confirmation", "human_start")
        ] {
            var changed = state
            changed["state/queue.json"] = Data(queue.replacingOccurrences(of: original, with: replacement).utf8)
            try rejects(name) { _ = try expectedRow(changed, expected) }
        }
        let replaced = QueueRow(provider: "github", repository_id: "900", pull_request_id: "901",
                                head_sha: "abcdef", trigger_policy: row.trigger_policy, watched_author: true,
                                waiting: "trust_confirmation", detected_at: 120)
        try rejects("same tuple replaced with new detected_at") { try validateRetainedRow(replaced, original: row) }

        let clock = Budget(start: 100, total: 60, cleanupReserve: 10)
        try require(clock.remaining(at: 145) == 5 && clock.remaining(at: 150) == 0
                        && clock.remaining(at: 150, cleanup: true) == 10
                        && clock.remaining(at: 161, cleanup: true) == 0,
                    "Total monotonic deadline must retain cleanup reserve")
        print("PASS total monotonic budget reserves cleanup and never resets per wait")
        let child = OwnedProcess(pid: 44, parent: 42, seconds: 100, micros: 7, executable: "/fixture/gh")
        let orphan = OwnedProcess(pid: 44, parent: 1, seconds: 100, micros: 7, executable: "/fixture/child")
        let reused = OwnedProcess(pid: 44, parent: 1, seconds: 101, micros: 7, executable: "/fixture/gh")
        try require(child == orphan && child != reused,
                    "Recorded descendant remains owned after reparent/exec; PID reuse is not the same child")
        print("PASS cleanup lifetime guard distinguishes reparenting from PID reuse")
        print("PASS offline guard suite; no native API, app, provider, or permissions accessed")
    }

    static func main() {
        do {
            try check()
        } catch {
            fputs("FAIL \(error)\n", stderr)
            exit(1)
        }
    }
}
