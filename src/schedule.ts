import { invoke } from "@tauri-apps/api/core";

/** Mirrors `schedule::Cadence` in src-tauri/src/schedule.rs. */
export type Cadence =
  | { kind: "every"; minutes: number }
  | { kind: "hourly"; minute: number }
  | { kind: "daily"; times: string[] };

/** Mirrors `schedule::Builder`. The backend encodes and decodes it. */
export interface ScheduleBuilder {
  cadence: Cadence;
  /** 0 = Sunday. All seven = every day. */
  days: number[];
  /** Hours `start..end`, end exclusive. Not with `daily`. */
  hours?: { start: number; end: number };
}

export type ScheduleRequest =
  | { kind: "builder"; builder: ScheduleBuilder }
  | { kind: "cron"; cron: string };

export interface SchedulePreview {
  cron: string;
  /** Null: only Advanced represents this cron. */
  builder: ScheduleBuilder | null;
  next_runs: number[];
}

export const DEFAULT_CRON = "*/30 * * * *";
export const EVERY_MINUTES = [5, 10, 15, 30, 60, 120, 240] as const;
export const ALL_DAYS = [0, 1, 2, 3, 4, 5, 6];
/** Monday-first display order. */
export const WEEK = [1, 2, 3, 4, 5, 6, 0];
export const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

export const previewSchedule = (request: ScheduleRequest) =>
  invoke<SchedulePreview>("preview_schedule", { request });

export const describeSchedules = (crons: string[]) =>
  invoke<(ScheduleBuilder | null)[]>("describe_schedules", { crons });

export const everyLabel = (minutes: number) =>
  minutes < 60 ? `${minutes} min` : `${minutes / 60} h`;

const pad = (value: number) => String(value).padStart(2, "0");

/** Days in Monday-first runs: `Mon–Fri`, `Mon, Wed`, or nothing for every day. */
function dayLabel(days: number[]): string {
  const selected = WEEK.filter((day) => days.includes(day));
  if (selected.length === 7) return "";
  const runs: number[][] = [];
  for (const day of selected) {
    const run = runs[runs.length - 1];
    if (run && WEEK.indexOf(run[run.length - 1]) + 1 === WEEK.indexOf(day))
      run.push(day);
    else runs.push([day]);
  }
  return runs
    .map((run) =>
      run.length > 2
        ? `${DAY_NAMES[run[0]]}–${DAY_NAMES[run[run.length - 1]]}`
        : run.map((day) => DAY_NAMES[day]).join(", "),
    )
    .join(", ");
}

function cadenceLabel(cadence: Cadence): string {
  if (cadence.kind === "every") return `every ${everyLabel(cadence.minutes)}`;
  if (cadence.kind === "hourly") return `hourly at :${pad(cadence.minute)}`;
  return `daily ${cadence.times.join(", ")}`;
}

/** Compact value, e.g. `Every 30 min` or `Mon–Fri 9–18 · every 15 min`.
 * A cron outside the builder shows as itself. */
export function scheduleSummary(
  builder: ScheduleBuilder | null | undefined,
  cron: string,
): string {
  if (!builder) return cron;
  const limits = [
    dayLabel(builder.days),
    builder.hours ? `${builder.hours.start}–${builder.hours.end}` : "",
  ].filter(Boolean);
  const cadence = cadenceLabel(builder.cadence);
  return limits.length
    ? `${limits.join(" ")} · ${cadence}`
    : cadence[0].toUpperCase() + cadence.slice(1);
}

/** One preview run in the saved time zone, e.g. `Fri, Jan 15, 08:30`.
 * Assembled from parts: engines join the default pattern differently. */
export function formatRun(seconds: number, timeZone: string): string {
  try {
    const parts = new Intl.DateTimeFormat("en-US", {
      timeZone,
      weekday: "short",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }).formatToParts(new Date(seconds * 1000));
    const part = (type: Intl.DateTimeFormatPartTypes) =>
      parts.find((entry) => entry.type === type)?.value ?? "";
    return `${part("weekday")}, ${part("month")} ${part("day")}, ${part("hour")}:${part("minute")}`;
  } catch {
    return new Date(seconds * 1000).toISOString();
  }
}
