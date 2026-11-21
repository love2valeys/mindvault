import { describe, expect, it, vi } from "vitest";

import {
  CANCELLABLE_TOOLS,
  ToolCanceledError,
  cancellableSleep,
  canceledToolResult,
  currentToolSignal,
  isToolCanceled,
  runCancellable,
} from "./toolCancellation.js";
import { fetchWithTimeout } from "./httpTimeout.js";

describe("runCancellable", () => {
  it("runs a tool with no signal, or a non-cancellable tool, untouched", async () => {
    const controller = new AbortController();
    expect(await runCancellable("mindvault_publish_status", undefined, async () => 1)).toBe(1);
    const buy = vi.fn(async () => "paid");
    controller.abort();
    expect(await runCancellable("mindvault_buy", controller.signal, buy)).toBe("paid");
    expect(CANCELLABLE_TOOLS.has("mindvault_buy")).toBe(false);
  });

  it("rejects without starting work when already canceled", async () => {
    const controller = new AbortController();
    controller.abort();
    const work = vi.fn(async () => "x");
    await expect(
      runCancellable("mindvault_publish_status", controller.signal, work),
    ).rejects.toBeInstanceOf(ToolCanceledError);
    expect(work).not.toHaveBeenCalled();
  });

  it("stops promptly when canceled mid-flight and removes its listener", async () => {
    const controller = new AbortController();
    const removeSpy = vi.spyOn(controller.signal, "removeEventListener");
    const pending = runCancellable("mindvault_publish_status", controller.signal, () =>
      new Promise<string>(() => {}),
    );
    controller.abort();
    await expect(pending).rejects.toSatisfy(isToolCanceled);
    expect(removeSpy).toHaveBeenCalledWith("abort", expect.any(Function));
  });

  it("exposes the signal to the work underneath", async () => {
    const controller = new AbortController();
    const seen = await runCancellable(
      "mindvault_tx_status",
      controller.signal,
      async () => currentToolSignal(),
    );
    expect(seen).toBe(controller.signal);
    expect(currentToolSignal()).toBeUndefined();
  });
});

describe("cancellableSleep", () => {
  it("resolves normally without a signal", async () => {
    await expect(cancellableSleep(1)).resolves.toBeUndefined();
  });

  it("clears its timer and rejects when canceled", async () => {
    vi.useFakeTimers();
    try {
      const controller = new AbortController();
      const sleeping = cancellableSleep(60_000, controller.signal);
      expect(vi.getTimerCount()).toBe(1);
      controller.abort();
      await expect(sleeping).rejects.toBeInstanceOf(ToolCanceledError);
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops a polling loop inside runCancellable", async () => {
    const controller = new AbortController();
    const polls = vi.fn();
    const result = runCancellable("mindvault_publish_status", controller.signal, async () => {
      for (;;) {
        polls();
        await cancellableSleep(10_000);
      }
    });
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(ToolCanceledError);
    expect(polls).toHaveBeenCalledTimes(1);
  });
});

describe("fetch cancellation", () => {
  it("aborts the in-flight request of a canceled call", async () => {
    const controller = new AbortController();
    let requestSignal: AbortSignal | undefined;
    const fetchImpl = ((_input: unknown, init?: RequestInit) =>
      new Promise((_resolve, reject) => {
        requestSignal = init?.signal ?? undefined;
        requestSignal?.addEventListener("abort", () =>
          reject(Object.assign(new Error("aborted"), { name: "AbortError" })),
        );
      })) as unknown as typeof fetch;

    const result = runCancellable("mindvault_tx_status", controller.signal, () =>
      fetchWithTimeout(fetchImpl, "https://example.test", undefined, "http", 60_000),
    );
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(ToolCanceledError);
    expect(requestSignal?.aborted).toBe(true);
  });
});

describe("canceledToolResult", () => {
  it("is a stable, structured canceled result", () => {
    const result = canceledToolResult("mindvault_export_receipts");
    expect(result.isError).toBe(true);
    expect(result.structuredContent).toEqual({
      status: "canceled",
      tool: "mindvault_export_receipts",
      message: "The call was canceled before it finished. No state was changed.",
    });
    expect(JSON.parse(result.content[0].text)).toEqual(result.structuredContent);
  });

  it("retains the transaction hash and last known status for a canceled watch", () => {
    const result = canceledToolResult("mindvault_tx_status", {
      hash: "abc123",
      status: "IN_PROGRESS",
    });
    expect(result.structuredContent).toMatchObject({
      status: "canceled",
      hash: "abc123",
      transactionStatus: "IN_PROGRESS",
    });
  });
});
