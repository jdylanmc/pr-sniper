import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";
import {
  clipboardWrites,
  copyDiagnostics,
  recordClipboard,
} from "./diagnostics-copy.mjs";

for (const surface of ["panel", "standalone"]) {
  const panel = surface === "panel";
  test(`${surface} Copy diagnostics distinguishes an empty journal from unavailable evidence`, async ({
    page,
    store,
  }) => {
    expect(await store("diagnostics")).toEqual([]);
    await recordClipboard(page);
    await page.goto(panel ? "/" : "/?view=settings");
    const text = await copyDiagnostics(page, { panel });
    expect(text).toMatch(/\n\nNo events\n\nRecorded job Intelligence\nNone\n$/);
    await page.evaluate(() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        command === "diagnostics"
          ? Promise.reject("Synthetic diagnostic journal read failure")
          : original(command, args);
    });
    const button = page.getByRole("button", { name: "Copy diagnostics" });
    await button.click();
    await expect(button).toHaveAttribute("data-state", "failed");
    await expect(page.locator("#error")).toHaveAttribute(
      "title",
      "Synthetic diagnostic journal read failure",
    );
    expect(await clipboardWrites(page)).toEqual([text]);
  });

  test(`${surface} Copy diagnostics surfaces a denied clipboard without a success state`, async ({
    page,
  }) => {
    await recordClipboard(page, { fail: true });
    await page.goto(panel ? "/" : "/?view=settings");
    expect(await copyDiagnostics(page, { panel })).toBeUndefined();
    await expect(
      page.getByRole("button", { name: "Copy diagnostics" }),
    ).toHaveAttribute("data-state", "failed");
    await expect(page.locator("#error")).toHaveAttribute(
      "title",
      "Copy failed",
    );
  });

  test(`${surface} Copy diagnostics exports persisted host and attempt evidence as text`, async ({
    page,
    store,
    dataRoot,
  }) => {
    await store("snapshot");
    await mkdir(join(dataRoot, "state"), { recursive: true });
    const paths = {
      count: 1,
      paths: ["src/<img src=x onerror=alert(1)>.rs"],
      omitted: 0,
    };
    const finished = (id, failure, covered) => ({
      timestamp_secs: 1000,
      event: "attempt",
      attempt: {
        attempt: {
          id,
          operation_id: "operation-153",
          work_id: "work-153",
          number: 153,
          head: "a".repeat(40),
          model: "fixture-model",
          started_at_ms: 1_000_000,
          session_id: "session-153",
          runtime_version: "fixture-runtime",
        },
        sequence: 5,
        elapsed_ms: 1234,
        event: {
          kind: "finished",
          stage: "runtime",
          failure,
          coverage: {
            changed: paths,
            covered: covered ? paths : { count: 0, paths: [], omitted: 0 },
            missing: covered ? { count: 0, paths: [], omitted: 0 } : paths,
            responsible_tool: "read_changes",
          },
          stop_reason: "not_exposed",
          session_events: 4,
          summarized_events: 4,
        },
      },
    });
    // New attempts record coverage without a failure; the old failure record still loads.
    const records = [
      finished("attempt-153", null, true),
      finished("attempt-old", "incomplete_coverage", false),
    ];
    await writeFile(
      join(dataRoot, "state", "attempts.jsonl"),
      records.map((record) => JSON.stringify(record)).join("\n") + "\n",
    );
    await writeFile(
      join(dataRoot, "state", "diagnostics.jsonl"),
      JSON.stringify({ timestamp_secs: 999, event: "window_opened" }) + "\n",
    );
    await recordClipboard(page);
    await page.goto(panel ? "/" : "/?view=settings");
    const log = await copyDiagnostics(page, { panel });
    expect(log).toContain("attempt-153");
    expect(log).toContain("1970-01-01T00:16:39.000Z  window_opened");
    expect(log).toContain("incomplete_coverage");
    expect(log).toContain("1 of 1 files read");
    expect(log).toContain("0 of 1 files read");
    expect(log).toContain("not_exposed");
    expect(log).toContain("1234");
    expect(log).toContain(paths.paths[0]);
    expect(log).not.toContain("No events");
    await expect(page.locator("img[src=x]")).toHaveCount(0);
  });
}
