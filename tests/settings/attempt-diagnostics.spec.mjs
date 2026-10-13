import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";

for (const surface of ["panel", "standalone"]) {
  test(`${surface} Diagnostics distinguishes an empty journal from unavailable evidence`, async ({
    page,
    store,
  }) => {
    expect(await store("diagnostics")).toEqual([]);
    await page.goto(surface === "panel" ? "/" : "/?view=diagnostics");
    if (surface === "panel")
      await page.locator("[data-panel-diagnostics]").click();
    const log = page.locator(
      surface === "panel" ? '[data-panel-view="utility"] pre' : "#log",
    );
    await expect(log).toHaveText(/\n\nNo events$/);
    await page.evaluate(() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        command === "diagnostics"
          ? Promise.reject("Synthetic diagnostic journal read failure")
          : original(command, args);
    });
    await page
      .getByRole("button", {
        name: surface === "panel" ? "Refresh diagnostics" : "Refresh",
        exact: true,
      })
      .click();
    await expect(
      page.locator(surface === "panel" ? "[data-panel-error]" : "#error"),
    ).toHaveText("Failed");
    if (surface === "panel")
      await expect(page.locator("[data-panel-error]")).toHaveAttribute(
        "title",
        "Synthetic diagnostic journal read failure",
      );
    if (surface === "panel") await expect(log).toHaveCount(0);
    else await expect(log).toHaveText("");
  });

  test(`${surface} Diagnostics renders persisted host and attempt evidence as text`, async ({
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
    await page.goto(surface === "panel" ? "/" : "/?view=diagnostics");
    if (surface === "panel")
      await page.locator("[data-panel-diagnostics]").click();
    const log = page.locator(
      surface === "panel" ? '[data-panel-view="utility"] pre' : "#log",
    );
    await expect(log).toContainText("attempt-153");
    await expect(log).toContainText("1970-01-01T00:16:39.000Z  window_opened");
    await expect(log).toContainText("incomplete_coverage");
    await expect(log).toContainText("1 of 1 files read");
    await expect(log).toContainText("0 of 1 files read");
    await expect(log).toContainText("not_exposed");
    await expect(log).toContainText("1234");
    await expect(log).toContainText(paths.paths[0]);
    await expect(log).not.toContainText("No events");
    await expect(log.locator("img")).toHaveCount(0);
  });
}
