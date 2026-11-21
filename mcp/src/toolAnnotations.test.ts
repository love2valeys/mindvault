import { describe, expect, it } from "vitest";
import { TOOL_DEFINITIONS } from "./tools.js";

describe("MCP tool annotations for destructive and paid tools (#185)", () => {
  const paidOrDestructiveTools = [
    "mindvault_buy",
    "mindvault_buy_lease",
    "mindvault_publish",
    "mindvault_reset",
    "mindvault_freeze",
    "mindvault_transfer_ownership",
  ];

  it("ensures paid and destructive tools carry explicit annotations", () => {
    for (const toolName of paidOrDestructiveTools) {
      const tool = TOOL_DEFINITIONS.find((t) => t.name === toolName);
      if (tool) {
        expect(tool.annotations).toBeDefined();
        expect(typeof tool.annotations.title).toBe("string");
        expect(tool.annotations.readOnlyHint).toBe(false);
      }
    }
  });
});
