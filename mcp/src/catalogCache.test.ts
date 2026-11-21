import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  recordCatalogSnapshot,
  getCatalogSnapshot,
  recordPreviewSnapshot,
  getPreviewSnapshot,
  catalogCacheLabel,
  revalidateCatalogCache,
  _clearCatalogCache,
  CATALOG_CACHE_STALE_AFTER_MS,
} from "./catalogCache.js";
import type { RetryAttemptInfo, RetryPolicy } from "./retry.js";

beforeEach(() => {
  _clearCatalogCache();
});

describe("catalog snapshot (#556)", () => {
  it("records and returns the last successful catalog read", () => {
    const resources = [
      { id: "a", title: "Alpha" },
      { id: "b", title: "Beta" },
    ];
    recordCatalogSnapshot(resources);

    const snapshot = getCatalogSnapshot();
    expect(snapshot).not.toBeNull();
    expect(snapshot!.resources).toEqual(resources);
    expect(snapshot!.savedAtMs).toBeGreaterThan(0);
  });

  it("returns a copy so the caller cannot mutate the cache", () => {
    recordCatalogSnapshot([{ id: "a", title: "Alpha" }]);
    const first = getCatalogSnapshot()!;
    const second = getCatalogSnapshot()!;
    expect(first).not.toBe(second);
    expect(first.resources).not.toBe(second.resources);
  });

  it("returns null before anything has been cached (no-cache path)", () => {
    expect(getCatalogSnapshot()).toBeNull();
  });

  it("overwrites the previous catalog read with the latest one", () => {
    recordCatalogSnapshot([{ id: "a" }]);
    recordCatalogSnapshot([{ id: "b" }, { id: "c" }]);
    expect(getCatalogSnapshot()!.resources).toEqual([{ id: "b" }, { id: "c" }]);
  });
});

describe("preview snapshot (#556)", () => {
  it("records and returns a per-resource preview meta", () => {
    recordPreviewSnapshot("res-1", { title: "Alpha", price: "5" });
    const snap = getPreviewSnapshot("res-1");
    expect(snap).not.toBeNull();
    expect(snap!.meta).toEqual({ title: "Alpha", price: "5" });
  });

  it("returns null for a resource that was never previewed", () => {
    expect(getPreviewSnapshot("res-missing")).toBeNull();
  });

  it("keeps previews for different resources independent", () => {
    recordPreviewSnapshot("res-1", { title: "Alpha" });
    recordPreviewSnapshot("res-2", { title: "Beta" });
    expect(getPreviewSnapshot("res-1")!.meta).toEqual({ title: "Alpha" });
    expect(getPreviewSnapshot("res-2")!.meta).toEqual({ title: "Beta" });
  });
});

describe("catalogCacheLabel (#556)", () => {
  it("labels a fresh snapshot with its age", () => {
    const now = 1_000_000;
    const label = catalogCacheLabel(now - 5 * 60_000, now); // 5 min old
    expect(label).toContain("cached 5 min ago");
    expect(label).toContain("Offline catalog snapshot served");
    expect(label).not.toContain("stale");
  });

  it("warns on a stale snapshot and points to the on-chain lookup", () => {
    const now = 1_000_000;
    const label = catalogCacheLabel(now - CATALOG_CACHE_STALE_AFTER_MS - 60_000, now);
    expect(label).toContain("stale");
    expect(label).toContain("mindvault_registry_lookup");
  });

  it("labels a snapshot exactly at the fresh/stale boundary as fresh", () => {
    const now = 1_000_000;
    const label = catalogCacheLabel(now - CATALOG_CACHE_STALE_AFTER_MS, now);
    expect(label).not.toContain("stale");
  });
});

describe("catalogCacheLabel reasons (#837)", () => {
  const now = Date.UTC(2026, 0, 1, 12, 0, 0);

  it("defaults to unreachable when no reason is given", () => {
    expect(catalogCacheLabel(now - 60_000, now)).toContain("catalog API unreachable");
  });

  it("names the status when the catalog answered but could not serve", () => {
    const label = catalogCacheLabel(now - 60_000, now, { kind: "status", status: 503 });
    expect(label).toContain("catalog API returned HTTP 503");
    expect(label).not.toContain("unreachable");
  });

  it("keeps the staleness warning alongside the reason", () => {
    const label = catalogCacheLabel(now - CATALOG_CACHE_STALE_AFTER_MS - 60_000, now, {
      kind: "status",
      status: 502,
    });
    expect(label).toContain("HTTP 502");
    expect(label).toContain("stale");
    expect(label).toContain("mindvault_registry_lookup");
  });
});

describe("revalidateCatalogCache retry/backoff (#220)", () => {
  const policy: RetryPolicy = { attempts: 3, baseDelayMs: 100, maxDelayMs: 1_000 };

  it("retries transient gateway errors with exponential backoff and replaces a stale snapshot", async () => {
    const staleAt = Date.UTC(2026, 0, 1, 0, 0, 0);
    const refreshedAt = staleAt + CATALOG_CACHE_STALE_AFTER_MS + 60_000;
    recordCatalogSnapshot([{ id: "stale-1" }], undefined, staleAt);

    const delays: number[] = [];
    const retries: RetryAttemptInfo[] = [];
    const fetcher = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, status: 502, data: { error: "bad gateway" } })
      .mockResolvedValueOnce({ ok: false, status: 503, data: { error: "unavailable" } })
      .mockResolvedValueOnce({ ok: true, status: 200, data: [{ id: "fresh-1" }] });

    const result = await revalidateCatalogCache({
      fetcher,
      policy,
      now: () => refreshedAt,
      sleep: async (ms) => {
        delays.push(ms);
      },
      random: () => 1,
      onRetry: (info) => retries.push(info),
    });

    expect(result.revalidated).toBe(true);
    if (result.revalidated) {
      expect(result.resources).toEqual([{ id: "fresh-1" }]);
    }
    expect(result.attempts).toBe(3);
    expect(fetcher).toHaveBeenCalledTimes(3);
    expect(delays).toEqual([100, 200]);
    expect(retries.map((r) => r.reason)).toEqual(["HTTP 502", "HTTP 503"]);

    // The stale snapshot must be unpinned and replaced with the fresh read.
    const snapshot = getCatalogSnapshot(refreshedAt);
    expect(snapshot).toEqual({ resources: [{ id: "fresh-1" }], savedAtMs: refreshedAt });
  });

  it("retries transient transport failures and honours Retry-After headers", async () => {
    const delays: number[] = [];
    const fetcher = vi
      .fn()
      .mockRejectedValueOnce(new Error("fetch failed"))
      .mockResolvedValueOnce({
        ok: false,
        status: 429,
        data: { error: "rate limited" },
        headers: { "retry-after": "2" },
      })
      .mockResolvedValueOnce({ ok: true, status: 200, data: [{ id: "ok" }] });

    const result = await revalidateCatalogCache({
      fetcher,
      policy: { attempts: 3, baseDelayMs: 100, maxDelayMs: 1_500 },
      sleep: async (ms) => {
        delays.push(ms);
      },
      random: () => 1,
    });

    expect(result.revalidated).toBe(true);
    expect(result.attempts).toBe(3);
    // Attempt 1 used baseDelayMs=100; attempt 2 used Retry-After=2s clamped to maxDelayMs=1500.
    expect(delays).toEqual([100, 1_500]);
  });

  it("falls back to the cached snapshot only after bounded retries are exhausted", async () => {
    const cachedAt = Date.UTC(2026, 0, 1, 10, 0, 0);
    recordCatalogSnapshot([{ id: "cached-1" }], undefined, cachedAt);
    const delays: number[] = [];
    const fetcher = vi
      .fn()
      .mockResolvedValue({ ok: false, status: 503, data: { error: "unavailable" } });

    const result = await revalidateCatalogCache({
      fetcher,
      policy,
      now: () => cachedAt + 60_000,
      sleep: async (ms) => {
        delays.push(ms);
      },
      random: () => 1,
    });

    expect(result.revalidated).toBe(false);
    expect(result.attempts).toBe(3);
    expect(delays).toEqual([100, 200]);
    if (!result.revalidated) {
      expect(result.reason).toEqual({ kind: "status", status: 503 });
      expect(result.snapshot?.resources).toEqual([{ id: "cached-1" }]);
    }
  });

  it("does not retry or serve stale cache on non-retryable 4xx errors or caller aborts", async () => {
    recordCatalogSnapshot([{ id: "cached-1" }]);

    const badReq = vi
      .fn()
      .mockResolvedValue({ ok: false, status: 400, data: { error: "bad filter" } });
    const badRes = await revalidateCatalogCache({
      fetcher: badReq,
      policy,
      sleep: async () => {},
    });
    expect(badRes.revalidated).toBe(false);
    expect(badRes.attempts).toBe(1);
    expect(badReq).toHaveBeenCalledTimes(1);
    if (!badRes.revalidated) {
      expect(badRes.reason).toBeNull();
      expect(badRes.snapshot).toBeNull();
    }

    const abort = new Error("aborted");
    abort.name = "AbortError";
    const abortedFetch = vi.fn().mockRejectedValue(abort);
    const abortRes = await revalidateCatalogCache({
      fetcher: abortedFetch,
      policy,
      sleep: async () => {},
    });
    expect(abortRes.revalidated).toBe(false);
    expect(abortRes.attempts).toBe(1);
    expect(abortedFetch).toHaveBeenCalledTimes(1);
    if (!abortRes.revalidated) {
      expect(abortRes.reason).toBeNull();
      expect(abortRes.snapshot).toBeNull();
    }
  });
});
