// Renders the CHANGELOG's inline Markdown (**bold**, `code`) as DOM nodes;
// text is never parsed as HTML.
export function inlineMarkdown(target: HTMLElement, text: string) {
  const pattern = /\*\*([^*]+)\*\*|`([^`]+)`/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    target.append(text.slice(last, match.index));
    const node = document.createElement(match[1] ? "strong" : "code");
    node.textContent = match[1] ?? match[2];
    target.append(node);
    last = match.index! + match[0].length;
  }
  target.append(text.slice(last));
}
