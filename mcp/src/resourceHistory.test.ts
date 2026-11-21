import { describe, expect, it } from "vitest";
import {
  recordResourceHistory,
  queryResourcePriceAndMetadataHistory,
  readResourceHistory,
} from "./resourceHistory.js";

describe("resource price and metadata change log (#175)", () => {
  it("records and queries price and metadata events", () => {
    const resourceId = "test-res-" + Date.now();
    recordResourceHistory({
      resourceId,
      kind: "price",
      amount: "10.00",
      txHash: "tx1",
    });
    recordResourceHistory({
      resourceId,
      kind: "metadata",
      value: "updated-title",
      txHash: "tx2",
    });

    const result = queryResourcePriceAndMetadataHistory(resourceId);
    expect(result.resourceId).toBe(resourceId);
    expect(result.count).toBeGreaterThanOrEqual(2);
    expect(result.changes.some((c) => c.kind === "price")).toBe(true);
    expect(result.changes.some((c) => c.kind === "metadata")).toBe(true);
  });
});
