import { z } from "zod";

export const MAX_REPAIR_IDS = 1000;

export const RepairTagIndexInput = z.object({
  tagIds: z.array(z.string().min(1)).max(MAX_REPAIR_IDS),
});

export type RepairTagIndexInput = z.infer<typeof RepairTagIndexInput>;

export interface TagIndexEntry {
  tagId: string;
  popularity: number;
}

export interface TagIndexStore {
  get(tagId: string): TagIndexEntry | undefined;
  set(entry: TagIndexEntry): void;
}

/**
 * Normalize input IDs: dedupe and sort so that repeated or reordered
 * repair calls converge to the same deterministic index.
 */
export function normalizeTagIds(tagIds: readonly string[]): string[] {
  return Array.from(new Set(tagIds)).sort();
}

/**
 * Repair the tag index for the given IDs.
 *
 * - Rejects oversized input before any writes.
 * - Deduplicates and sorts IDs so repeated/reordered calls are idempotent.
 * - Does not inflate popularity on repeated repair calls.
 */
export function repairTagIndex(
  input: RepairTagIndexInput,
  store: TagIndexStore,
): TagIndexEntry[] {
  const parsed = RepairTagIndexInput.parse(input);
  const ids = normalizeTagIds(parsed.tagIds);

  const result: TagIndexEntry[] = [];
  for (const tagId of ids) {
    const existing = store.get(tagId);
    const entry: TagIndexEntry = existing
      ? { tagId, popularity: existing.popularity }
      : { tagId, popularity: 0 };
    store.set(entry);
    result.push(entry);
  }

  return result;
}
