import { invoke } from "@tauri-apps/api/core";
import type { Agent, Doctrine, Policy } from "./policy";
import type { Repository } from "./repositories";

export interface Settings {
  launch_at_login: boolean;
  defaults: Policy;
  capacity: number;
  repositories?: Repository[];
  root_folder?: string;
  doctrines?: Doctrine[];
  agents?: Agent[];
  presets?: { id: string; name: string; body: string }[];
  default_review_preset?: string;
}

export interface GlobalPreferences {
  defaults: Policy;
  capacity: number;
  root_folder: string | null;
  presets: NonNullable<Settings["presets"]>;
  default_review_preset: string | null;
}

export type ResourceEdit =
  | { kind: "agent"; id: string; expected: Agent | null; value: Agent | null }
  | {
      kind: "doctrine";
      title: string;
      expected: Doctrine | null;
      value: Doctrine | null;
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
export const saveResource = (edit: ResourceEdit) =>
  invoke<{ settings: Settings; warning: string | null }>("save_resource", {
    edit,
  });
export const globalPreferences = (settings: Settings): GlobalPreferences => ({
  defaults: settings.defaults,
  capacity: settings.capacity,
  root_folder: settings.root_folder ?? null,
  presets: settings.presets ?? [],
  default_review_preset: settings.default_review_preset ?? null,
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
  };
}

/** Update only the committed resource and its rename references. Other draft
 * values and their compare-and-save baselines deliberately stay untouched. */
export function acceptResource(
  settings: Settings,
  committed: Settings,
  edit: ResourceEdit,
): void {
  if (edit.kind === "preferences") {
    settings.defaults = committed.defaults;
    settings.capacity = committed.capacity;
    settings.root_folder = committed.root_folder;
    settings.presets = committed.presets;
    settings.default_review_preset = committed.default_review_preset;
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
  } else {
    const key = (title: string) => title.trim().toLowerCase();
    const value = committed.doctrines?.find(
      (d) => key(d.title) === key(edit.value?.title ?? edit.title),
    );
    const index = (settings.doctrines ??= []).findIndex(
      (d) => key(d.title) === key(edit.title),
    );
    if (index >= 0)
      settings.doctrines.splice(index, 1, ...(value ? [value] : []));
    else if (value) settings.doctrines.push(value);
    if (edit.value) {
      const title = edit.value.title;
      for (const agent of settings.agents ?? []) {
        if (agent.doctrine && key(agent.doctrine) === key(edit.title))
          agent.doctrine = title;
        if (agent.doctrines)
          agent.doctrines = agent.doctrines.map((d) =>
            key(d) === key(edit.title) ? title : d,
          );
      }
    }
  }
}
