import { describe, expect, it } from "vitest";
import {
  DEFAULT_MAX_AUTO_PAY_USDC,
  assertAutoPaymentWithinCeiling,
  assertTransactionFeeWithinCeiling,
  checkAutoPaymentCeiling,
  recomputePaymentCeilingAfterPriceChange,
  resolveAutoPaymentCeiling,
} from "./paymentCeiling.js";
import { MIN_USDC_BALANCE, MIN_NATIVE_XLM, assertMinimumBalance } from "./paymentPreflight.js";

const env = { MINDVAULT_MAX_AUTO_PAY_USDC: "5" } as NodeJS.ProcessEnv;

describe("automatic x402 payment ceiling", () => {
  it("uses a documented 10 USDC default", () => {
    expect(DEFAULT_MAX_AUTO_PAY_USDC).toBe("10");
  });

  it("allows a payment below the configured ceiling", () => {
    expect(() => assertAutoPaymentWithinCeiling({ price: "4.99", env })).not.toThrow();
  });

  it("allows a payment exactly at the configured ceiling", () => {
    expect(() => assertAutoPaymentWithinCeiling({ price: "5.00", env })).not.toThrow();
  });

  it("blocks a payment above the ceiling until that call explicitly overrides it", () => {
    expect(() => assertAutoPaymentWithinCeiling({ price: "5.01", env })).toThrow(
      'Purchase requires 5.01 USDC, which exceeds the automatic payment ceiling of 5 USDC. To authorize this purchase, call mindvault_buy with maxAutoPayUsdc: "5.01"',
    );
    expect(() =>
      assertAutoPaymentWithinCeiling({ price: "5.01", maxAutoPayUsdc: "5.01", env }),
    ).not.toThrow();
  });

  it("treats unset and blank MINDVAULT_MAX_AUTO_PAY_USDC identically using the 10 USDC default", () => {
    for (const testEnv of [
      {} as NodeJS.ProcessEnv,
      { MINDVAULT_MAX_AUTO_PAY_USDC: "" } as NodeJS.ProcessEnv,
      { MINDVAULT_MAX_AUTO_PAY_USDC: "   " } as NodeJS.ProcessEnv,
    ]) {
      const resolved = resolveAutoPaymentCeiling(testEnv);
      expect(resolved.isDefault).toBe(true);
      expect(resolved.value).toBe("10");
      expect(checkAutoPaymentCeiling({ price: "9.9999999", env: testEnv })).toEqual({
        allowed: true,
        ceiling: "10",
        isDefaultCeiling: true,
      });
      expect(() =>
        assertAutoPaymentWithinCeiling({ price: "10.0000000", env: testEnv }),
      ).not.toThrow();
    }
  });
});

describe("transaction fee ceiling", () => {
  it("allows fees at or below the configured ceiling", () => {
    expect(() =>
      assertTransactionFeeWithinCeiling({
        feeStroops: "100",
        env: { MINDVAULT_MAX_AUTO_FEE_STROOPS: "100" },
      }),
    ).not.toThrow();
  });

  it("blocks an oversized registry mutation fee", () => {
    expect(() =>
      assertTransactionFeeWithinCeiling({
        feeStroops: "101",
        env: { MINDVAULT_MAX_AUTO_FEE_STROOPS: "100" },
      }),
    ).toThrow(/exceeds/);
  });
});

describe("minimum balance guard", () => {
  it("uses documented defaults", () => {
    expect(MIN_NATIVE_XLM).toBe(0.5);
    expect(MIN_USDC_BALANCE).toBe(0);
  });

  it("returns null when balance is sufficient", () => {
    expect(
      assertMinimumBalance({
        usdcBalance: "10",
        nativeBalance: "2",
        operation: "buy",
      }),
    ).toBeNull();
  });

  it("returns an error when native XLM is too low", () => {
    const result = assertMinimumBalance({
      usdcBalance: "10",
      nativeBalance: "0.1",
      operation: "buy",
    });
    expect(result).toContain("Insufficient native XLM");
    expect(result).toContain("0.1");
  });

  it("returns an error when USDC balance is below required amount", () => {
    const result = assertMinimumBalance({
      usdcBalance: "1",
      nativeBalance: "2",
      operation: "buy",
      requiredUsdc: "5",
    });
    expect(result).toContain("Insufficient USDC");
    expect(result).toContain("1");
    expect(result).toContain("5");
  });

  it("returns null when USDC balance meets the required amount", () => {
    expect(
      assertMinimumBalance({
        usdcBalance: "5",
        nativeBalance: "2",
        operation: "buy",
        requiredUsdc: "5",
      }),
    ).toBeNull();
  });

  it("returns null when no required amount is specified", () => {
    expect(
      assertMinimumBalance({
        usdcBalance: "0.5",
        nativeBalance: "2",
        operation: "buy",
      }),
    ).toBeNull();
  });
});

describe("recomputePaymentCeilingAfterPriceChange", () => {
  const env = { MINDVAULT_MAX_AUTO_PAY_USDC: "10" } as NodeJS.ProcessEnv;

  it("returns priceChanged: false when price is unchanged", () => {
    const res = recomputePaymentCeilingAfterPriceChange({
      resourceId: "res-1",
      approvedPrice: "5.00",
      challengedPrice: "5.0000000",
      env,
    });
    expect(res).toEqual({ priceChanged: false });
  });

  it("returns priceChanged: false when prices are invalid or malformed", () => {
    const res = recomputePaymentCeilingAfterPriceChange({
      resourceId: "res-1",
      approvedPrice: "invalid",
      challengedPrice: "5.00",
      env,
    });
    expect(res).toEqual({ priceChanged: false });
  });

  it("throws ceiling error when changed price exceeds the automatic ceiling", () => {
    expect(() =>
      recomputePaymentCeilingAfterPriceChange({
        resourceId: "res-1",
        approvedPrice: "5.00",
        challengedPrice: "12.00",
        env,
      }),
    ).toThrow(/exceeds the automatic payment ceiling of 10 USDC/);
  });

  it("returns structured confirmation-needed outcome when price changed within ceiling", () => {
    const res = recomputePaymentCeilingAfterPriceChange({
      resourceId: "res-1",
      approvedPrice: "5.00",
      challengedPrice: "7.50",
      env,
    });
    expect(res.priceChanged).toBe(true);
    expect(res.confirmation).toEqual({
      status: "confirmation_needed",
      confirmationRequired: true,
      reason: "price_changed",
      resourceId: "res-1",
      previousPrice: "5.00",
      newPrice: "7.50",
      message:
        "Resource price changed from 5.00 USDC to 7.50 USDC between initial preview and x402 challenge. Confirmation is required before authorizing payment at the new price.",
      maxAutoPayUsdc: null,
    });
  });

  it("honors maxAutoPayUsdc override when price changed above default ceiling", () => {
    const res = recomputePaymentCeilingAfterPriceChange({
      resourceId: "res-1",
      approvedPrice: "5.00",
      challengedPrice: "15.00",
      maxAutoPayUsdc: "20",
      env,
    });
    expect(res.priceChanged).toBe(true);
    expect(res.confirmation?.maxAutoPayUsdc).toBe("20");
    expect(res.confirmation?.newPrice).toBe("15.00");
  });
});
