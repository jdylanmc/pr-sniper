import type { PolicyOverrides } from "./policy";

export interface Repository {
  id: string;
  provider: "github";
  name: string;
  enabled: boolean;
  overrides?: PolicyOverrides;
}
