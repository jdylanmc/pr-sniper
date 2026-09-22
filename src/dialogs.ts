interface HiddenSibling {
  element: HTMLElement;
  ariaHidden: string | null;
  inert: boolean;
}

interface Entry {
  modal: HTMLDialogElement;
  host: HTMLElement;
  opener: Element | null;
  hidden: HiddenSibling[];
  nativeClose: (() => void) | null;
}

// Native dialogs arrived after our minimum macOS version. Both paths share
// lifecycle, nested-modal isolation and keyboard handling without requiring inert.
export function createDialogs(root: HTMLElement, changed: () => void) {
  const stack: Entry[] = [];
  const top = () => stack[stack.length - 1];
  const controls = (modal: HTMLElement) =>
    [
      ...modal.querySelectorAll<HTMLElement>(
        'button,input,select,textarea,a[href],summary,[tabindex],[contenteditable="true"]',
      ),
    ].filter(
      (element) =>
        element.tabIndex >= 0 &&
        !element.matches(":disabled") &&
        element.getClientRects().length > 0 &&
        !element.closest('[hidden],[aria-hidden="true"]'),
    );
  const focusFirst = (modal: HTMLElement) =>
    (controls(modal)[0] ?? modal).focus({ preventScroll: true });

  function keydown(event: KeyboardEvent) {
    const entry = top();
    if (!entry) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      entry.modal.close();
    } else if (event.key === "Tab") {
      const items = controls(entry.modal);
      const index = items.indexOf(document.activeElement as HTMLElement);
      if (
        index < 0 ||
        (!event.shiftKey && index === items.length - 1) ||
        (event.shiftKey && index === 0)
      ) {
        event.preventDefault();
        (event.shiftKey
          ? (items[items.length - 1] ?? entry.modal)
          : (items[0] ?? entry.modal)
        ).focus();
      }
    }
  }
  function focusin(event: FocusEvent) {
    const entry = top();
    if (
      entry &&
      event.target instanceof Node &&
      !entry.modal.contains(event.target)
    )
      focusFirst(entry.modal);
  }
  function blockOutside(event: Event) {
    const entry = top();
    if (
      entry &&
      event.target instanceof Node &&
      !entry.modal.contains(event.target)
    ) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  }
  function listen(enabled: boolean) {
    if (enabled) {
      document.addEventListener("keydown", keydown, true);
      document.addEventListener("focusin", focusin, true);
      document.addEventListener("click", blockOutside, true);
      document.addEventListener("pointerdown", blockOutside, true);
    } else {
      document.removeEventListener("keydown", keydown, true);
      document.removeEventListener("focusin", focusin, true);
      document.removeEventListener("click", blockOutside, true);
      document.removeEventListener("pointerdown", blockOutside, true);
    }
  }
  function close(entry: Entry) {
    if (!stack.includes(entry)) return;
    while (top() !== entry) close(top());
    changed();
    stack.pop();
    if (entry.nativeClose) entry.nativeClose();
    else {
      entry.modal.open = false;
      entry.modal.removeAttribute("open");
    }
    entry.modal.removeEventListener("input", changed);
    entry.modal.removeEventListener("change", changed);
    for (const { element, ariaHidden, inert } of entry.hidden) {
      if (ariaHidden === null) element.removeAttribute("aria-hidden");
      else element.setAttribute("aria-hidden", ariaHidden);
      if (!inert) element.removeAttribute("inert");
    }
    entry.host.remove();
    if (!stack.length) listen(false);
    if (entry.opener instanceof HTMLElement && entry.opener.isConnected)
      entry.opener.focus({ preventScroll: true });
    else if (top()) focusFirst(top().modal);
  }
  return {
    hasOpen: () => stack.length > 0,
    closeAll: () => {
      while (top()) close(top());
    },
    show(modal: HTMLDialogElement) {
      changed();
      const native =
        typeof modal.showModal === "function" &&
        typeof modal.close === "function";
      const host = native ? modal : document.createElement("div");
      if (!native) {
        host.className = "dialog-fallback";
        host.style.zIndex = String(1000 + stack.length);
        host.append(modal);
      }
      const entry: Entry = {
        modal,
        host,
        opener: document.activeElement,
        hidden: [],
        nativeClose: native ? modal.close.bind(modal) : null,
      };
      modal.setAttribute("role", "dialog");
      modal.setAttribute("aria-modal", "true");
      modal.tabIndex = -1;
      modal.close = () => close(entry);
      modal.addEventListener("cancel", (event) => {
        event.preventDefault();
        close(entry);
      });
      modal.addEventListener("input", changed);
      modal.addEventListener("change", changed);
      root.append(host);
      stack.push(entry);
      listen(true);
      if (native) modal.showModal();
      else {
        modal.open = true;
        modal.setAttribute("open", "");
      }
      focusFirst(modal);
      let branch: HTMLElement = host;
      while (branch.parentElement) {
        const parent = branch.parentElement;
        for (const sibling of parent.children) {
          if (sibling === branch || !(sibling instanceof HTMLElement)) continue;
          entry.hidden.push({
            element: sibling,
            ariaHidden: sibling.getAttribute("aria-hidden"),
            inert: sibling.hasAttribute("inert"),
          });
          sibling.setAttribute("aria-hidden", "true");
          if ("inert" in sibling) sibling.setAttribute("inert", "");
        }
        if (parent === document.body) break;
        branch = parent;
      }
    },
  };
}
