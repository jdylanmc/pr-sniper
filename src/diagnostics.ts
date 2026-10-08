export interface Diagnostic {
  timestamp_secs: number;
  event: string;
  attempt?: unknown;
}

export const diagnosticNotice =
  "Local redacted host and attempt evidence. Attempt history rotates across four 1 MiB files; paths and event summaries are bounded with explicit omissions. No prompts, source, comment bodies or credentials.";

export function diagnosticLog(entries: Diagnostic[]): string {
  return entries.length
    ? entries
        .map(
          (entry) =>
            `${new Date(entry.timestamp_secs * 1000).toISOString()}  ${entry.event}${
              entry.attempt ? `\n${JSON.stringify(entry.attempt, null, 2)}` : ""
            }`,
        )
        .join("\n")
    : "No diagnostic events recorded.";
}
