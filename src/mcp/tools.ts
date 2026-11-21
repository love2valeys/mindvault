import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { MindVaultClient } from "../client.js";

export function registerTools(server: McpServer, client: MindVaultClient): void {
  server.tool(
    "mindvault_list_notes",
    "List notes owned by the connected account",
    {
      limit: z.number().int().positive().max(100).optional(),
      cursor: z.string().optional(),
    },
    async ({ limit, cursor }) => {
      const result = await client.listNotes({ limit, cursor });
      return {
        content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
      };
    },
  );

  server.tool(
    "mindvault_get_note",
    "Fetch a single note by id",
    {
      noteId: z.string().min(1),
    },
    async ({ noteId }) => {
      const result = await client.getNote(noteId);
      return {
        content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
      };
    },
  );

  server.tool(
    "mindvault_fee_config",
    "Read the platform and royalty fee configuration so agents can quote the correct creator payout before setting a price",
    {},
    async () => {
      const result = await client.getFeeConfig();
      return {
        content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
      };
    },
  );
}
