import type { PolicyOverrides } from "./policy";

export interface Repository {
  id: string;
  provider: "github";
  name: string;
  enabled: boolean;
  installation_id?: string;
  provider_repository_id?: string;
  overrides?: PolicyOverrides;
}
