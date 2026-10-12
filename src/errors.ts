// Backend sentences shown as short labels; unknown sentences read "Failed".
const known: [RegExp, string | null][] = [
  [/^Reconcile the original publication/i, "Already reviewed"],
  [/already reviewed this revision/i, "Already reviewed"],
  [/not waiting for manual retry/i, null],
  [/retry budget exhausted/i, "Retries used up"],
  [/running or stopping/i, "Still running"],
  [/no longer available/i, "Unavailable"],
  [
    /credential|keychain|signed[ -]?out|unauthori[sz]ed|bad token/i,
    "Reconnect required",
  ],
  [/network (is )?(unreachable|error)|timed out|dns lookup/i, "Network error"],
];

/** Terse label for a backend error; null means suppress. */
export function terseError(
  message: string,
  keepUnknown = false,
): string | null {
  const text = message.trim();
  const match = known.find(([pattern]) => pattern.test(text));
  if (match) return match[1];
  if (keepUnknown) return text;
  const short = text.split(/\s+/).length <= 3 && !/[\n:;]/.test(text);
  return short ? text : "Failed";
}
