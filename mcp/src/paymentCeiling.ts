/**
 * Guardrail for automatic x402 payments.
 *
 * Prices are converted to Stellar's 7-decimal stroop precision before
 * comparison so a ceiling never depends on floating-point rounding.
 */

import { usdcToStroops } from "./usdcAmount.js";

export const DEFAULT_MAX_AUTO_PAY_USDC = "10";
export const DEFAULT_MAX_AUTO_FEE_STROOPS = "100000";

/**
 * Amounts are compared in stroops through the shared converter (#838) rather
 * than a local copy of the 10^7 factor, so a ceiling and the price it guards
 * can never be measured on two different scales.
 */
const toStroops = (value: string): bigint | null => usdcToStroops(value);

export interface ResolvedAutoPaymentCeiling {
  value: string;
  stroops: bigint;
  isDefault: boolean;
}

/**
 * Resolve the effective automatic payment ceiling from the environment.
 *
 * Unset or blank (`""` / whitespace-only) `MINDVAULT_MAX_AUTO_PAY_USDC` falls
 * back to {@link DEFAULT_MAX_AUTO_PAY_USDC} so an empty `.env` placeholder
 * behaves identically to an unset variable rather than failing preflight or
 * live buys.
 */
export function resolveAutoPaymentCeiling(
  env: NodeJS.ProcessEnv = process.env,
): ResolvedAutoPaymentCeiling {
  const raw = env.MINDVAULT_MAX_AUTO_PAY_USDC?.trim();
  const isDefault = !raw;
  const value = raw || DEFAULT_MAX_AUTO_PAY_USDC;
  const stroops = toStroops(value);
  if (stroops === null) {
    throw new Error(
      "MINDVAULT_MAX_AUTO_PAY_USDC must be a non-negative USDC decimal with at most 7 decimal places.",
    );
  }
  return { value, stroops, isDefault };
}

export type AutoPaymentCeilingCheck =
  | {
      allowed: true;
      ceiling: string;
      isDefaultCeiling: boolean;
    }
  | {
      allowed: false;
      reason: "invalid_price" | "invalid_ceiling" | "invalid_override" | "exceeds_ceiling";
      message: string;
      ceiling: string | null;
      isDefaultCeiling: boolean;
    };

/**
 * Pure, non-throwing ceiling evaluator shared by payment preflight and live
 * `mindvault_buy` / `mindvault_buy_lease` guards so dry-run / preflight and
 * live payment decisions cannot drift.
 */
export function checkAutoPaymentCeiling(input: {
  price: unknown;
  maxAutoPayUsdc?: string;
  env?: NodeJS.ProcessEnv;
}): AutoPaymentCeilingCheck {
  const requiredAmount = typeof input.price === "string" ? input.price : String(input.price ?? "");
  const requiredStroops = toStroops(requiredAmount);
  if (requiredStroops === null) {
    return {
      allowed: false,
      reason: "invalid_price",
      message:
        "Automatic payment blocked because the resource price is missing or invalid; no x402 payment was submitted.",
      ceiling: null,
      isDefaultCeiling: false,
    };
  }

  let ceiling: ResolvedAutoPaymentCeiling;
  try {
    ceiling = resolveAutoPaymentCeiling(input.env ?? process.env);
  } catch (err) {
    return {
      allowed: false,
      reason: "invalid_ceiling",
      message: (err as Error).message,
      ceiling: null,
      isDefaultCeiling: false,
    };
  }

  if (requiredStroops <= ceiling.stroops) {
    return {
      allowed: true,
      ceiling: ceiling.value,
      isDefaultCeiling: ceiling.isDefault,
    };
  }

  const overrideStroops =
    input.maxAutoPayUsdc === undefined ? null : toStroops(input.maxAutoPayUsdc);
  if (input.maxAutoPayUsdc !== undefined && overrideStroops === null) {
    return {
      allowed: false,
      reason: "invalid_override",
      message:
        "maxAutoPayUsdc must be a non-negative USDC decimal with at most 7 decimal places.",
      ceiling: ceiling.value,
      isDefaultCeiling: ceiling.isDefault,
    };
  }
  if (overrideStroops !== null && overrideStroops >= requiredStroops) {
    return {
      allowed: true,
      ceiling: ceiling.value,
      isDefaultCeiling: ceiling.isDefault,
    };
  }

  return {
    allowed: false,
    reason: "exceeds_ceiling",
    message:
      `Purchase requires ${requiredAmount} USDC, which exceeds the automatic payment ceiling of ${ceiling.value} USDC. ` +
      `To authorize this purchase, call mindvault_buy with maxAutoPayUsdc: "${requiredAmount}" (or a higher amount).`,
    ceiling: ceiling.value,
    isDefaultCeiling: ceiling.isDefault,
  };
}

/**
 * Prevent an automatic x402 payment above the configured ceiling unless the
 * caller explicitly authorizes a limit that covers the advertised price.
 */
export function assertAutoPaymentWithinCeiling(input: {
  price: unknown;
  maxAutoPayUsdc?: string;
  env?: NodeJS.ProcessEnv;
}): void {
  const result = checkAutoPaymentCeiling(input);
  if (!result.allowed) {
    throw new Error(result.message);
  }
}

/** Bound sponsored Stellar transaction fees for registry mutations. */
export function assertTransactionFeeWithinCeiling(input: {
  feeStroops: string | number | bigint;
  env?: NodeJS.ProcessEnv;
}): void {
  const configured =
    input.env !== undefined
      ? input.env.MINDVAULT_MAX_AUTO_FEE_STROOPS?.trim()
      : process.env.MINDVAULT_MAX_AUTO_FEE_STROOPS?.trim();
  const raw = configured || DEFAULT_MAX_AUTO_FEE_STROOPS;
  if (!/^\d+$/.test(raw))
    throw new Error("MINDVAULT_MAX_AUTO_FEE_STROOPS must be a non-negative integer.");
  const fee = BigInt(String(input.feeStroops));
  const ceiling = BigInt(raw);
  if (fee > ceiling) {
    throw new Error(
      `On-chain mutation fee ${fee} stroops exceeds the automatic fee ceiling of ${ceiling} stroops.`,
    );
  }
}

export interface PriceChangeConfirmation {
  status: "confirmation_needed";
  confirmationRequired: true;
  reason: "price_changed";
  resourceId: string;
  previousPrice: string;
  newPrice: string;
  message: string;
  maxAutoPayUsdc: string | null;
}

/**
 * Handle a price change between initial preview and x402 challenge (#71).
 * Recomputes renewed ceiling validation against the challenged price, and
 * returns a structured confirmation-needed outcome when the price changed.
 */
export function recomputePaymentCeilingAfterPriceChange(input: {
  resourceId: string;
  approvedPrice: string;
  challengedPrice: string;
  maxAutoPayUsdc?: string;
  env?: NodeJS.ProcessEnv;
}): { priceChanged: boolean; confirmation?: PriceChangeConfirmation } {
  const approvedStroops = toStroops(input.approvedPrice);
  const challengedStroops = toStroops(input.challengedPrice);

  if (
    approvedStroops === null ||
    challengedStroops === null ||
    approvedStroops === challengedStroops
  ) {
    return { priceChanged: false };
  }

  // Renewed ceiling validation: if the new price exceeds the auto-pay ceiling,
  // this throws the ceiling error before any payment can be submitted.
  assertAutoPaymentWithinCeiling({
    price: input.challengedPrice,
    maxAutoPayUsdc: input.maxAutoPayUsdc,
    env: input.env,
  });

  return {
    priceChanged: true,
    confirmation: {
      status: "confirmation_needed",
      confirmationRequired: true,
      reason: "price_changed",
      resourceId: input.resourceId,
      previousPrice: input.approvedPrice,
      newPrice: input.challengedPrice,
      message: `Resource price changed from ${input.approvedPrice} USDC to ${input.challengedPrice} USDC between initial preview and x402 challenge. Confirmation is required before authorizing payment at the new price.`,
      maxAutoPayUsdc: input.maxAutoPayUsdc ?? null,
    },
  };
}
