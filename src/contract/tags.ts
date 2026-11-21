/**
 * Tag popularity counter contract.
 *
 * Popularity counters are unsigned 32-bit integers. Increments are
 * *saturating*: once a counter reaches `TAG_POPULARITY_MAX` it stays there
 * instead of wrapping back to a smaller value. A saturated counter is
 * represented by the value `TAG_POPULARITY_MAX` itself (no sentinel, no
 * `null`), so existing clients keep reading a plain number and the response
 * shape is unchanged.
 */

export const TAG_POPULARITY_MAX = 0xffffffff;

export interface TagPopularity {
  tag: string;
  count: number;
}

/**
 * Saturating increment for a single popularity counter.
 *
 * Never returns a value smaller than `current` for non-negative inputs and
 * clamps at `TAG_POPULARITY_MAX`.
 */
export function incrementPopularity(current: number, delta = 1): number {
  const base = normalizeCount(current);
  const step = normalizeCount(delta);
  if (base >= TAG_POPULARITY_MAX) {
    return TAG_POPULARITY_MAX;
  }
  if (step >= TAG_POPULARITY_MAX - base) {
    return TAG_POPULARITY_MAX;
  }
  return base + step;
}

/**
 * Clamp an arbitrary stored count into the valid popularity range.
 * Used by index repair so repaired counters preserve saturation semantics.
 */
export function normalizeCount(value: number): number {
  if (!Number.isFinite(value) || value <= 0) {
    return 0;
  }
  if (value >= TAG_POPULARITY_MAX) {
    return TAG_POPULARITY_MAX;
  }
  return Math.floor(value);
}

/**
 * Apply a batch of increments to a popularity index, saturating each counter.
 */
export function applyPopularityUpdates(
  index: ReadonlyMap<string, number>,
  updates: ReadonlyMap<string, number>,
): Map<string, number> {
  const next = new Map<string, number>();
  for (const [tag, count] of index) {
    next.set(tag, normalizeCount(count));
  }
  for (const [tag, delta] of updates) {
    next.set(tag, incrementPopularity(next.get(tag) ?? 0, delta));
  }
  return next;
}

/**
 * Repair a popularity index: drop invalid entries and clamp saturated ones.
 * Saturated counters remain represented as `TAG_POPULARITY_MAX`.
 */
export function repairPopularityIndex(
  index: ReadonlyMap<string, number>,
): Map<string, number> {
  const repaired = new Map<string, number>();
  for (const [tag, count] of index) {
    if (typeof tag !== "string" || tag.length === 0) {
      continue;
    }
    repaired.set(tag, normalizeCount(count));
  }
  return repaired;
}
