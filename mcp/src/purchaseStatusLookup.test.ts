import { mkdtempSync, rmSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { validateToolArgs } from "./validation.js";
import { _setPurchasesFilePath } from "./purchaseHistory.js";
import {
  listPendingPurchaseAttempts,
  lookupPurchaseStatus,
  trackPurchaseAttempt,
} from "./purchaseStatusLookup.js";

let dir: string;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "mv-attempts-"));
  _setPurchasesFilePath(join(dir, "purchases.json"));
});

afterEach(() => {
  _setPurchasesFilePath(null);
  rmSync(dir, { recursive: true, force: true });
});

const settledResult = JSON.stringify({
  after: { purchased: true, receipt: { paymentId: "pay-1" } },
  txHash: "tx-1",
});

describe("lookupPurchaseStatus", () => {
  it("reports unknown for a key that was never used", () => {
    const out = JSON.parse(lookupPurchaseStatus("default", "key-1"));
    expect(out.state).toBe("unknown");
    expect(out.resourceId).toBeNull();
  });

  it("reports settled with receipt details", async () => {
    await trackPurchaseAttempt("default", "key-1", "res-1", false, async () => settledResult);
    const out = JSON.parse(lookupPurchaseStatus("default", "key-1"));
    expect(out).toMatchObject({
      state: "settled",
      resourceId: "res-1",
      txHash: "tx-1",
      receiptRef: "pay-1",
    });
  });

  it("is scoped to the profile and reveals nothing across profiles", async () => {
    await trackPurchaseAttempt("alice", "key-1", "res-1", false, async () => settledResult);
    const out = JSON.parse(lookupPurchaseStatus("bob", "key-1"));
    expect(out.state).toBe("unknown");
    expect(out.resourceId).toBeNull();
    expect(out.txHash).toBeNull();
  });
});

describe("listPendingPurchaseAttempts", () => {
  it("returns only pending attempts for the requested profile", async () => {
    await expect(
      trackPurchaseAttempt("alice", "pending-key", "res-pending", false, async () => {
        throw Object.assign(new Error("Request timed out"), { name: "TimeoutError" });
      }),
    ).rejects.toThrow();
    await trackPurchaseAttempt("bob", "settled-key", "res-settled", false, async () => settledResult);

    expect(listPendingPurchaseAttempts("alice").map((attempt) => attempt.resourceId)).toEqual([
      "res-pending",
    ]);
    expect(listPendingPurchaseAttempts("bob")).toEqual([]);
  });
});

describe("trackPurchaseAttempt", () => {
  it("runs untouched without a key or on a dry run", async () => {
    await trackPurchaseAttempt("default", undefined, "res-1", false, async () => "ok");
    await trackPurchaseAttempt("default", "key-1", "res-1", true, async () => "ok");
    expect(JSON.parse(lookupPurchaseStatus("default", "key-1")).state).toBe("unknown");
  });

  it("records pending while the buy is in flight", async () => {
    let during = "";
    await trackPurchaseAttempt("default", "key-1", "res-1", false, async () => {
      during = JSON.parse(lookupPurchaseStatus("default", "key-1")).state;
      return settledResult;
    });
    expect(during).toBe("pending");
  });

  it("records failed when a non-payment result comes back", async () => {
    await trackPurchaseAttempt("default", "key-1", "res-1", false, async () => "Insufficient USDC");
    expect(JSON.parse(lookupPurchaseStatus("default", "key-1")).state).toBe("failed");
  });

  it("records failed on a definite error but keeps pending on a timeout", async () => {
    await expect(
      trackPurchaseAttempt("default", "key-1", "res-1", false, async () => {
        throw new Error("Insufficient funds");
      }),
    ).rejects.toThrow();
    expect(JSON.parse(lookupPurchaseStatus("default", "key-1")).state).toBe("failed");

    await expect(
      trackPurchaseAttempt("default", "key-2", "res-1", false, async () => {
        throw Object.assign(new Error("Request timed out"), { name: "TimeoutError" });
      }),
    ).rejects.toThrow();
    expect(JSON.parse(lookupPurchaseStatus("default", "key-2")).state).toBe("pending");
  });

  it("does not pay again for a key that already settled", async () => {
    let calls = 0;
    const buy = async () => {
      calls += 1;
      return settledResult;
    };
    await trackPurchaseAttempt("default", "key-1", "res-1", false, buy);
    const replay = await trackPurchaseAttempt("default", "key-1", "res-1", false, buy);
    expect(calls).toBe(1);
    expect(JSON.parse(replay).state).toBe("settled");
  });
});

describe("argument validation", () => {
  it("requires a well-formed idempotencyKey", () => {
    expect(() => validateToolArgs("mindvault_purchase_status", {})).toThrow();
    expect(() =>
      validateToolArgs("mindvault_purchase_status", { idempotencyKey: "bad key!" }),
    ).toThrow();
    expect(() =>
      validateToolArgs("mindvault_purchase_status", { idempotencyKey: "buy-1:a.b_c" }),
    ).not.toThrow();
  });

  it("accepts the key on mindvault_buy", () => {
    expect(() =>
      validateToolArgs("mindvault_buy", { resourceId: "res-1", idempotencyKey: "buy-1" }),
    ).not.toThrow();
  });
});
