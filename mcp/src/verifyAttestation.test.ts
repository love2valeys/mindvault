/**
 * Focused tests for mindvault_verify_attestation tool (#189).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { dispatchTool, verifyAttestation } from "./index.js";
import { ToolValidationError, validateToolArgs } from "./validation.js";

const getAttestationHashMock = vi.fn();

vi.mock("@mindvault/registry-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@mindvault/registry-client")>();
  return {
    ...actual,
    getAttestationHash: (...args: unknown[]) => getAttestationHashMock(...args),
  };
});

describe("mindvault_verify_attestation tool (#189)", () => {
  const prevMock = process.env.MINDVAULT_MOCK;

  beforeEach(() => {
    getAttestationHashMock.mockReset();
  });

  afterEach(() => {
    if (prevMock === undefined) {
      delete process.env.MINDVAULT_MOCK;
    } else {
      process.env.MINDVAULT_MOCK = prevMock;
    }
  });

  describe("direct verifyAttestation helper", () => {
    it("reports matching hash on-chain", async () => {
      delete process.env.MINDVAULT_MOCK;
      getAttestationHashMock.mockResolvedValue("abc123hash");

      const result = await verifyAttestation("cm7x8y9z", "abc123hash");
      const parsed = JSON.parse(result);

      expect(parsed.matches).toBe(true);
      expect(parsed.verified).toBe(true);
      expect(parsed.resourceId).toBe("cm7x8y9z");
      expect(parsed.expectedAttestationHash).toBe("abc123hash");
      expect(parsed.registeredAttestationHash).toBe("abc123hash");
      expect(parsed.summary).toContain("matches");
    });

    it("reports mismatched hash on-chain without throwing transport error", async () => {
      delete process.env.MINDVAULT_MOCK;
      getAttestationHashMock.mockResolvedValue("registered-hash");

      const result = await verifyAttestation("cm7x8y9z", "different-hash");
      const parsed = JSON.parse(result);

      expect(parsed.matches).toBe(false);
      expect(parsed.verified).toBe(false);
      expect(parsed.resourceId).toBe("cm7x8y9z");
      expect(parsed.expectedAttestationHash).toBe("different-hash");
      expect(parsed.registeredAttestationHash).toBe("registered-hash");
      expect(parsed.summary).toContain("does not match");
    });

    it("reports when resource has no attestation hash registered", async () => {
      delete process.env.MINDVAULT_MOCK;
      getAttestationHashMock.mockResolvedValue(null);

      const result = await verifyAttestation("cm7x8y9z", "somehash");
      const parsed = JSON.parse(result);

      expect(parsed.matches).toBe(false);
      expect(parsed.verified).toBe(false);
      expect(parsed.registeredAttestationHash).toBeNull();
      expect(parsed.summary).toContain("No attestation hash is registered");
    });

    it("trims whitespace from the expected hash", async () => {
      delete process.env.MINDVAULT_MOCK;
      getAttestationHashMock.mockResolvedValue("hash123");

      const result = await verifyAttestation("cm7x8y9z", "  hash123  ");
      const parsed = JSON.parse(result);

      expect(parsed.matches).toBe(true);
      expect(parsed.expectedAttestationHash).toBe("hash123");
    });

    it("rejects missing resourceId", async () => {
      await expect(verifyAttestation("", "hash123")).rejects.toThrow("resourceId is required");
    });

    it("rejects missing attestationHash", async () => {
      await expect(verifyAttestation("cm7x8y9z", "")).rejects.toThrow(
        "attestationHash is required",
      );
    });

    it("rejects attestationHash exceeding 64 characters", async () => {
      await expect(verifyAttestation("cm7x8y9z", "a".repeat(65))).rejects.toThrow(
        "at most 64 characters",
      );
    });

    it("maps missing get_attestation_hash contract binding to actionable error", async () => {
      delete process.env.MINDVAULT_MOCK;
      getAttestationHashMock.mockRejectedValue(
        new Error("The deployed vault-registry contract does not expose get_attestation_hash."),
      );

      const err = await verifyAttestation("cm7x8y9z", "hash123").catch((e) => e);
      expect(err.message).toContain("Category: contract");
      expect(err.message).toContain("mindvault_check_bindings");
    });
  });

  describe("mock mode behavior", () => {
    it("reports matching mock attestation for mock1", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const result = await verifyAttestation("mock1", "mock-attestation-1");
      const parsed = JSON.parse(result);

      expect(parsed.matches).toBe(true);
      expect(parsed.verified).toBe(true);
      expect(parsed.registeredAttestationHash).toBe("mock-attestation-1");
    });

    it("reports mismatch in mock mode", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const result = await verifyAttestation("mock1", "wrong-hash");
      const parsed = JSON.parse(result);

      expect(parsed.matches).toBe(false);
      expect(parsed.verified).toBe(false);
    });
  });

  describe("MCP tool dispatch", () => {
    it("dispatches mindvault_verify_attestation through dispatchTool", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const res = await dispatchTool("mindvault_verify_attestation", {
        resourceId: "mock1",
        attestationHash: "mock-attestation-1",
      });
      const parsed = JSON.parse(res);

      expect(parsed.verified).toBe(true);
      expect(parsed.matches).toBe(true);
    });
  });

  describe("argument validation", () => {
    it("accepts valid arguments", () => {
      const result = validateToolArgs("mindvault_verify_attestation", {
        resourceId: "cm7x8y9z",
        attestationHash: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
      });
      expect(result.resourceId).toBe("cm7x8y9z");
      expect(result.attestationHash).toBe(
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
      );
    });

    it("rejects non-alphanumeric resourceId pattern", () => {
      expect(() =>
        validateToolArgs("mindvault_verify_attestation", {
          resourceId: "res-001!",
          attestationHash: "hash123",
        }),
      ).toThrow(ToolValidationError);
    });

    it("rejects attestationHash longer than 64 chars", () => {
      expect(() =>
        validateToolArgs("mindvault_verify_attestation", {
          resourceId: "cm7x8y9z",
          attestationHash: "a".repeat(65),
        }),
      ).toThrow(ToolValidationError);
    });
  });
});
