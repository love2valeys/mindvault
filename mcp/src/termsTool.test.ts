/**
 * Focused tests for mindvault_terms tool (#190).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { dispatchTool, _resetProfiles, _setAgentWallet } from "./index.js";
import { setProfiles } from "./runtime.js";
import { publisherTerms } from "./tools/state.js";
import { ToolValidationError, validateToolArgs } from "./validation.js";

const VALID_ADDRESS = "GA6HCMBLTZS5VYYBCATRBRZ3BZJMAFUDKYYF6AH6MVCMGWMRDNSWJPIH";
const TEST_WALLET = {
  publicKey: VALID_ADDRESS,
  secretKey: "SBZXN7PIRZGNMHGA728RGRFZAPPWN9G83LU7575DA2OHGL52HSXU5C58",
};

const state = vi.hoisted(() => ({
  get_terms_hash: vi.fn(),
  set_terms_hash: vi.fn(),
  signAndSend: vi.fn(),
}));

vi.mock("@mindvault/registry-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@mindvault/registry-client")>();
  return {
    ...actual,
    createRegistryClient: () => ({
      get_terms_hash: state.get_terms_hash,
      set_terms_hash: (...args: unknown[]) => {
        state.set_terms_hash(...args);
        return {
          signAndSend: state.signAndSend,
        };
      },
    }),
  };
});

describe("mindvault_terms tool (#190)", () => {
  const prevMock = process.env.MINDVAULT_MOCK;

  beforeEach(() => {
    state.get_terms_hash.mockReset();
    state.set_terms_hash.mockReset();
    state.signAndSend.mockReset();
    _resetProfiles();
    _setAgentWallet(TEST_WALLET);
    setProfiles({ default: { wallet: TEST_WALLET } });
  });

  afterEach(() => {
    _resetProfiles();
    setProfiles({});
    if (prevMock === undefined) {
      delete process.env.MINDVAULT_MOCK;
    } else {
      process.env.MINDVAULT_MOCK = prevMock;
    }
  });

  describe("argument validation", () => {
    it("accepts valid get arguments with creator", () => {
      const result = validateToolArgs("mindvault_terms", {
        operation: "get",
        creator: VALID_ADDRESS,
      });
      expect(result.operation).toBe("get");
      expect(result.creator).toBe(VALID_ADDRESS);
    });

    it("accepts valid set arguments with termsHash", () => {
      const result = validateToolArgs("mindvault_terms", {
        operation: "set",
        termsHash: "sha256:0123456789abcdef",
      });
      expect(result.operation).toBe("set");
      expect(result.termsHash).toBe("sha256:0123456789abcdef");
    });

    it("rejects invalid operation", () => {
      expect(() =>
        validateToolArgs("mindvault_terms", {
          operation: "delete",
        }),
      ).toThrow(ToolValidationError);
    });

    it("rejects missing operation", () => {
      expect(() => validateToolArgs("mindvault_terms", {})).toThrow(ToolValidationError);
    });

    it("rejects invalid creator address", () => {
      expect(() =>
        validateToolArgs("mindvault_terms", {
          operation: "get",
          creator: "not-a-valid-stellar-address",
        }),
      ).toThrow(ToolValidationError);
    });

    it("rejects termsHash longer than 64 characters", () => {
      expect(() =>
        validateToolArgs("mindvault_terms", {
          operation: "set",
          termsHash: "a".repeat(65),
        }),
      ).toThrow(ToolValidationError);
    });
  });

  describe("mock mode behavior", () => {
    it("returns mock terms for default creator", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const result = await publisherTerms("get");
      const parsed = JSON.parse(result);

      expect(parsed.found).toBe(true);
      expect(parsed.termsHash).toBe("mock-terms-hash-1");
      expect(parsed.source).toBe("on-chain (mock)");
    });

    it("returns not found for unknown mock address", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const result = await publisherTerms("get", "mock-unknown");
      const parsed = JSON.parse(result);

      expect(parsed.found).toBe(false);
      expect(parsed.termsHash).toBeNull();
      expect(parsed.source).toBe("on-chain (mock)");
    });

    it("sets terms in mock mode", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const result = await publisherTerms("set", undefined, "mock-terms-hash-2");
      const parsed = JSON.parse(result);

      expect(parsed.status).toBe("success");
      expect(parsed.termsHash).toBe("mock-terms-hash-2");
      expect(parsed.source).toBe("on-chain (mock)");
    });

    it("rejects set with empty termsHash in mock mode", async () => {
      process.env.MINDVAULT_MOCK = "1";
      await expect(publisherTerms("set", undefined, "")).rejects.toThrow(
        "termsHash is required for set.",
      );
    });

    it("rejects set with termsHash exceeding 64 chars in mock mode", async () => {
      process.env.MINDVAULT_MOCK = "1";
      await expect(publisherTerms("set", undefined, "a".repeat(65))).rejects.toThrow(
        "termsHash must be at most 64 characters.",
      );
    });

    it("rejects unknown operation in mock mode", async () => {
      process.env.MINDVAULT_MOCK = "1";
      await expect(publisherTerms("unknown")).rejects.toThrow(
        'Unknown operation "unknown". Use "get" or "set".',
      );
    });
  });

  describe("on-chain client behavior", () => {
    it("fetches terms hash for specified creator", async () => {
      delete process.env.MINDVAULT_MOCK;
      state.get_terms_hash.mockResolvedValue({
        isOk: () => true,
        unwrap: () => "sha256:custom-terms-hash",
      });

      const creator = VALID_ADDRESS;
      const result = await publisherTerms("get", creator);
      const parsed = JSON.parse(result);

      expect(parsed.creator).toBe(creator);
      expect(parsed.termsHash).toBe("sha256:custom-terms-hash");
      expect(parsed.found).toBe(true);
      expect(state.get_terms_hash).toHaveBeenCalledWith({ creator });
    });

    it("handles NotFound error from contract gracefully", async () => {
      delete process.env.MINDVAULT_MOCK;
      state.get_terms_hash.mockResolvedValue({
        isOk: () => false,
        unwrapErr: () => new Error("NotFound"),
      });

      const creator = VALID_ADDRESS;
      const result = await publisherTerms("get", creator);
      const parsed = JSON.parse(result);

      expect(parsed.creator).toBe(creator);
      expect(parsed.termsHash).toBeNull();
      expect(parsed.found).toBe(false);
    });

    it("sets terms hash on-chain and submits transaction", async () => {
      delete process.env.MINDVAULT_MOCK;
      state.signAndSend.mockResolvedValue({ status: "SUCCESS" });

      const termsHash = "sha256:0123456789abcdef";
      const result = await publisherTerms("set", undefined, termsHash);
      const parsed = JSON.parse(result);

      expect(parsed.status).toBe("success");
      expect(parsed.termsHash).toBe(termsHash);
      expect(state.set_terms_hash).toHaveBeenCalledWith(
        {
          creator: VALID_ADDRESS,
          terms_hash: termsHash,
        },
        { simulate: false },
      );
      expect(state.signAndSend).toHaveBeenCalled();
    });

    it("allows get without wallet configured when creator is provided", async () => {
      delete process.env.MINDVAULT_MOCK;
      _resetProfiles();
      _setAgentWallet(null);
      setProfiles({});
      state.get_terms_hash.mockResolvedValue({
        isOk: () => true,
        unwrap: () => "sha256:read-terms",
      });

      const result = await publisherTerms("get", VALID_ADDRESS);
      const parsed = JSON.parse(result);
      expect(parsed.creator).toBe(VALID_ADDRESS);
      expect(parsed.termsHash).toBe("sha256:read-terms");
      expect(parsed.found).toBe(true);
    });

    it("rejects set if termsHash is missing", async () => {
      delete process.env.MINDVAULT_MOCK;
      await expect(publisherTerms("set")).rejects.toThrow("termsHash is required for set.");
    });

    it("rejects set if termsHash exceeds 64 chars", async () => {
      delete process.env.MINDVAULT_MOCK;
      await expect(publisherTerms("set", undefined, "b".repeat(65))).rejects.toThrow(
        "termsHash must be at most 64 characters.",
      );
    });
  });

  describe("MCP tool dispatch", () => {
    it("dispatches mindvault_terms get through dispatchTool", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const res = await dispatchTool("mindvault_terms", {
        operation: "get",
        creator: VALID_ADDRESS,
      });
      const parsed = JSON.parse(res);

      expect(parsed.found).toBe(true);
      expect(parsed.termsHash).toBe("mock-terms-hash-1");
    });

    it("dispatches mindvault_terms set through dispatchTool", async () => {
      process.env.MINDVAULT_MOCK = "1";
      const res = await dispatchTool("mindvault_terms", {
        operation: "set",
        termsHash: "mock-terms-hash-dispatch",
      });
      const parsed = JSON.parse(res);

      expect(parsed.status).toBe("success");
      expect(parsed.termsHash).toBe("mock-terms-hash-dispatch");
    });
  });
});
