import {
  type DoctrineCatalog,
  type Settings,
  doctrineCatalogLabel,
} from "./resources";

export interface Diagnostic {
  timestamp_secs: number;
  event: string;
  attempt?: unknown;
}

export function doctrineDiagnostics(
  catalog?: DoctrineCatalog | null,
  reset?: Settings["doctrine_reset"],
): string {
  return (
    `${doctrineCatalogLabel(catalog)}\n${JSON.stringify(catalog ?? null, null, 2)}\n\n` +
    (reset
      ? `Doctrine reset: ${reset.previous_count} replaced; ${reset.removed_references} references removed.\n\n`
      : "")
  );
}

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
    : "No events";
}
