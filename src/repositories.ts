import type { PolicyOverrides } from "./policy";

export interface Repository {
  id: string;
  provider: "github" | "azure_devops";
  name: string;
  enabled: boolean;
  provider_account_id?: string;
  installation_id?: string;
  provider_repository_id?: string;
  overrides?: PolicyOverrides;
}
