import {
  BASE_URL,
  jsonFetch,
  jsonFetchOnce,
  logRetry,
  RETRY_POLICY,
  SERVICE_OPERATION,
  sourceForUrl,
} from "../runtime.js";
import {
  applyCatalogSort,
  applyClientCatalogFilters,
  buildCatalogQueryString,
  describeCatalogFilters,
  type CatalogFilters,
} from "../catalogFilters.js";
import {
  catalogCacheLabel,
  getCatalogSnapshot,
  getPreviewSnapshot,
  recordPreviewSnapshot,
  revalidateCatalogCache,
  type CatalogFallbackReason,
} from "../catalogCache.js";
import { currentCatalogCacheScope } from "../catalogCacheScope.js";
import { isRetryableStatus } from "../retry.js";
import {
  mapHttpError,
  mapTransportError,
  mappedErrorOf,
  mcpError,
  throwHttpError,
} from "../errorMapping.js";
import { cacheStalenessNotice } from "../cacheStaleness.js";
import { truncateResponse } from "../truncation.js";
import { applyPreviewLimits, serializePreview } from "../previewLimits.js";
import { formatResource } from "./registry.js";

interface CatalogLoad {
  items: any[];
  notice: string | null;
  fromCache: boolean;
}

const catalogLoads = new Map<string, Promise<CatalogLoad>>();
const previewLoads = new Map<string, Promise<{ r: any; label: string | null }>>();

/**
 * Whether a failed catalog read should fall back to the offline snapshot (#837).
 *
 * Only a catalog that could not answer: a transport failure, or a transient
 * server/throttling status that survived the retry layer's replays. A 4xx is
 * the service answering about this request, and must reach the agent instead of
 * being papered over with stale data labelled "API unreachable".
 */
function catalogFallbackFor(err: unknown): CatalogFallbackReason | null {
  const mapped = mappedErrorOf(err);
  if (!mapped) return { kind: "unreachable" };
  if (mapped.status === undefined) {
    return mapped.category === "network" || mapped.category === "timeout"
      ? { kind: "unreachable" }
      : null;
  }
  return isRetryableStatus(mapped.status) ? { kind: "status", status: mapped.status } : null;
}

async function fetchCatalog(url: string, operation: string, scope: string): Promise<CatalogLoad> {
  const result = await revalidateCatalogCache({
    fetcher: () => jsonFetchOnce(url),
    policy: RETRY_POLICY,
    label: `GET ${new URL(url).pathname}`,
    scope,
    onRetry: logRetry,
  });

  if (result.revalidated) {
    return {
      items: result.resources,
      notice: cacheStalenessNotice(result.response.headers),
      fromCache: false,
    };
  }

  if (result.error !== undefined) {
    const source = sourceForUrl(url);
    const err = mcpError(
      mapTransportError({ operation: SERVICE_OPERATION[source], source, error: result.error }),
    );
    const reason = catalogFallbackFor(err);
    const snapshot = reason ? getCatalogSnapshot(Date.now(), scope) : null;
    if (!snapshot || !reason) throw err;
    return {
      items: Array.isArray(snapshot.resources) ? (snapshot.resources as any[]) : [],
      notice: catalogCacheLabel(snapshot.savedAtMs, Date.now(), reason),
      fromCache: true,
    };
  }

  const res = result.response!;
  const err = mcpError(
    mapHttpError({ operation, source: "api", status: res.status, data: res.data }),
  );
  if (result.snapshot && result.reason) {
    return {
      items: Array.isArray(result.snapshot.resources) ? (result.snapshot.resources as any[]) : [],
      notice: catalogCacheLabel(result.snapshot.savedAtMs, Date.now(), result.reason),
      fromCache: true,
    };
  }
  throw err;
}

function loadCatalog(url: string, operation: string): Promise<CatalogLoad> {
  const scope = currentCatalogCacheScope();
  const key = `${scope}\u0000${url}`;
  const pending = catalogLoads.get(key);
  if (pending) return pending;

  const request = fetchCatalog(url, operation, scope);
  catalogLoads.set(key, request);
  void request.then(
    () => {
      if (catalogLoads.get(key) === request) catalogLoads.delete(key);
    },
    () => {
      if (catalogLoads.get(key) === request) catalogLoads.delete(key);
    },
  );
  return request;
}

export async function browse(filters: CatalogFilters = {}): Promise<string> {
  const qs = buildCatalogQueryString(filters);
  const url = qs ? `${BASE_URL}/resources?${qs}` : `${BASE_URL}/resources`;
  const { items: raw, notice } = await loadCatalog(url, "Browse failed");
  const items = applyCatalogSort(applyClientCatalogFilters(raw, filters), filters.sort);
  const body =
    items.length === 0
      ? filters.query ||
        filters.minPrice ||
        filters.maxPrice ||
        filters.verificationStatus ||
        filters.resourceType ||
        filters.owner ||
        filters.tags?.length ||
        filters.listed !== undefined
        ? `No resources match ${describeCatalogFilters(filters)}.`
        : "No resources listed yet."
      : items.map(formatResource).join("\n\n");
  const full = notice ? `${body}\n\n${notice}` : body;
  return truncateResponse(full);
}

export async function search(filtersOrQuery: string | CatalogFilters): Promise<string> {
  const filters: CatalogFilters =
    typeof filtersOrQuery === "string" ? { query: filtersOrQuery } : filtersOrQuery;

  const hasCriteria = Boolean(
    filters.query?.trim() ||
    filters.minPrice ||
    filters.maxPrice ||
    filters.verificationStatus ||
    filters.resourceType ||
    filters.owner ||
    filters.sort ||
    filters.limit !== undefined ||
    filters.offset !== undefined ||
    (filters.tags && filters.tags.length > 0) ||
    filters.listed !== undefined,
  );
  if (!hasCriteria) return "Provide a search query or at least one catalog filter.";

  const qs = buildCatalogQueryString(filters);
  const url = qs ? `${BASE_URL}/resources?${qs}` : `${BASE_URL}/resources`;
  const { items: raw, notice, fromCache } = await loadCatalog(url, "Search failed");
  const items = applyCatalogSort(applyClientCatalogFilters(raw, filters), filters.sort);

  if (items.length === 0) return `No resources match ${describeCatalogFilters(filters)}.`;
  const body = items.map(formatResource).join("\n\n");
  const full = fromCache && notice ? `${body}\n\n${notice}` : body;
  return truncateResponse(full);
}

async function fetchPreviewData(
  resourceId: string,
  scope: string,
): Promise<{ r: any; label: string | null }> {
  try {
    const res = await jsonFetch(`${BASE_URL}/resources/${resourceId}/meta`);
    if (!res.ok)
      throwHttpError({
        operation: "Preview failed",
        source: "api",
        status: res.status,
        data: res.data,
      });
    recordPreviewSnapshot(resourceId, res.data, scope);
    return { r: res.data, label: null };
  } catch (err) {
    const reason = catalogFallbackFor(err);
    const snap = reason ? getPreviewSnapshot(resourceId, Date.now(), scope) : null;
    if (!snap || !reason) throw err;
    return { r: snap.meta as any, label: catalogCacheLabel(snap.savedAtMs, Date.now(), reason) };
  }
}

function previewData(resourceId: string): Promise<{ r: any; label: string | null }> {
  const scope = currentCatalogCacheScope();
  const key = `${scope}\u0000${resourceId}`;
  const pending = previewLoads.get(key);
  if (pending) return pending;

  const request = fetchPreviewData(resourceId, scope);
  previewLoads.set(key, request);
  void request.then(
    () => {
      if (previewLoads.get(key) === request) previewLoads.delete(key);
    },
    () => {
      if (previewLoads.get(key) === request) previewLoads.delete(key);
    },
  );
  return request;
}

export async function preview(resourceId: string): Promise<string> {
  const { r, label } = await previewData(resourceId);
  const out: Record<string, unknown> = {
    id: r.id,
    title: r.title,
    description: r.description,
    price: `$${r.price} USDC`,
    type: r.resourceType,
    verificationStatus: r.verificationStatus,
    accessUrl: r.accessUrl,
  };
  if (label) out.offlineCache = label;
  // Publisher-supplied title/description are unbounded at the source, so cap
  // them before serializing rather than truncating the JSON afterwards (#582).
  return serializePreview(applyPreviewLimits(out));
}
