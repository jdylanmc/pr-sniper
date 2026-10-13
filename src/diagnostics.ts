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

interface Counted {
  count?: number;
}

// Informational count of changed files read; recorded, never a gate.
function filesRead(attempt: unknown): string {
  const coverage = (
    attempt as {
      event?: { coverage?: { changed?: Counted; covered?: Counted } };
    }
  )?.event?.coverage;
  const { changed, covered } = coverage ?? {};
  return typeof changed?.count === "number" &&
    typeof covered?.count === "number"
    ? `\n${covered.count} of ${changed.count} files read`
    : "";
}

export function diagnosticLog(entries: Diagnostic[]): string {
  return entries.length
    ? entries
        .map(
          (entry) =>
            `${new Date(entry.timestamp_secs * 1000).toISOString()}  ${entry.event}${filesRead(entry.attempt)}${
              entry.attempt ? `\n${JSON.stringify(entry.attempt, null, 2)}` : ""
            }`,
        )
        .join("\n")
    : "No events";
}
