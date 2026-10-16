// Draws box-drawing borders around an element on the character grid.

const H = "─".repeat(240);
const V = "│\n".repeat(90);

export function frame(el) {
  if (el.querySelector(":scope > .fb")) return;
  const parts = [
    ["fb fb-t", H],
    ["fb fb-b", H],
    ["fb fb-l", V],
    ["fb fb-r", V],
    ["fb fb-c fb-tl", "┌"],
    ["fb fb-c fb-tr", "┐"],
    ["fb fb-c fb-bl", "└"],
    ["fb fb-c fb-br", "┘"],
  ];
  for (const [className, text] of parts) {
    const strip = document.createElement("span");
    strip.className = className;
    strip.setAttribute("aria-hidden", "true");
    strip.textContent = text;
    el.append(strip);
  }
}
