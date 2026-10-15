import { invoke } from "@tauri-apps/api/core";
import type { Agent, Doctrine, Policy, Voice, WatchedIdentity } from "./policy";
import type { Repository } from "./repositories";
import type { Theme } from "./theme";

/** Saved Preferences bounding Rubber Duck use per primary attempt. */
export interface DuckLimits {
  max_calls: number;
  call_timeout_seconds: number;
  primary_window_minutes: number;
}
export const DUCK_LIMITS: DuckLimits = {
  max_calls: 8,
  call_timeout_seconds: 180,
  primary_window_minutes: 15,
};
/** Inclusive ranges; mirror `DuckLimits` in storage.rs. */
export const DUCK_RANGES: Record<keyof DuckLimits, [number, number]> = {
  max_calls: [1, 32],
  call_timeout_seconds: [30, 600],
  primary_window_minutes: [5, 120],
};

export interface Settings {
  capability_notice?: string;
  launch_at_login: boolean;
  defaults: Policy;
  capacity: number;
  /** IANA zone for every repository schedule. */
  time_zone: string;
  rubber_duck?: DuckLimits;
  repositories?: Repository[];
  root_folder?: string;
  doctrines?: Doctrine[];
  voices?: Voice[];
  agents?: Agent[];
  presets?: { id: string; name: string; body: string }[];
  default_review_preset?: string;
  doctrine_catalog_version?: number;
  voice_catalog_version?: number;
  admission_migration_version?: number;
  doctrine_reset?: { previous_count: number; removed_references: number };
  theme?: Theme;
}

export interface DoctrineCatalog {
  source: string;
  source_revision: string;
  effective_revision: string;
  count: number;
}

export const doctrineCatalogLabel = (catalog?: DoctrineCatalog | null) =>
  catalog
    ? `${catalog.count} doctrines / ${catalog.source} / source ${catalog.source_revision.slice(0, 12)} / effective ${catalog.effective_revision.slice(0, 12)}`
    : "Not captured";

export interface GlobalPreferences {
  defaults: Policy;
  capacity: number;
  time_zone: string;
  root_folder: string | null;
  presets: NonNullable<Settings["presets"]>;
  default_review_preset: string | null;
  rubber_duck: DuckLimits;
}

export type ResourceEdit =
  | { kind: "agent"; id: string; expected: Agent | null; value: Agent | null }
  | {
      kind: "doctrine";
      id: string;
      expected: Doctrine | null;
      value: Doctrine | null;
    }
  | {
      kind: "voice";
      id: string;
      expected: Voice | null;
      value: Voice | null;
    }
  | {
      kind: "repository";
      id: string;
      expected: Repository | null;
      value: Repository | null;
    }
  | {
      kind: "preferences";
      expected: GlobalPreferences;
      value: GlobalPreferences;
    };

export interface AssignmentAuthority {
  primary: boolean;
  comment: boolean;
  reply: boolean;
  approve: boolean;
  merge: boolean;
}

export interface ResourceReadiness {
  configuration_ready: boolean;
  issues: string[];
  repositories: {
    repository_id: string;
    primary_assignment_id: string | null;
    assignments: [string, AssignmentAuthority][];
    issues: string[];
  }[];
}

export interface SavedResources {
  settings: Settings;
  readiness: ResourceReadiness;
}

export interface MonitoringActivationStatus {
  repository_id: string;
  active: boolean;
  reason: string | null;
  mode: "all_open_and_future" | "new_only" | "selected_existing" | null;
  selected_existing: number;
  creation_watermark: number | null;
}

export interface SetupReview {
  resources: SavedResources;
  repository_accounts: Record<string, { login: string; connected: boolean }>;
  ai_accounts: Record<string, { login: string; connected: boolean }>;
  scopes: MonitoringActivationStatus[];
  watched_authors: Record<string, WatchedIdentity[]>;
  configured_inactive: boolean;
  paused: boolean;
  confirmation: string;
}

export const savedResources = () => invoke<SavedResources>("saved_resources");
export function sameResource(left: unknown, right: unknown): boolean {
  const ordered = (value: unknown): unknown => {
    if (Array.isArray(value)) return value.map(ordered);
    if (value !== null && typeof value === "object")
      return Object.fromEntries(
        Object.entries(value)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([key, value]) => [key, ordered(value)]),
      );
    return value;
  };
  return JSON.stringify(ordered(left)) === JSON.stringify(ordered(right));
}
export const validateResource = (edit: ResourceEdit) =>
  invoke<ResourceReadiness>("validate_resource", { edit });
export const saveResource = (edit: ResourceEdit, accountGeneration?: number) =>
  invoke<{
    settings: Settings;
    doctrine_catalog: DoctrineCatalog;
    warning: string | null;
  }>("save_resource", {
    edit,
    accountGeneration,
  });
export const globalPreferences = (settings: Settings): GlobalPreferences => ({
  defaults: settings.defaults,
  capacity: settings.capacity,
  time_zone: settings.time_zone,
  root_folder: settings.root_folder ?? null,
  presets: settings.presets ?? [],
  default_review_preset: settings.default_review_preset ?? null,
  rubber_duck: settings.rubber_duck ?? { ...DUCK_LIMITS },
});

export interface ReviewSelection {
  agent: Agent;
  policy: Policy;
  doctrine: string | null;
  preset: string | null;
  configuration?: {
    repository: Repository;
    authority: AssignmentAuthority;
    doctrines: Doctrine[];
    doctrine_catalog?: DoctrineCatalog;
    voice?: Voice;
    rubber_duck?: { agent: Agent; doctrines: Doctrine[] };
  };
}

/** Update only the committed resource (library entries by id; Agents are
 * never rewritten by a rename). Other draft
 * values and their compare-and-save baselines deliberately stay untouched. */
export function acceptResource(
  settings: Settings,
  committed: Settings,
  edit: ResourceEdit,
): void {
  if (edit.kind === "preferences") {
    settings.defaults = committed.defaults;
    settings.capacity = committed.capacity;
    settings.time_zone = committed.time_zone;
    settings.root_folder = committed.root_folder;
    settings.presets = committed.presets;
    settings.default_review_preset = committed.default_review_preset;
    settings.rubber_duck = committed.rubber_duck;
  } else if (edit.kind === "agent") {
    const value = committed.agents?.find((a) => a.id === edit.id);
    const index = (settings.agents ??= []).findIndex((a) => a.id === edit.id);
    if (index >= 0) settings.agents.splice(index, 1, ...(value ? [value] : []));
    else if (value) settings.agents.push(value);
  } else if (edit.kind === "repository") {
    const value = committed.repositories?.find((r) => r.id === edit.id);
    const index = (settings.repositories ??= []).findIndex(
      (r) => r.id === edit.id,
    );
    if (index >= 0 && value) {
      const current = settings.repositories[index];
      Object.keys(current).forEach((key) =>
        Reflect.deleteProperty(current, key),
      );
      Object.assign(current, value);
    } else if (index >= 0) settings.repositories.splice(index, 1);
    else if (value) settings.repositories.push(value);
  } else if (edit.kind === "voice") {
    // By id: a rename never touches Agents, which reference the id.
    const value = committed.voices?.find((v) => v.id === edit.id);
    const index = (settings.voices ??= []).findIndex((v) => v.id === edit.id);
    if (index >= 0) settings.voices.splice(index, 1, ...(value ? [value] : []));
    else if (value) settings.voices.push(value);
  } else {
    const value = committed.doctrines?.find((d) => d.id === edit.id);
    const index = (settings.doctrines ??= []).findIndex(
      (d) => d.id === edit.id,
    );
    if (index >= 0)
      settings.doctrines.splice(index, 1, ...(value ? [value] : []));
    else if (value) settings.doctrines.push(value);
  }
}
