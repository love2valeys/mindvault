/**
 * Purchase status lookup by client idempotency key — issue #74.
 *
 * When a `mindvault_buy` response is lost (timeout, dropped connection, agent
 * restart) the agent cannot tell whether it paid. If the buy carried an
 * `idempotencyKey`, this ledger records the attempt, and
 * `mindvault_purchase_status` resolves it read-only to one of:
 *
 *   - `unknown`  — no attempt with that key exists for the active profile
 *   - `pending`  — started but no outcome recorded (in flight, or interrupted)
 *   - `settled`  — payment succeeded; receipt details are included
 *   - `failed`   — the attempt ended without a payment
 *
 * Records are scoped to the wallet profile: a key from another profile reads as
 * `unknown`, so one buyer can never learn about another's purchases.
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname, join } from "path";
import { purchasesFilePath } from "./purchaseHistory.js";
import { isAmbiguousOutcome } from "./publishRetryPolicy.js";

export type PurchaseAttemptState = "pending" | "settled" | "failed";
export type PurchaseLookupState = PurchaseAttemptState | "unknown";

export interface PurchaseAttempt {
  profile: string;
  idempotencyKey: string;
  resourceId: string;
  state: PurchaseAttemptState;
  startedAt: string;
  updatedAt: string;
  txHash: string | null;
  receiptRef: string | null;
  error: string | null;
}

interface AttemptFile {
  version: 1;
  attempts: PurchaseAttempt[];
}

/** Keep the ledger bounded; the oldest attempts are dropped first. */
export const MAX_PURCHASE_ATTEMPTS = 500;

export const PURCHASE_STATUS_OUTPUT_SCHEMA = {
  type: "object",
  properties: {
    idempotencyKey: { type: "string" },
    state: { type: "string", enum: ["unknown", "pending", "settled", "failed"] },
    resourceId: { type: ["string", "null"] },
    txHash: { type: ["string", "null"] },
    receiptRef: { type: ["string", "null"] },
    startedAt: { type: ["string", "null"] },
    updatedAt: { type: ["string", "null"] },
    message: { type: "string" },
    next: { type: "string" },
  },
  required: ["idempotencyKey", "state", "message"],
} as const;

export function purchaseAttemptsFilePath(): string {
  return join(dirname(purchasesFilePath()), "purchase-attempts.json");
}

function load(): AttemptFile {
  const file = purchaseAttemptsFilePath();
  if (!existsSync(file)) return { version: 1, attempts: [] };
  try {
    const raw = JSON.parse(readFileSync(file, "utf-8"));
    if (!raw || !Array.isArray(raw.attempts)) return { version: 1, attempts: [] };
    const attempts = (raw.attempts as unknown[]).filter(
      (a): a is PurchaseAttempt =>
        !!a &&
        typeof a === "object" &&
        typeof (a as PurchaseAttempt).profile === "string" &&
        typeof (a as PurchaseAttempt).idempotencyKey === "string" &&
        ["pending", "settled", "failed"].includes((a as PurchaseAttempt).state),
    );
    return { version: 1, attempts };
  } catch {
    return { version: 1, attempts: [] };
  }
}

function save(store: AttemptFile): void {
  const file = purchaseAttemptsFilePath();
  mkdirSync(dirname(file), { recursive: true });
  const attempts = store.attempts.slice(-MAX_PURCHASE_ATTEMPTS);
  writeFileSync(file, JSON.stringify({ version: 1, attempts }, null, 2), { mode: 0o600 });
}

/** Find the attempt for exactly this profile and key. */
export function findPurchaseAttempt(
  profile: string,
  idempotencyKey: string,
): PurchaseAttempt | null {
  return (
    load().attempts.find((a) => a.profile === profile && a.idempotencyKey === idempotencyKey) ??
    null
  );
}

/** Pending attempts for one profile, used by the read-only spend estimate. */
export function listPendingPurchaseAttempts(profile: string): PurchaseAttempt[] {
  return load().attempts.filter(
    (attempt) => attempt.profile === profile && attempt.state === "pending",
  );
}

function upsert(attempt: PurchaseAttempt): void {
  const store = load();
  const rest = store.attempts.filter(
    (a) => !(a.profile === attempt.profile && a.idempotencyKey === attempt.idempotencyKey),
  );
  save({ version: 1, attempts: [...rest, attempt] });
}

/** Read-only lookup, returned as the tool's JSON result. */
export function lookupPurchaseStatus(profile: string, idempotencyKey: string): string {
  const attempt = findPurchaseAttempt(profile, idempotencyKey);
  if (!attempt) {
    return JSON.stringify(
      {
        idempotencyKey,
        state: "unknown",
        resourceId: null,
        txHash: null,
        receiptRef: null,
        startedAt: null,
        updatedAt: null,
        message: "No purchase attempt with this idempotency key exists for the active profile.",
        next: "No payment was recorded under this key. It is safe to call mindvault_buy with it.",
      },
      null,
      2,
    );
  }

  const copy: Record<PurchaseAttemptState, { message: string; next: string }> = {
    pending: {
      message: "The purchase was started but no outcome was recorded; it may still be in flight.",
      next: "Check mindvault_purchase_history for a receipt for this resource before paying again.",
    },
    settled: {
      message: "The purchase settled.",
      next: "Use the txHash with mindvault_tx_status to confirm on-chain settlement.",
    },
    failed: {
      message: attempt.error
        ? `The purchase ended without a payment: ${attempt.error}`
        : "The purchase ended without a payment.",
      next: "No payment was made for this attempt; fix the cause and call mindvault_buy again.",
    },
  };

  return JSON.stringify(
    {
      idempotencyKey,
      state: attempt.state,
      resourceId: attempt.resourceId,
      txHash: attempt.txHash,
      receiptRef: attempt.receiptRef,
      startedAt: attempt.startedAt,
      updatedAt: attempt.updatedAt,
      ...copy[attempt.state],
    },
    null,
    2,
  );
}

function settledDetails(
  result: string,
): { txHash: string | null; receiptRef: string | null } | null {
  try {
    const parsed = JSON.parse(result) as Record<string, any>;
    if (parsed?.after?.purchased !== true) return null;
    const ref = parsed.after?.receipt?.paymentId;
    return {
      txHash: typeof parsed.txHash === "string" ? parsed.txHash : null,
      receiptRef: ref != null ? String(ref) : null,
    };
  } catch {
    return null;
  }
}

/**
 * Run a buy under an idempotency key. Without a key (or on a dry run) the buy
 * runs untouched. A key that already settled returns that record instead of
 * paying a second time.
 */
export async function trackPurchaseAttempt(
  profile: string,
  idempotencyKey: string | undefined,
  resourceId: string,
  dryRun: boolean,
  run: () => Promise<string>,
): Promise<string> {
  if (!idempotencyKey || dryRun) return run();

  const existing = findPurchaseAttempt(profile, idempotencyKey);
  if (existing?.state === "settled") return lookupPurchaseStatus(profile, idempotencyKey);

  const startedAt = new Date().toISOString();
  const base: PurchaseAttempt = {
    profile,
    idempotencyKey,
    resourceId,
    state: "pending",
    startedAt,
    updatedAt: startedAt,
    txHash: null,
    receiptRef: null,
    error: null,
  };
  upsert(base);

  try {
    const result = await run();
    const settled = settledDetails(result);
    upsert(
      settled
        ? { ...base, ...settled, state: "settled", updatedAt: new Date().toISOString() }
        : {
            ...base,
            state: "failed",
            error: "no payment was submitted",
            updatedAt: new Date().toISOString(),
          },
    );
    return result;
  } catch (err) {
    // A timeout or lost connection may have happened after the payment went
    // out, so it stays pending rather than being reported as a failure.
    if (!isAmbiguousOutcome(err)) {
      const message = err instanceof Error ? err.message : String(err);
      upsert({
        ...base,
        state: "failed",
        error: message.slice(0, 200),
        updatedAt: new Date().toISOString(),
      });
    }
    throw err;
  }
}
