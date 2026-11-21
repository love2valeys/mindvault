import { z } from "zod";

export const TAG_INDEX_MAX_IDS = 500;

export const tagIndexEntrySchema = z.object({
  id: z.string().min(1),
  tags: z.array(z.string().min(1)),
});

export type TagIndexEntry = z.infer<typeof tagIndexEntrySchema>;

export const tagIndexIntegrityRequestSchema = z.object({
  entries: z
    .array(tagIndexEntrySchema)
    .max(TAG_INDEX_MAX_IDS, `at most ${TAG_INDEX_MAX_IDS} ids may be checked`),
});

export type TagIndexIntegrityRequest = z.infer<
  typeof tagIndexIntegrityRequestSchema
>;

export const tagIndexIntegrityReportSchema = z.object({
  checked: z.number().int().nonnegative(),
  missing: z.array(z.string()),
  stale: z.array(z.string()),
});

export type TagIndexIntegrityReport = z.infer<
  typeof tagIndexIntegrityReportSchema
>;

/**
 * Read-only consistency check comparing supplied resource tags with the
 * tag-index membership for a bounded set of ids.
 *
 * - `missing`: ids whose resource carries tags that the index does not list.
 * - `stale`: ids whose index membership lists tags the resource no longer has.
 *
 * The check never mutates the index; it only reports discrepancies so that
 * operational repair tooling can act on them.
 */
export function buildTagIndexIntegrityReport(
  request: TagIndexIntegrityRequest,
  indexMembership: ReadonlyMap<string, readonly string[]>,
): TagIndexIntegrityReport {
  const missing: string[] = [];
  const stale: string[] = [];

  for (const entry of request.entries) {
    const resourceTags = new Set(entry.tags);
    const indexedTags = new Set(indexMembership.get(entry.id) ?? []);

    let hasMissing = false;
    for (const tag of resourceTags) {
      if (!indexedTags.has(tag)) {
        hasMissing = true;
        break;
      }
    }

    let hasStale = false;
    for (const tag of indexedTags) {
      if (!resourceTags.has(tag)) {
        hasStale = true;
        break;
      }
    }

    if (hasMissing) missing.push(entry.id);
    if (hasStale) stale.push(entry.id);
  }

  return {
    checked: request.entries.length,
    missing,
    stale,
  };
}
