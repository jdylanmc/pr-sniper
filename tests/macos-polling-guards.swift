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
    static func parentAcquisition() throws {
        let owner = OwnedProcess(pid: 42, parent: 1, seconds: 100, micros: 0, executable: "/fixture/app")
        let child = OwnedProcess(pid: 44, parent: 42, seconds: 101, micros: 0, executable: "/fixture/child")
        let replacement = OwnedProcess(pid: 44, parent: 1, seconds: 102, micros: 0, executable: "/other/app")
        let unrelated = OwnedProcess(pid: 55, parent: 44, seconds: 103, micros: 0, executable: "/other/child")
        var observed: [OwnedProcess] = []
        var replaced = false
        try rejects("replacement pending parent cannot grant child ownership") {
            try sampleDescendants(owner, observed: &observed, identity: { pid in
                if pid == 42 { return owner }
                if pid == 44 { return replaced ? replacement : child }
                return unrelated
            }, enumerate: { pid in
                if pid == 42 { return [44] }
                if pid == 44 {
                    replaced = true
                    return [55]
                }
                return []
            }, check: {})
        }
        try require(!observed.contains(unrelated), "Replacement-parent child entered cleanup authority")
        print("PASS rejected acquisition never adds unrelated child to signal set")

        for (name, fresh): (String, OwnedProcess?) in [
            ("parent exited before enumeration", nil),
            ("parent PID reused before enumeration", replacement)
        ] {
            var enumerated = false
            var acquired: [OwnedProcess] = []
            try rejects(name) {
                acquired = try ownedChildren(child, identity: { _ in fresh }, enumerate: { _ in
                    enumerated = true
                    return [55]
                }, check: {})
            }
            try require(!enumerated && acquired.isEmpty, "Stale parent was enumerated or granted signal authority")
        }
        for (name, fresh): (String, OwnedProcess?) in [
            ("parent exited during enumeration", nil),
            ("parent reused during enumeration", replacement)
        ] {
            var enumerated = false
            var inspectedChild = false
            var acquired: [OwnedProcess] = []
            try rejects(name) {
                acquired = try ownedChildren(child, identity: { pid in
                    if pid == 44 { return enumerated ? fresh : child }
                    inspectedChild = true
                    return unrelated
                }, enumerate: { _ in
                    enumerated = true
                    return [55]
                }, check: {})
            }
            try require(!inspectedChild && acquired.isEmpty,
                        "Changed post-enumeration parent admitted a child")
        }
        var parentReads = 0
        var acquired: [OwnedProcess] = []
        try rejects("parent changes while child identities are read") {
            acquired = try ownedChildren(child, identity: { pid in
                if pid == 44 {
                    parentReads += 1
                    return parentReads < 3 ? child : replacement
                }
                return unrelated
            }, enumerate: { _ in [55] }, check: {})
        }
        try require(acquired.isEmpty, "Late parent replacement granted signal authority")
        let reparented = OwnedProcess(pid: 44, parent: 1, seconds: 101, micros: 0, executable: "/fixture/exec")
        let grandchild = OwnedProcess(pid: 56, parent: 44, seconds: 104, micros: 0, executable: "/fixture/grandchild")
        observed = [child]
        try sampleDescendants(child, observed: &observed, identity: { pid in
            pid == 44 ? reparented : grandchild
        }, enumerate: { pid in pid == 44 ? [56] : [] }, check: {})
        try require(observed == [child, grandchild],
                    "A recorded child remains owned through reparent/exec and can own legitimate descendants")
        print("PASS already-owned child reparent/exec preserves lifetime and descendant ownership")
        observed = []
        for _ in 0..<2 {
            try sampleDescendants(owner, observed: &observed, identity: { pid in
                [42: owner, 44: child, 56: grandchild][pid]
            }, enumerate: { pid in [42: [Int32(44)], 44: [Int32(56)]][pid] ?? [] }, check: {})
        }
        try require(observed == [child, grandchild], "Repeated traversal must retain only legitimate lifetimes once")
        print("PASS ordinary descendant traversal and repeated sampling preserve exact ownership")
        try rejects("new child changed ancestry before admission") {
            _ = try ownedChildren(owner, identity: { pid in pid == 42 ? owner : reparented },
                                  enumerate: { _ in [44] }, check: {})
        }
    }

    static func messagingBudget() throws {
        let clock = Budget(start: 100, total: 60, cleanupReserve: 10)
        try require(try clock.messagingTimeout(at: 100) == 2, "AX timeout must retain two-second cap")
        try require(try clock.messagingTimeout(at: 149.5) == 0.25,
                    "Near-deadline AX call must use at most half the remaining work budget")
        for now in [149.8, 149.7, 149.6] {
            let timeout = try clock.messagingTimeout(at: now)
            try require(timeout > 0 && timeout.isFinite && Double(timeout) <= clock.remaining(at: now) / 2,
                        "Float rounding must not increase timeout above selected budget")
        }
        print("PASS timeout cap, positive Float rounding and cleanup reserve")
        for now in [149.95, 150, 151, Double.nan, Double.infinity] {
            try rejects("insufficient or invalid AX budget at \(now)") {
                _ = try clock.messagingTimeout(at: now)
            }
        }

        final class Reference {
            let name = "equal semantic AX object"
        }
        let app = Reference()
        let child = Reference()
        let equalChild = Reference()
        var configured: [Reference] = []
        var events: [String] = []
        var now = 140.0
        for reference in [app, child, equalChild, child] {
            let returned = try boundedAXMessage(reference, limit: clock, now: { now }, configure: { actual, timeout in
                configured.append(actual)
                events.append("configure")
                return actual === reference && timeout > 0 && timeout <= 2
            }, operation: {
                events.append("message")
                now += 0.1
                return 7
            })
            try require(returned == 7, "Bounded AX wrapper changed operation result")
        }
        try require(configured.count == 4 && configured[0] === app && configured[1] === child
                        && configured[2] === equalChild && configured[3] === child
                        && events == ["configure", "message", "configure", "message",
                                      "configure", "message", "configure", "message"],
                    "Every call must configure its actual reference, even equal/repeated references")
        print("PASS exact-reference configuration precedes every message without inheritance/cache")

        var operated = false
        try rejects("failed per-reference timeout configuration prevents message") {
            _ = try boundedAXMessage(child, limit: clock, now: { 140 },
                                     configure: { _, _ in false }, operation: { operated = true })
        }
        try require(!operated, "Message dispatched after failed timeout configuration")
        now = 149
        try rejects("configuration consumed available call budget") {
            _ = try boundedAXMessage(child, limit: clock, now: { now }, configure: { _, _ in
                now = 149.6
                return true
            }, operation: { operated = true })
        }
        try require(!operated, "Message dispatched after configuration made selected timeout unsafe")
        var configuredWithoutBudget = false
        try rejects("no configuration or message may start inside cleanup reserve") {
            _ = try boundedAXMessage(child, limit: clock, now: { 150 }, configure: { _, _ in
                configuredWithoutBudget = true
                return true
            }, operation: { operated = true })
        }
        try require(!configuredWithoutBudget && !operated, "AX work started inside cleanup reserve")
        now = 149
        _ = try boundedAXMessage(child, limit: clock, now: { now }, configure: { _, _ in true },
                                 operation: { now = 149.95 })
        try rejects("budget rechecked between consecutive blocking messages") {
            _ = try boundedAXMessage(child, limit: clock, now: { now }, configure: { _, _ in
                configuredWithoutBudget = true
                return true
            }, operation: { operated = true })
        }
        try require(!configuredWithoutBudget && !operated, "Second message skipped budget admission")
        now = 149
        try rejects("message overrun is explicit failure, never success") {
            _ = try boundedAXMessage(child, limit: clock, now: { now }, configure: { _, _ in true },
                                     operation: { now = 150 })
        }
        try require(clock.remaining(at: now, cleanup: true) == 10, "Work deadline must leave cleanup allowance")
    }

    static func check() throws {
        let nearlySpent = Budget(start: 100, total: 60, cleanupReserve: 10)
        try require(try nearlySpent.messagingTimeout(at: 149.5) <= 0.25,
                    "AX timeout must fit remaining work budget without consuming cleanup reserve")
        print("PASS AX timeout shrinks before cleanup reserve")
        try messagingBudget()
        try parentAcquisition()
        let fourChildren: [Int32] = [41, 42, 43, 44] + Array(repeating: 0, count: 252)
        try require(try childPIDs(fourChildren, count: 4, error: 0) == [41, 42, 43, 44],
                    "proc_listchildpids returns four PIDs, not four bytes")
        print("PASS all four returned child PIDs consumed")
        let pidBuffer = Array(Int32(1)...Int32(256))
        for count: Int32 in [0, 1, 2, 3, 4, 255] {
            let decoded = try childPIDs(pidBuffer, count: count, error: 0)
            try require(decoded.count == Int(count) && decoded == Array(pidBuffer.prefix(Int(count))),
                        "Returned PID count \(count) was not consumed exactly")
            print("PASS child PID count \(count)")
        }
        for (name, count, error): (String, Int32, Int32) in [
            ("saturated 256-PID snapshot", 256, 0),
            ("oversized PID count", 257, 0),
            ("negative enumeration result", -1, 0),
            ("enumeration error", -1, EIO),
            ("zero result with errno", 0, ESRCH),
            ("positive result with errno", 1, EIO)
        ] {
            try rejects(name) { _ = try childPIDs(pidBuffer, count: count, error: error) }
        }
        try rejects("invalid PID in reported range") { _ = try childPIDs([0, 42], count: 1, error: 0) }
        try rejects("duplicate PID in reported range") { _ = try childPIDs([42, 42, 0], count: 2, error: 0) }

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
