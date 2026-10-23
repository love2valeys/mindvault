import { createHash } from "node:crypto";
import { describe, expect, it, vi } from "vitest";

import {
  assertContractPreflightReady,
  checkContractPreflight,
  EXPECTED_REGISTRY_NAME,
  SUPPORTED_RESOURCE_SCHEMA_VERSIONS,
} from "./contractPreflight.js";

const CONTRACT_ID = `C${"A".repeat(55)}`;
const TESTNET_PASSPHRASE = "Test SDF Network ; September 2015";

function networkId(passphrase: string): Uint8Array {
  return createHash("sha256").update(passphrase, "utf8").digest();
}

const validOptions = {
  contractId: CONTRACT_ID,
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  networkPassphrase: TESTNET_PASSPHRASE,
};

describe("checkContractPreflight", () => {
  it("accepts a deployed registry on the configured network with a supported schema", async () => {
    const result = await checkContractPreflight({
      ...validOptions,
      readRegistryInfo: async () => ({
        name: EXPECTED_REGISTRY_NAME,
        network_id: networkId(TESTNET_PASSPHRASE),
        resource_schema_version: SUPPORTED_RESOURCE_SCHEMA_VERSIONS[0],
      }),
    });

    expect(result.status).toBe("ready");
    expect(result.ready).toBe(true);
    expect(result.issues).toEqual([]);
    expect(result.deployedNetworkId).toBe(result.expectedNetworkId);
  });

  it("reports actionable registry, network, and schema mismatches", async () => {
    const result = await checkContractPreflight({
      ...validOptions,
      network: "mainnet",
      networkPassphrase: "Public Global Stellar Network ; September 2015",
      readRegistryInfo: async () => ({
        name: "different-contract",
        network_id: networkId(TESTNET_PASSPHRASE),
        resource_schema_version: -1,
      }),
    });

    expect(result.status).toBe("blocked");
    expect(result.ready).toBe(false);
    expect(result.issues.map((issue) => issue.field)).toEqual([
      "registry",
      "networkId",
      "schemaVersion",
    ]);
    expect(result.issues.every((issue) => issue.action.length > 0)).toBe(true);
    expect(() => assertContractPreflightReady(result)).toThrow("STELLAR_NETWORK");
  });

  it("rejects a missing or invalid contract ID without making an RPC call", async () => {
    const readRegistryInfo = vi.fn();
    const result = await checkContractPreflight({
      ...validOptions,
      contractId: "not-a-contract-id",
      readRegistryInfo,
    });

    expect(result.status).toBe("blocked");
    expect(result.issues[0]?.field).toBe("contractId");
    expect(readRegistryInfo).not.toHaveBeenCalled();
  });

  it("returns actionable details when the configured registry cannot be read", async () => {
    const result = await checkContractPreflight({
      ...validOptions,
      readRegistryInfo: async () => {
        throw new Error("RPC unavailable");
      },
    });

    expect(result.status).toBe("error");
    expect(result.ready).toBe(false);
    expect(result.issues[0]?.actual).toContain("RPC unavailable");
    expect(result.issues[0]?.action).toContain("SOROBAN_RPC_URL");
  });
});
