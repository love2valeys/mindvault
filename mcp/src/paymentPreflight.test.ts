import { describe, expect, it } from "vitest";
import {
  DEFAULT_MAX_AUTO_PAY_USDC,
  assertAutoPaymentWithinCeiling,
  checkAutoPaymentCeiling,
} from "./paymentCeiling.js";
import { assertMinimumBalance, buildPaymentPreflight } from "./paymentPreflight.js";

const WALLET = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

describe("buildPaymentPreflight — ceiling alignment with payment path (#223)", () => {
  it("accepts casual and near-limit buy amounts when MINDVAULT_MAX_AUTO_PAY_USDC is unset", () => {
    const unsetEnv = {} as NodeJS.ProcessEnv;

    for (const amount of ["0.10", "5.00", "9.99", "9.9999999", "10", "10.0000000"]) {
      const preflight = buildPaymentPreflight({
        operation: "buy",
        walletPublicKey: WALLET,
        usdcBalance: "25.00",
        nativeBalance: "5",
        expectedAmount: amount,
        network: "stellar:testnet",
        env: unsetEnv,
      });

      expect(preflight.ready, `preflight should accept ${amount} USDC`).toBe(true);
      expect(preflight.warnings).toEqual([]);
      expect(preflight.autoPayCeiling).toBe(DEFAULT_MAX_AUTO_PAY_USDC);
      expect(() => assertAutoPaymentWithinCeiling({ price: amount, env: unsetEnv })).not.toThrow();
    }
  });

  it("treats blank or whitespace-only MINDVAULT_MAX_AUTO_PAY_USDC as unset", () => {
    for (const blank of ["", "   "]) {
      const env = { MINDVAULT_MAX_AUTO_PAY_USDC: blank } as NodeJS.ProcessEnv;
      const preflight = buildPaymentPreflight({
        operation: "buy",
        walletPublicKey: WALLET,
        usdcBalance: "15",
        nativeBalance: "2",
        expectedAmount: "9.99",
        network: "stellar:testnet",
        env,
      });

      expect(preflight.ready).toBe(true);
      expect(preflight.warnings).toEqual([]);
      expect(preflight.autoPayCeiling).toBe(DEFAULT_MAX_AUTO_PAY_USDC);
      expect(() => assertAutoPaymentWithinCeiling({ price: "9.99", env })).not.toThrow();
    }
  });

  it("warns when a buy exceeds the effective ceiling unless maxAutoPayUsdc covers it", () => {
    const unsetEnv = {} as NodeJS.ProcessEnv;

    const blocked = buildPaymentPreflight({
      operation: "buy",
      walletPublicKey: WALLET,
      usdcBalance: "50",
      nativeBalance: "5",
      expectedAmount: "10.0000001",
      network: "stellar:testnet",
      env: unsetEnv,
    });
    expect(blocked.ready).toBe(false);
    expect(blocked.autoPayCeiling).toBe(DEFAULT_MAX_AUTO_PAY_USDC);
    expect(blocked.warnings.some((w) => w.includes("exceeds the automatic payment ceiling"))).toBe(
      true,
    );
    expect(checkAutoPaymentCeiling({ price: "10.0000001", env: unsetEnv }).allowed).toBe(false);

    const overridden = buildPaymentPreflight({
      operation: "buy",
      walletPublicKey: WALLET,
      usdcBalance: "50",
      nativeBalance: "5",
      expectedAmount: "10.0000001",
      maxAutoPayUsdc: "10.0000001",
      network: "stellar:testnet",
      env: unsetEnv,
    });
    expect(overridden.ready).toBe(true);
    expect(overridden.warnings).toEqual([]);
  });

  it("honors a configured MINDVAULT_MAX_AUTO_PAY_USDC ceiling including 0", () => {
    const env = { MINDVAULT_MAX_AUTO_PAY_USDC: "25" } as NodeJS.ProcessEnv;
    const ok = buildPaymentPreflight({
      operation: "buy",
      walletPublicKey: WALLET,
      usdcBalance: "30",
      nativeBalance: "2",
      expectedAmount: "25.00",
      network: "stellar:testnet",
      env,
    });
    expect(ok.ready).toBe(true);
    expect(ok.autoPayCeiling).toBe("25");

    const zeroCeiling = buildPaymentPreflight({
      operation: "buy",
      walletPublicKey: WALLET,
      usdcBalance: "30",
      nativeBalance: "2",
      expectedAmount: "0.01",
      network: "stellar:testnet",
      env: { MINDVAULT_MAX_AUTO_PAY_USDC: "0" } as NodeJS.ProcessEnv,
    });
    expect(zeroCeiling.ready).toBe(false);
    expect(zeroCeiling.autoPayCeiling).toBe("0");
  });

  it("never throws when MINDVAULT_MAX_AUTO_PAY_USDC is malformed", () => {
    const result = buildPaymentPreflight({
      operation: "buy",
      walletPublicKey: WALLET,
      usdcBalance: "10",
      nativeBalance: "2",
      expectedAmount: "1.00",
      network: "stellar:testnet",
      env: { MINDVAULT_MAX_AUTO_PAY_USDC: "not-a-number" } as NodeJS.ProcessEnv,
    });

    expect(result.ready).toBe(false);
    expect(result.autoPayCeiling).toBeNull();
    expect(result.warnings.some((w) => w.includes("MINDVAULT_MAX_AUTO_PAY_USDC"))).toBe(true);
  });

  it("does not apply the buy auto-pay ceiling to verification preflights", () => {
    const result = buildPaymentPreflight({
      operation: "verification",
      walletPublicKey: WALLET,
      usdcBalance: "20",
      nativeBalance: "2",
      expectedAmount: "15.00",
      network: "stellar:testnet",
      env: { MINDVAULT_MAX_AUTO_PAY_USDC: "5" } as NodeJS.ProcessEnv,
    });

    expect(result.ready).toBe(true);
    expect(result.autoPayCeiling).toBeNull();
    expect(result.warnings).toEqual([]);
  });

  it("compares USDC balances using exact 7-decimal stroop math", () => {
    const exact = buildPaymentPreflight({
      operation: "buy",
      walletPublicKey: WALLET,
      usdcBalance: "9.9999999",
      nativeBalance: "1",
      expectedAmount: "9.9999999",
      network: "stellar:testnet",
      env: {} as NodeJS.ProcessEnv,
    });
    expect(exact.ready).toBe(true);

    const shortByOneStroop = buildPaymentPreflight({
      operation: "buy",
      walletPublicKey: WALLET,
      usdcBalance: "9.9999998",
      nativeBalance: "1",
      expectedAmount: "9.9999999",
      network: "stellar:testnet",
      env: {} as NodeJS.ProcessEnv,
    });
    expect(shortByOneStroop.ready).toBe(false);
    expect(shortByOneStroop.warnings[0]).toContain("shortfall 0.0000001");

    expect(
      assertMinimumBalance({
        usdcBalance: "9.9999998",
        nativeBalance: "1",
        operation: "buy",
        requiredUsdc: "9.9999999",
      }),
    ).toContain("Shortfall: 0.0000001 USDC");
  });
});
