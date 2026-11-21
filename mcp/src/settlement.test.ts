import { describe, expect, it } from "vitest";
import {
  isTransactionSettled,
  normalizeSettlementWaitPolicy,
  normalizeSettlementTimeoutMs,
  normalizeSettlementIntervalMs,
  estimateSettlementSteps,
  pollSettlement,
  buildSettlementSnapshot,
} from "./settlement.js";

describe("mindvault_buy settlement wait confirmation (#174)", () => {
  it("detects terminal transaction statuses", () => {
    expect(isTransactionSettled("SUCCESS")).toBe(true);
    expect(isTransactionSettled("FAILED")).toBe(true);
    expect(isTransactionSettled("PENDING")).toBe(false);
    expect(isTransactionSettled(null)).toBe(false);
  });

  it("normalizes settlement wait policies", () => {
    expect(normalizeSettlementWaitPolicy(true, undefined)).toBe(true);
    expect(normalizeSettlementWaitPolicy(false, undefined)).toBe(false);
    expect(normalizeSettlementWaitPolicy(undefined, "wait_for_final_result")).toBe(true);
    expect(normalizeSettlementWaitPolicy(undefined, "return_after_submit")).toBe(false);
  });

  it("bounds settlement timeout and intervals", () => {
    expect(normalizeSettlementTimeoutMs(10000)).toBe(10000);
    expect(normalizeSettlementIntervalMs(1000)).toBe(1000);
    expect(normalizeSettlementIntervalMs(50)).toBe(200);
  });

  it("polls settlement until transaction reaches SUCCESS", async () => {
    let callCount = 0;
    const result = await pollSettlement({
      txHash: "0123456789abcdef",
      wait: true,
      timeoutMs: 5000,
      intervalMs: 200,
      fetchTransaction: async () => {
        callCount++;
        return {
          hash: "0123456789abcdef",
          found: true,
          status: callCount >= 2 ? "SUCCESS" : "PENDING",
        };
      },
      sleep: async () => {},
    });

    expect(result.tx.status).toBe("SUCCESS");
    expect(result.attempts).toBe(2);
    expect(result.timedOut).toBe(false);

    const snapshot = buildSettlementSnapshot({
      txHash: "0123456789abcdef",
      wait: true,
      result,
    });
    expect(snapshot.settled).toBe(true);
    expect(snapshot.success).toBe(true);
  });
});
