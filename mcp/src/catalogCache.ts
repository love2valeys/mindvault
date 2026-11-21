/**
 * Offline read-only cache for catalog tools (#556).
 *
 * `browse`, `search`, and `preview` normally depend on the MindVault catalog
 * API. When that service is unreachable (a transport-level failure — DNS,
 * refused connection, or timeout), these tools fall back to the last snapshot
 * captured from a successful read, clearly labelled with the age of the
 * snapshot. Mutating and payment tools never consult this cache.
 *
 * The cache is held in memory for the lifetime of the MCP server process — the
 * same scope as the profile state — so it is available for later offline calls
 * without persisting data to disk. There is deliberately no filesystem use:
 * that keeps the failure "no cache present" deterministic and independent of
 * anything left behind by an earlier run.
 */

import { currentCatalogCacheScope } from "./catalogCacheScope.js";
import {
  isRetryableError,
  isRetryableStatus,
  retryAfterDelay,
  retryPolicyFromEnv,
  withRetry,
  type RetryAttemptInfo,
  type RetryPolicy,
} from "./retry.js";

export interface CatalogSnapshot {
  resources: unknown[];
  savedAtMs: number;
}

export interface PreviewSnapshot {
  meta: unknown;
  savedAtMs: number;
}

/**
 * Default age at which a snapshot is labelled "stale" rather than "fresh".
 *
 * Kept as the default for the configurable TTL added in #573; existing
 * importers keep the constant they had.
 */
export const CATALOG_CACHE_STALE_AFTER_MS = 24 * 60 * 60 * 1000;

/** Environment variables controlling cache lifetime (#573). */
export const CATALOG_CACHE_ENV_VARS = {
  /** Age at which a snapshot starts being served with a staleness warning. */
  ttlMs: "MINDVAULT_CATALOG_CACHE_TTL_MS",
  /** Age past which a snapshot is not served at all. 0 disables the limit. */
  maxAgeMs: "MINDVAULT_CATALOG_CACHE_MAX_AGE_MS",
} as const;

/**
 * How long a snapshot stays useful.
 *
 * Two limits rather than one, because "stale" and "useless" are different
 * questions. Past `ttlMs` a snapshot is still served — offline, a day-old
 * catalog beats no catalog — but carries a warning. Past `maxAgeMs` it is
 * withheld entirely, which is what an operator wants when acting on outdated
 * pricing is worse than failing.
 *
 * `maxAgeMs` of 0 means "never withhold", preserving the behaviour before
 * #573: always serve, label when old.
 */
export interface CatalogCacheConfig {
  ttlMs: number;
  maxAgeMs: number;
}

export const DEFAULT_CATALOG_CACHE_CONFIG: CatalogCacheConfig = {
  ttlMs: CATALOG_CACHE_STALE_AFTER_MS,
  maxAgeMs: 0,
};

/**
 * Parse one duration from the environment.
 *
 * A malformed or negative value falls back to the default rather than failing
 * startup: an offline cache is a resilience feature, and a typo in its tuning
 * must not stop the server from serving.
 */
function parseDuration(raw: string | undefined, fallback: number): number {
  if (raw == null || raw.trim() === "") return fallback;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed) || parsed < 0) return fallback;
  return Math.floor(parsed);
}

/** Resolve the cache lifetime from the environment. */
export function resolveCatalogCacheConfig(
  env: NodeJS.ProcessEnv = process.env,
): CatalogCacheConfig {
  const ttlMs = parseDuration(
    env[CATALOG_CACHE_ENV_VARS.ttlMs],
    DEFAULT_CATALOG_CACHE_CONFIG.ttlMs,
  );
  const maxAgeMs = parseDuration(
    env[CATALOG_CACHE_ENV_VARS.maxAgeMs],
    DEFAULT_CATALOG_CACHE_CONFIG.maxAgeMs,
  );

  // A max age below the staleness threshold would mean a snapshot is withheld
  // before it is ever labelled stale, making ttlMs unreachable. Honour the
  // stricter intent by pulling ttl down rather than silently ignoring one.
  return maxAgeMs > 0 && maxAgeMs < ttlMs ? { ttlMs: maxAgeMs, maxAgeMs } : { ttlMs, maxAgeMs };
}

let config: CatalogCacheConfig = { ...DEFAULT_CATALOG_CACHE_CONFIG };
// Snapshots are keyed by cache scope (#85) so one context never serves another.
let catalogs = new Map<string, CatalogSnapshot>();
let previews: Record<string, PreviewSnapshot> = {};

/** Apply a cache lifetime. Called once at startup from the environment. */
export function configureCatalogCache(next: CatalogCacheConfig): void {
  config = { ...next };
}

/** Load the lifetime from the environment. */
export function initCatalogCache(env: NodeJS.ProcessEnv = process.env): CatalogCacheConfig {
  configureCatalogCache(resolveCatalogCacheConfig(env));
  return getCatalogCacheConfig();
}

/** The active cache lifetime. */
export function getCatalogCacheConfig(): CatalogCacheConfig {
  return { ...config };
}

/** Whether a snapshot taken at `savedAtMs` is too old to serve at all. */
export function isExpired(savedAtMs: number, nowMs: number = Date.now()): boolean {
  if (config.maxAgeMs <= 0) return false;
  return nowMs - savedAtMs > config.maxAgeMs;
}

/** Whether a snapshot is past its TTL and should carry a staleness warning. */
export function isStale(savedAtMs: number, nowMs: number = Date.now()): boolean {
  return Math.max(0, nowMs - savedAtMs) > config.ttlMs;
}

/** Record a successful catalog read. The raw resource array is snapshotted. */
export function recordCatalogSnapshot(
  resources: unknown[],
  scope: string = currentCatalogCacheScope(),
  nowMs: number = Date.now(),
): void {
  catalogs.set(scope, { resources, savedAtMs: nowMs });
}

/**
 * The latest catalog snapshot, or null when none has ever been captured — or
 * when the one held is past `maxAgeMs` (#573).
 *
 * An expired snapshot is dropped rather than merely hidden, so the memory is
 * released and a later call cannot resurrect it.
 */
export function getCatalogSnapshot(
  nowMs: number = Date.now(),
  scope: string = currentCatalogCacheScope(),
): CatalogSnapshot | null {
  const catalog = catalogs.get(scope);
  if (!catalog) return null;
  if (isExpired(catalog.savedAtMs, nowMs)) {
    catalogs.delete(scope);
    return null;
  }
  return { savedAtMs: catalog.savedAtMs, resources: [...catalog.resources] };
}

/** Record a successful preview read for a resource id. */
export function recordPreviewSnapshot(
  resourceId: string,
  meta: unknown,
  scope: string = currentCatalogCacheScope(),
  nowMs: number = Date.now(),
): void {
  previews[`${scope}\u0000${resourceId}`] = { meta, savedAtMs: nowMs };
}

/**
 * The latest preview snapshot for a resource id, or null when absent or past
 * `maxAgeMs` (#573).
 */
export function getPreviewSnapshot(
  resourceId: string,
  nowMs: number = Date.now(),
  scope: string = currentCatalogCacheScope(),
): PreviewSnapshot | null {
  const key = `${scope}\u0000${resourceId}`;
  const snap = previews[key];
  if (!snap) return null;
  if (isExpired(snap.savedAtMs, nowMs)) {
    delete previews[key];
    return null;
  }
  return { meta: snap.meta, savedAtMs: snap.savedAtMs };
}

function describeAge(ms: number): string {
  const minutes = Math.max(0, Math.round(ms / 60000));
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} hr`;
  const days = Math.round(hours / 24);
  return `${days} day`;
}

/**
 * Why the live catalog could not be read, so the snapshot label can say so.
 *
 * "Unreachable" and "the API answered HTTP 503" are different facts, and an
 * operator reading a tool transcript needs the real one: the first points at
 * the network, the second at the service. The label used to assert the first
 * for both (#837).
 */
export type CatalogFallbackReason = { kind: "unreachable" } | { kind: "status"; status: number };

/** Render the cause clause used in the snapshot label. */
function describeReason(reason: CatalogFallbackReason | undefined): string {
  if (reason?.kind === "status") return `catalog API returned HTTP ${reason.status}`;
  return "catalog API unreachable";
}

/**
 * Agent-facing label describing the served snapshot, its age, and why the live
 * read failed. "Fresh" snapshots are served normally; "stale" ones carry a
 * warning to re-check on-chain when freshness matters, mirroring the
 * cacheStaleness guidance.
 */
export function catalogCacheLabel(
  savedAtMs: number,
  nowMs: number = Date.now(),
  reason?: CatalogFallbackReason,
): string {
  const ageMs = Math.max(0, nowMs - savedAtMs);
  const age = describeAge(ageMs);
  if (ageMs <= config.ttlMs) {
    return `Offline catalog snapshot served (cached ${age} ago) — ${describeReason(reason)}.`;
  }
  return (
    `⚠ Offline catalog snapshot served (cached ${age} ago) — ${describeReason(reason)}; ` +
    `stale, results may be outdated. ` +
    `Confirm a specific resource on-chain with mindvault_registry_lookup when freshness matters.`
  );
}

/** Default retry-log label for catalog cache revalidation reads (`GET /resources`). */
export const CATALOG_REVALIDATION_LABEL = "GET /resources";

/** Minimal HTTP response shape accepted by {@link revalidateCatalogCache}. */
export interface CatalogRevalidationResponse<T = unknown> {
  ok: boolean;
  status: number;
  data: T;
  headers?: Headers | Record<string, string>;
}

export interface RevalidateCatalogOptions<
  T extends CatalogRevalidationResponse = CatalogRevalidationResponse,
> {
  /** Performs one idempotent `GET /resources` (or filtered query) attempt. */
  fetcher: () => Promise<T>;
  /** Retry policy for transient gateway/transport errors; defaults to env policy. */
  policy?: RetryPolicy;
  /** Label used in retry logs; defaults to {@link CATALOG_REVALIDATION_LABEL}. */
  label?: string;
  /** Cache scope key; defaults to `currentCatalogCacheScope()`. */
  scope?: string;
  /** Optional hook called before each retry delay for observability. */
  onRetry?: (info: RetryAttemptInfo) => void;
  /** Injectable clock, sleep, and RNG so backoff is deterministic in tests. */
  now?: () => number;
  sleep?: (ms: number) => Promise<void>;
  random?: () => number;
}

export type RevalidateCatalogResult<
  T extends CatalogRevalidationResponse = CatalogRevalidationResponse,
> =
  | {
      revalidated: true;
      resources: unknown[];
      response: T;
      attempts: number;
    }
  | {
      revalidated: false;
      snapshot: CatalogSnapshot | null;
      reason: CatalogFallbackReason | null;
      response?: T;
      error?: unknown;
      attempts: number;
    };

function readRetryAfterHeader(
  headers: Headers | Record<string, string> | undefined,
): string | null {
  if (!headers) return null;
  if (typeof (headers as Headers).get === "function") {
    return (headers as Headers).get("retry-after");
  }
  for (const [key, value] of Object.entries(headers as Record<string, string>)) {
    if (key.toLowerCase() === "retry-after") return value;
  }
  return null;
}

/**
 * Revalidate the catalog cache (`GET /resources`) under bounded, jittered
 * exponential backoff (#220).
 *
 * Transient gateway and throttling errors (408/425/429/5xx) and transport
 * failures are retried with the same retry policy used for other idempotent
 * reads (honouring `Retry-After`, clamped to `maxDelayMs`) so a single gateway
 * blip does not pin a stale snapshot for a full TTL period.
 *
 * - On recovery, records the fresh snapshot immediately and returns it.
 * - After exhausting retries on a retryable gateway/transport error, returns
 *   the cached snapshot (if any and unexpired) alongside the accurate fallback
 *   reason so callers can decide whether to serve stale data or report failure.
 * - On non-retryable client errors (e.g. 400/401/403/404) or caller aborts,
 *   stops immediately without retrying and never masks the error behind cache.
 */
export async function revalidateCatalogCache<
  T extends CatalogRevalidationResponse = CatalogRevalidationResponse,
>(opts: RevalidateCatalogOptions<T>): Promise<RevalidateCatalogResult<T>> {
  const policy = opts.policy ?? retryPolicyFromEnv();
  const label = opts.label ?? CATALOG_REVALIDATION_LABEL;
  const scope = opts.scope ?? currentCatalogCacheScope();
  const now = opts.now ?? Date.now;

  let attempts = 0;
  try {
    const response = await withRetry(
      () => {
        attempts += 1;
        return opts.fetcher();
      },
      {
        policy,
        label,
        shouldRetryResult: (res) => !res.ok && isRetryableStatus(res.status),
        describeResult: (res) => `HTTP ${res.status}`,
        delayFromResult: (res) =>
          retryAfterDelay(readRetryAfterHeader(res.headers), policy, now()),
        onRetry: opts.onRetry,
        sleep: opts.sleep,
        random: opts.random,
      },
    );

    if (response.ok) {
      const resources = Array.isArray(response.data) ? response.data : [];
      recordCatalogSnapshot(resources, scope, now());
      return { revalidated: true, resources, response, attempts };
    }

    const reason: CatalogFallbackReason | null = isRetryableStatus(response.status)
      ? { kind: "status", status: response.status }
      : null;
    const snapshot = reason ? getCatalogSnapshot(now(), scope) : null;
    return { revalidated: false, snapshot, reason, response, attempts };
  } catch (error) {
    const reason: CatalogFallbackReason | null = isRetryableError(error)
      ? { kind: "unreachable" }
      : null;
    const snapshot = reason ? getCatalogSnapshot(now(), scope) : null;
    return { revalidated: false, snapshot, reason, error, attempts };
  }
}

/**
 * Test-only: clear the in-memory snapshots so the "no cache" path is testable.
 * Also restores the default lifetime, so one test's TTL cannot leak into the
 * next.
 */
export function _clearCatalogCache(): void {
  catalogs = new Map();
  previews = {};
  config = { ...DEFAULT_CATALOG_CACHE_CONFIG };
}
