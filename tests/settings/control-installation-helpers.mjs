import { expect } from "./fixtures.mjs";

export async function performInstaller(
  page,
  ipc,
  button,
  command,
  { delay = 0, afterNative, record = () => {} } = {},
) {
  const completion = ipc.holdNext(command);
  let timer;
  try {
    await button.click();
    await expect(button).toBeDisabled();
    await completion.arrived;
    await expect(button).toBeDisabled();
    record({ command, phase: "native_settled_ui_pending" });
    await afterNative?.();
    const release = () => {
      record({ command, phase: "reply_released" });
      completion.release();
    };
    if (delay) timer = setTimeout(release, delay);
    else release();
    await page.evaluate(() => window.__settingsIdle());
  } finally {
    clearTimeout(timer);
    completion.release();
  }
}
