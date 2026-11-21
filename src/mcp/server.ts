import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { createPublicClient, http, formatUnits } from "viem";
import { base } from "viem/chains";
import { MINDFUL_VAULT_ABI, MINDFUL_VAULT_ADDRESS } from "../lib/contracts.js";

const client = createPublicClient({
  chain: base,
  transport: http(process.env.BASE_RPC_URL),
});

const server = new McpServer({
  name: "mindvault",
  version: "1.0.0",
});

server.tool(
  "mindvault_get_vault",
  "Read a MindVault vault's on-chain state by token id",
  {
    tokenId: z.string().describe("The vault token id"),
  },
  async ({ tokenId }) => {
    const [owner, price, unlocked] = await client.readContract({
      address: MINDFUL_VAULT_ADDRESS,
      abi: MINDFUL_VAULT_ABI,
      functionName: "getVault",
      args: [BigInt(tokenId)],
    });

    return {
      content: [
        {
          type: "text",
          text: JSON.stringify(
            {
              tokenId,
              owner,
              price: formatUnits(price, 18),
              unlocked,
            },
            null,
            2,
          ),
        },
      ],
    };
  },
);

server.tool(
  "mindvault_fee_config",
  "Read the MindVault platform and royalty fee configuration used to quote creator payouts",
  {},
  async () => {
    const [platformFeeBps, royaltyFeeBps] = await client.readContract({
      address: MINDFUL_VAULT_ADDRESS,
      abi: MINDFUL_VAULT_ABI,
      functionName: "get_fee_config",
    });

    return {
      content: [
        {
          type: "text",
          text: JSON.stringify(
            {
              platformFeeBps: Number(platformFeeBps),
              royaltyFeeBps: Number(royaltyFeeBps),
              platformFeePercent: Number(platformFeeBps) / 100,
              royaltyFeePercent: Number(royaltyFeeBps) / 100,
            },
            null,
            2,
          ),
        },
      ],
    };
  },
);

const transport = new StdioServerTransport();
await server.connect(transport);
