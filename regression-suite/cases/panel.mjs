// Semantic actions are implemented by the future guest driver, not by app hooks.
const preconditions = [
  "Exact installed candidate, production frontend, isolated empty offline profile.",
  "No provider accounts, automation, login-item changes or existing editor drafts.",
  "One guest-owned application instance; panel initially hidden.",
];
const cleanup = [
  "Discard the test-only unsaved draft, if created, and restore the hidden panel.",
  "Verify original profile state and terminate only the owned instance.",
];

export const panelCases = [
  {
    id: "panel-destinations",
    feature: "panel-navigation",
    expectation: "All four destinations use one retained native panel.",
    source: "tests/settings/panel.spec.mjs; tests/macos-acceptance.md (#70-2)",
    preconditions,
    cleanup,
    async run({ act, observe, equal }) {
      await act("openPanel");
      const first = await observe("panel");
      equal(first.visible, true, "Panel opens");
      equal(first.windowCount, 1, "Exactly one native panel");
      equal(typeof first.windowId, "string", "Native window identity exists");
      equal(
        Boolean(first.windowId),
        true,
        "Native window identity is nonempty",
      );
      for (const [destination, heading] of [
        ["Queue", "Your queue"],
        ["Running", "Work queue"],
        ["Reviewed", "Reviewed"],
        ["Settings", "Settings"],
      ]) {
        await act("navigate", { destination });
        const panel = await observe("panel");
        equal(panel.visible, true, "Destination is visible");
        equal(panel.windowCount, 1, "No duplicate panel");
        equal(panel.windowId, first.windowId, "Same native panel");
        equal(panel.destination, destination, "Exact selected destination");
        equal(panel.heading, heading, "Exact rendered heading");
      }
    },
  },
  {
    id: "panel-unsaved-draft",
    feature: "panel-drafts",
    expectation: "Navigation and hide/reopen retain unsaved doctrine text.",
    source:
      "tests/settings/panel.spec.mjs; tests/macos-acceptance.md (#70-1/4)",
    preconditions,
    cleanup,
    async run({ act, observe, equal }) {
      const draft = {
        title: "Regression-only draft",
        principles: "Keep this unsaved text after hiding.",
      };
      await act("openPanel");
      const first = await observe("panel");
      equal(first.visible, true, "Panel opens");
      equal(first.windowCount, 1, "Exactly one native panel");
      equal(typeof first.windowId, "string", "Native window identity exists");
      equal(
        Boolean(first.windowId),
        true,
        "Native window identity is nonempty",
      );
      await act("navigate", { destination: "Settings" });
      await act("newDoctrineDraft", draft);
      await act("navigate", { destination: "Running" });
      await act("navigate", { destination: "Settings" });
      for (const dismissal of ["escape", "close-button"]) {
        equal(await observe("doctrineDraft"), draft, "Unsaved text retained");
        await act("dismissPanel", { dismissal });
        equal((await observe("panel")).visible, false, "Dismissal hides panel");
        await act("openPanel");
        const panel = await observe("panel");
        equal(panel.visible, true, "Panel reopens");
        equal(panel.windowCount, 1, "No duplicate panel");
        equal(panel.windowId, first.windowId, "Same native panel after hiding");
        equal(panel.destination, "Settings", "Settings route retained");
        equal(
          await observe("doctrineDraft"),
          draft,
          "Draft survives reopening",
        );
      }
    },
  },
];
