import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { getTagIndex, getResourceTags } from "../../contract/tags.js";

const MAX_IDS = 200;

const integrityReportInputSchema = {
  ids: z
    .array(z.string().min(1))
    .min(1)
    .max(MAX_IDS)
    .describe(
      `Bounded set of resource IDs to check (max ${MAX_IDS}). The check is read-only.`,
    ),
};

export interface TagIndexIntegrityReport {
  checked: number;
  missing: string[];
  stale: string[];
}

/**
 * Read-only consistency check comparing supplied resource tags with tag-index
 * membership for a bounded ID set.
 *
 * - `missing`: IDs whose resource has tags but no tag-index membership.
 * - `stale`: IDs whose tag-index membership exists but the resource has no tags.
 */
export async function buildTagIndexIntegrityReport(
  ids: string[],
): Promise<TagIndexIntegrityReport> {
  const bounded = Array.from(new Set(ids)).slice(0, MAX_IDS);
  const missing: string[] = [];
  const stale: string[] = [];

  for (const id of bounded) {
    const resourceTags = await getResourceTags(id);
    const indexed = await getTagIndex(id);

    const hasResourceTags = resourceTags.length > 0;
    const hasIndexMembership = indexed.length > 0;

    if (hasResourceTags && !hasIndexMembership) {
      missing.push(id);
    } else if (!hasResourceTags && hasIndexMembership) {
      stale.push(id);
    }
  }

  return { checked: bounded.length, missing, stale };
}

export function registerTagIndexTools(server: McpServer): void {
  server.tool(
    "tag_index_integrity_report",
    "Read-only consistency check comparing resource tags with tag-index membership for a bounded ID set. Reports missing and stale memberships separately.",
    integrityReportInputSchema,
    async ({ ids }) => {
      const report = await buildTagIndexIntegrityReport(ids);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(report, null, 2),
          },
        ],
      };
    },
  );
}
