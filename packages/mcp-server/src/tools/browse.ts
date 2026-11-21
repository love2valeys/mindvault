import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import type { MindVaultClient } from "../client.js";
import { formatError, formatResult } from "../format.js";

export function registerBrowseTool(server: McpServer, client: MindVaultClient): void {
  server.tool(
    "mindvault_browse",
    "Browse catalog entries, optionally filtered by tags. Returns matched tags for each entry.",
    {
      tags: z
        .array(z.string())
        .optional()
        .describe("Filter entries to those matching any of the given tags"),
      limit: z.number().int().positive().max(100).optional().describe("Maximum entries to return"),
      cursor: z.string().optional().describe("Pagination cursor from a previous browse call"),
    },
    async ({ tags, limit, cursor }) => {
      try {
        const result = await client.browse({ tags, limit, cursor });
        const normalizedTags = (tags ?? []).map((tag) => tag.trim().toLowerCase()).filter(Boolean);
        const entries = result.entries.map((entry) => {
          const entryTags = entry.tags ?? [];
          const matchedTags =
            normalizedTags.length > 0
              ? entryTags.filter((tag) => normalizedTags.includes(tag.trim().toLowerCase()))
              : entryTags;
          return { ...entry, matchedTags };
        });
        return formatResult({ ...result, entries, tags: normalizedTags });
      } catch (error) {
        return formatError(error);
      }
    },
  );
}
