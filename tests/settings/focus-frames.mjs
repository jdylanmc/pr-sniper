import { expect } from "./fixtures.mjs";

// Delay application frames only; Playwright's isolated-world actionability
// checks and the real Store bridge continue running.
export async function holdFocusFrames(page) {
  await page.evaluate(() => {
    const request = window.requestAnimationFrame.bind(window);
    const frames = [];
    const trace = [];
    const describe = (element) =>
      element instanceof HTMLElement
        ? {
            tag: element.tagName,
            id: element.id,
            tab: element.dataset.panelTab,
            text: element.textContent?.trim().slice(0, 80),
          }
        : null;
    const record = (event, extra = {}) =>
      trace.push({
        event,
        heading: document.querySelector("[data-panel-heading]")?.textContent,
        active: describe(document.activeElement),
        ...extra,
      });
    const focus = HTMLElement.prototype.focus;
    HTMLElement.prototype.focus = function (...args) {
      record("focus-call", {
        target: describe(this),
        stack: new Error().stack,
      });
      return focus.apply(this, args);
    };
    document.addEventListener("focusin", () => record("focusin"));
    window.requestAnimationFrame = (callback) => {
      const index = frames.length;
      frames.push(callback);
      record("frame-scheduled", { index, stack: new Error().stack });
      return -(index + 1);
    };
    window.__focusFrames = {
      trace,
      frames,
      mark: record,
      release: (index) =>
        new Promise((resolve) =>
          request((time) => {
            const callback = frames[index];
            if (!callback) throw new Error(`No held frame ${index}`);
            frames[index] = null;
            record("frame-start", { index });
            callback(time);
            record("frame-end", { index });
            resolve();
          }),
        ),
      resume: () => {
        window.requestAnimationFrame = request;
      },
    };
    record("armed");
  });
  return {
    waitForCount: (count) =>
      expect
        .poll(() => page.evaluate(() => window.__focusFrames.frames.length))
        .toBe(count),
    release: (index = 0) =>
      page.evaluate((index) => window.__focusFrames.release(index), index),
    resume: () => page.evaluate(() => window.__focusFrames.resume()),
    mark: (event) =>
      page.evaluate((event) => window.__focusFrames.mark(event), event),
    attach: async (testInfo) =>
      testInfo.attach("focus-ordering", {
        body: JSON.stringify(
          await page.evaluate(() => window.__focusFrames.trace),
          null,
          2,
        ),
        contentType: "application/json",
      }),
  };
}
