import { createHash } from "node:crypto";
import { safeErrorMessage } from "./redaction.js";

// Keep this list aligned with RESOURCE_SCHEMA_VERSION in the contract source.
export const SUPPORTED_RESOURCE_SCHEMA_VERSIONS = [6] as const;
export const EXPECTED_REGISTRY_NAME = "mindvault-vault-registry";

export interface RegistryPreflightInfo {
  name: string;
  network_id: Uint8Array;
  resource_schema_version: number;
}

export interface ContractPreflightIssue {
  field: "contractId" | "registry" | "networkId" | "schemaVersion";
  expected: string;
  actual: string;
  action: string;
}

export interface ContractPreflightResult {
  status: "ready" | "blocked" | "error" | "skipped";
  ready: boolean;
  message: string;
  contractId: string;
  network: string;
  rpcUrl: string;
  expectedNetworkId: string;
  deployedNetworkId: string | null;
  registryName: string | null;
  deployedSchemaVersion: number | null;
  supportedSchemaVersions: number[];
  issues: ContractPreflightIssue[];
}

export interface ContractPreflightOptions {
  contractId: string;
  network: string;
  rpcUrl: string;
  networkPassphrase: string;
  readRegistryInfo: () => Promise<RegistryPreflightInfo>;
}

function networkIdForPassphrase(passphrase: string): Buffer {
  return createHash("sha256").update(passphrase, "utf8").digest();
}

function isContractId(value: string): boolean {
  return /^C[A-Z2-7]{55}$/.test(value);
}

function isRegistryPreflightInfo(value: RegistryPreflightInfo): boolean {
  return (
    typeof value?.name === "string" &&
    value.network_id instanceof Uint8Array &&
    value.network_id.length === 32 &&
    Number.isInteger(value.resource_schema_version)
  );
}

export async function checkContractPreflight(
  options: ContractPreflightOptions,
): Promise<ContractPreflightResult> {
  const expectedNetworkId = networkIdForPassphrase(options.networkPassphrase).toString("hex");
  const issues: ContractPreflightIssue[] = [];
  const result: ContractPreflightResult = {
    status: "ready",
    ready: true,
    message: "Registry contract ID, network, and Resource schema are compatible.",
    contractId: options.contractId,
    network: options.network,
    rpcUrl: options.rpcUrl,
    expectedNetworkId,
    deployedNetworkId: null,
    registryName: null,
    deployedSchemaVersion: null,
    supportedSchemaVersions: [...SUPPORTED_RESOURCE_SCHEMA_VERSIONS],
    issues,
  };

  if (!isContractId(options.contractId)) {
    issues.push({
      field: "contractId",
      expected: "A deployed Stellar contract ID (C...)",
      actual: options.contractId || "(not configured)",
      action: "Set VAULT_REGISTRY_CONTRACT_ID to the intended deployed registry contract ID.",
    });
    result.status = "blocked";
    result.ready = false;
    result.message = "Registry preflight blocked: configured contract ID is missing or invalid.";
    return result;
  }

  let info: RegistryPreflightInfo;
  try {
    info = await options.readRegistryInfo();
  } catch (error) {
    issues.push({
      field: "registry",
      expected: EXPECTED_REGISTRY_NAME,
      actual: safeErrorMessage(error),
      action:
        "Check VAULT_REGISTRY_CONTRACT_ID and SOROBAN_RPC_URL, and confirm the registry is deployed and reachable on the configured network.",
    });
    result.status = "error";
    result.ready = false;
    result.message = "Registry preflight failed: the configured registry could not be read.";
    return result;
  }

  if (!isRegistryPreflightInfo(info)) {
    issues.push({
      field: "registry",
      expected: EXPECTED_REGISTRY_NAME,
      actual: "malformed registry_info response",
      action:
        "Confirm VAULT_REGISTRY_CONTRACT_ID points to the current MindVault registry deployment and update it if needed.",
    });
    result.status = "error";
    result.ready = false;
    result.message = "Registry preflight failed: the contract returned invalid discovery metadata.";
    return result;
  }

  const deployedNetworkId = Buffer.from(info.network_id).toString("hex");
  result.deployedNetworkId = deployedNetworkId;
  result.registryName = info.name;
  result.deployedSchemaVersion = info.resource_schema_version;

  if (info.name !== EXPECTED_REGISTRY_NAME) {
    issues.push({
      field: "registry",
      expected: EXPECTED_REGISTRY_NAME,
      actual: info.name,
      action: "Point VAULT_REGISTRY_CONTRACT_ID at a MindVault vault-registry deployment.",
    });
  }

  if (deployedNetworkId !== expectedNetworkId) {
    issues.push({
      field: "networkId",
      expected: `${options.network} (${expectedNetworkId})`,
      actual: deployedNetworkId,
      action:
        "Use a registry deployed on the configured Stellar network, or align STELLAR_NETWORK and SOROBAN_RPC_URL with the registry.",
    });
  }

  if (
    !SUPPORTED_RESOURCE_SCHEMA_VERSIONS.some((version) => version === info.resource_schema_version)
  ) {
    issues.push({
      field: "schemaVersion",
      expected: SUPPORTED_RESOURCE_SCHEMA_VERSIONS.join(", "),
      actual: String(info.resource_schema_version),
      action:
        "Deploy a registry with a supported Resource schema, or update the MCP client to support this schema before using on-chain tools.",
    });
  }

  result.ready = issues.length === 0;
  result.status = result.ready ? "ready" : "blocked";
  if (!result.ready) {
    result.message = `Registry preflight blocked: ${issues.map((issue) => `${issue.field} mismatch`).join(", ")}.`;
  }
  return result;
}

export function skippedContractPreflight(
  contractId: string,
  network: string,
  rpcUrl: string,
  networkPassphrase: string,
): ContractPreflightResult {
  return {
    status: "skipped",
    ready: false,
    message: "Live registry compatibility was not checked in mock mode.",
    contractId,
    network,
    rpcUrl,
    expectedNetworkId: networkIdForPassphrase(networkPassphrase).toString("hex"),
    deployedNetworkId: null,
    registryName: null,
    deployedSchemaVersion: null,
    supportedSchemaVersions: [...SUPPORTED_RESOURCE_SCHEMA_VERSIONS],
    issues: [
      {
        field: "registry",
        expected: EXPECTED_REGISTRY_NAME,
        actual: "mock mode",
        action:
          "Disable mock mode and rerun mindvault_contract_preflight against the deployed registry.",
      },
    ],
  };
}

export function assertContractPreflightReady(result: ContractPreflightResult): void {
  if (result.ready) return;
  const actions = result.issues.map((issue) => issue.action);
  throw new Error(
    `${result.message} ${actions.length > 0 ? `Next: ${[...new Set(actions)].join(" ")}` : ""}`,
  );
}
