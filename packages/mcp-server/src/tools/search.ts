import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { MindVaultClient } from "../client.js";
import { formatError, formatResult } from "../format.js";

const tagFilterSchema = z
  .union([z.string(), z.array(z.string())])
  .optional()
  .describe(
    "Optional tag or list of tags to filter results by. Matches entries that carry any of the given tags.",
  );

function normalizeTags(tags: string | string[] | undefined): string[] {
  if (!tags) return [];
  const list = Array.isArray(tags) ? tags : [tags];
  return list.map((t) => t.trim()).filter((t) => t.length > 0);
}

function matchesTags(entryTags: string[] | undefined, wanted: string[]): boolean {
  if (wanted.length === 0) return true;
  if (!entryTags || entryTags.length === 0) return false;
  const normalized = entryTags.map((t) => t.toLowerCase());
  return wanted.some((t) => normalized.includes(t.toLowerCase()));
}

function matchedTags(entryTags: string[] | undefined, wanted: string[]): string[] {
  if (!entryTags || entryTags.length === 0) return [];
  if (wanted.length === 0) return entryTags;
  const wantedLower = wanted.map((t) => t.toLowerCase());
  return entryTags.filter((t) => wantedLower.includes(t.toLowerCase()));
}

export function registerSearchTools(server: McpServer, client: MindVaultClient): void {
  server.tool(
    "mindvault_search",
    "Search the MindVault catalog by free-text query, optionally filtered by tags.",
    {
      query: z.string().describe("Free-text search query."),
      tags: tagFilterSchema,
      limit: z.number().int().positive().max(100).optional(),
    },
    async ({ query, tags, limit }) => {
      try {
        const wanted = normalizeTags(tags);
        const results = await client.search(query, { limit });
        const filtered = results.filter((entry) => matchesTags(entry.tags, wanted));
        const payload = filtered.map((entry) => ({
          ...entry,
          matchedTags: matchedTags(entry.tags, wanted),
        }));
        return formatResult(payload);
      } catch (error) {
        return formatError(error);
      }
    },
  );

  server.tool(
    "mindvault_browse",
    "Browse the MindVault catalog, optionally filtered by tags. Results include matchedTags.",
    {
      tags: tagFilterSchema,
      limit: z.number().int().positive().max(100).optional(),
      offset: z.number().int().nonnegative().optional(),
    },
    async ({ tags, limit, offset }) => {
      try {
        const wanted = normalizeTags(tags);
        const results = await client.browse({ limit, offset });
        const filtered = results.filter((entry) => matchesTags(entry.tags, wanted));
        const payload = filtered.map((entry) => ({
          ...entry,
          matchedTags: matchedTags(entry.tags, wanted),
        }));
        return formatResult(payload);
      } catch (error) {
        return formatError(error);
      }
    },
  );
}
