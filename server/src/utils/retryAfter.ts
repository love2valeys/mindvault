/**
 * Issue #117: Consistent Retry-After parsing in seconds and HTTP-date formats with deadline caps.
 */

export interface RetryAfterOptions {
  /** Maximum wait duration allowed in milliseconds (default: 60,000ms / 60s) */
  maxWaitMs?: number;
  /** Current timestamp reference for testing (default: Date.now()) */
  nowMs?: number;
}

/**
 * Parses Retry-After header values adhering to RFC 7231 Section 7.1.3:
 * - Integer seconds representation (e.g. "120")
 * - HTTP-date representation (e.g. "Wed, 21 Oct 2026 07:28:00 GMT")
 *
 * Enforces bounded sleep guarantees:
 * - Negative or past dates resolve to 0ms
 * - Values exceeding maxWaitMs are strictly clamped to avoid unbounded execution
 * - Invalid or unparseable formats safely resolve to 0ms
 */
export function parseRetryAfter(
  headerValue: string | number | null | undefined,
  options: RetryAfterOptions = {},
): number {
  const maxWaitMs = options.maxWaitMs ?? 60_000;
  const nowMs = options.nowMs ?? Date.now();

  if (headerValue === null || headerValue === undefined) {
    return 0;
  }

  if (typeof headerValue === 'number') {
    if (!Number.isFinite(headerValue) || headerValue <= 0) {
      return 0;
    }
    const ms = Math.floor(headerValue * 1000);
    return Math.min(ms, maxWaitMs);
  }

  const trimmed = headerValue.trim();
  if (trimmed.length === 0) {
    return 0;
  }

  // 1. Integer seconds format (e.g. "120")
  if (/^\d+$/.test(trimmed)) {
    const seconds = Number.parseInt(trimmed, 10);
    if (!Number.isFinite(seconds) || seconds <= 0) {
      return 0;
    }
    const ms = seconds * 1000;
    return Math.min(ms, maxWaitMs);
  }

  // 2. HTTP-date format (e.g. "Wed, 21 Oct 2026 07:28:00 GMT")
  const parsedDate = Date.parse(trimmed);
  if (!Number.isNaN(parsedDate)) {
    const diffMs = parsedDate - nowMs;
    if (diffMs <= 0) {
      return 0; // Target time is in the past
    }
    return Math.min(diffMs, maxWaitMs);
  }

  return 0;
}