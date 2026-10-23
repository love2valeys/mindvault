import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { TagStore } from "../store/tags.js";

/**
 * Popularity counters saturate at Number.MAX_SAFE_INTEGER instead of wrapping.
 * A saturated counter is represented as MAX_SAFE_INTEGER and never decreases
 * due to overflow; index repair preserves this saturation semantics.
 */
export const TAG_POPULARITY_MAX = Number.MAX_SAFE_INTEGER;

/** Saturating increment: never wraps to a smaller value at the integer boundary. */
export function saturatingIncrement(current: number, delta = 1): number {
  if (!Number.isFinite(current) || current < 0) {
    current = 0;
  }
  if (!Number.isFinite(delta) || delta <= 0) {
    return current;
  }
  if (current >= TAG_POPULARITY_MAX - delta) {
    return TAG_POPULARITY_MAX;
  }
  return current + delta;
}

export function registerTagTools(server: McpServer, store: TagStore): void {
  server.tool(
    "tags_list",
    "List tags with their popularity counters.",
    {},
    async () => {
      const tags = await store.list();
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ tags }),
          },
        ],
      };
    },
  );

  server.tool(
    "tags_increment",
    "Increment a tag's popularity counter. Counters saturate at the maximum safe integer and never wrap.",
    {
      name: z.string().min(1),
      delta: z.number().int().positive().optional(),
    },
    async ({ name, delta }) => {
      const tag = await store.get(name);
      if (!tag) {
        return {
          content: [{ type: "text", text: `Tag not found: ${name}` }],
          isError: true,
        };
      }
      const next = saturatingIncrement(tag.popularity, delta ?? 1);
      const updated = await store.update(name, { popularity: next });
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ tag: updated }),
          },
        ],
      };
    },
  );
}
