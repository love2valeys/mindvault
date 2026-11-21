/**
 * Bounded cancellation for long-running MCP tools — issue #76.
 *
 * The MCP client sends `notifications/cancelled` for a request it no longer
 * wants; the SDK surfaces that as an `AbortSignal` on the request handler. This
 * module turns the signal into prompt, clean cancellation for the tools that can
 * run for a long time without side effects worth protecting:
 *
 *   - `mindvault_publish_status` (verification polling)
 *   - `mindvault_tx_status`
 *   - `mindvault_export_receipts`
 *
 * Tools that move money or write state (`buy`, `publish`, `register_onchain`,
 * ...) are deliberately NOT cancellable: abandoning one mid-flight would leave
 * the outcome unknown. They keep running to completion.
 *
 * The signal travels through an `AsyncLocalStorage`, so the shared sleep helper
 * and `fetchWithTimeout` pick it up without every call site passing it along.
 * A canceled call clears its poll timer, aborts its in-flight request, and
 * returns a stable `canceled` result. Nothing is persisted on the way out.
 */

import { AsyncLocalStorage } from "node:async_hooks";

/** Tools whose work can be abandoned safely. */
export const CANCELLABLE_TOOLS: ReadonlySet<string> = new Set([
  "mindvault_publish_status",
  "mindvault_tx_status",
  "mindvault_export_receipts",
]);

export class ToolCanceledError extends Error {
  readonly name = "ToolCanceledError";
  constructor(message = "The tool call was canceled by the client.") {
    super(message);
  }
}

const storage = new AsyncLocalStorage<AbortSignal>();

/** The cancellation signal of the tool call currently running, if any. */
export function currentToolSignal(): AbortSignal | undefined {
  return storage.getStore();
}

/** Sleep that ends early, with a cleared timer, when the current call is canceled. */
export function cancellableSleep(ms: number, signal = currentToolSignal()): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    if (signal?.aborted) return reject(new ToolCanceledError());
    const onAbort = () => {
      clearTimeout(timer);
      reject(new ToolCanceledError());
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

/**
 * Run a tool under the client's cancellation signal.
 *
 * Tools outside {@link CANCELLABLE_TOOLS} (and calls with no signal) run
 * untouched. Otherwise the call rejects with {@link ToolCanceledError} as soon
 * as the signal fires, while the signal also reaches the work underneath.
 */
export async function runCancellable<T>(
  toolName: string,
  signal: AbortSignal | undefined,
  fn: () => Promise<T>,
): Promise<T> {
  if (!signal || !CANCELLABLE_TOOLS.has(toolName)) return fn();
  if (signal.aborted) throw new ToolCanceledError();

  let onAbort: (() => void) | undefined;
  const canceled = new Promise<never>((_, reject) => {
    onAbort = () => reject(new ToolCanceledError());
    signal.addEventListener("abort", onAbort, { once: true });
  });

  try {
    return await Promise.race([storage.run(signal, fn), canceled]);
  } finally {
    if (onAbort) signal.removeEventListener("abort", onAbort);
  }
}

/** True for the error {@link runCancellable} and {@link cancellableSleep} throw. */
export function isToolCanceled(error: unknown): boolean {
  return error instanceof ToolCanceledError;
}

/** The stable result an MCP client receives for a canceled call. */
export function canceledToolResult(
  toolName: string,
  transaction?: { hash: string; status: string },
) {
  const structuredContent = {
    status: "canceled",
    tool: toolName,
    message: "The call was canceled before it finished. No state was changed.",
    ...(transaction ? { hash: transaction.hash, transactionStatus: transaction.status } : {}),
  };
  return {
    content: [{ type: "text" as const, text: JSON.stringify(structuredContent, null, 2) }],
    isError: true,
    structuredContent,
  };
}
