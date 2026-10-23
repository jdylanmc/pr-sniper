import { restoreShared } from "./interned";
import { invoke } from "@tauri-apps/api/core";
import {
  type DoctrineCatalog,
  type Settings,
  doctrineCatalogLabel,
} from "./resources";
import type { MonitoringSnapshot } from "./monitoring";
import { intelligenceDiagnostics } from "./work-presentation";

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

interface DiagnosticSnapshot {
  version: string;
  isolated: boolean;
  doctrine_catalog?: DoctrineCatalog | null;
  settings: Pick<Settings, "doctrine_reset"> | null;
}

// The content-free export: host journal, doctrine catalog and recorded job
// Intelligence only (ADR 0019). Never provider bodies or credentials.
export async function diagnosticsExport(): Promise<string> {
  const [state, entries, jobs] = await Promise.all([
    invoke<DiagnosticSnapshot>("snapshot"),
    invoke<Diagnostic[]>("diagnostics"),
    invoke("monitoring_snapshot").then(restoreShared<MonitoringSnapshot>),
  ]);
  return (
    `PR Sniper ${state.version}${state.isolated ? " \u00b7 Isolated" : ""}\n\n` +
    doctrineDiagnostics(
      state.doctrine_catalog,
      state.settings?.doctrine_reset,
    ) +
    diagnosticLog(entries) +
    `\n\n${intelligenceDiagnostics(jobs)}\n`
  );
}

type CopyOutcome = { ok: true } | { ok: false; error: string };

// WebKit only allows clipboard writes inside the click's activation, so the
// export is handed over as a pending ClipboardItem before any native read.
function writeClipboard(text: Promise<string>): Promise<void> {
  const clipboard = navigator.clipboard;
  if (!clipboard) return Promise.reject(new Error("Clipboard unavailable"));
  if (typeof ClipboardItem === "function" && clipboard.write)
    return clipboard.write([
      new ClipboardItem({
        "text/plain": text.then(
          (value) => new Blob([value], { type: "text/plain" }),
        ),
      }),
    ]);
  return text.then((value) => clipboard.writeText(value));
}

export async function copyDiagnostics(): Promise<CopyOutcome> {
  const text = diagnosticsExport();
  const written = writeClipboard(text);
  try {
    await text;
  } catch (cause) {
    written.catch(() => undefined);
    return {
      ok: false,
      error: typeof cause === "string" ? cause : "Diagnostics unavailable",
    };
  }
  try {
    await written;
    return { ok: true };
  } catch {
    return { ok: false, error: "Copy failed" };
  }
}
