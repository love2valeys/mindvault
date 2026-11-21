import { describe, expect, it } from "vitest";
import { emitProfileClientConfig } from "./serverFactory.js";

describe("client config generation from serverFactory (#179)", () => {
  it("emits Claude Desktop copy-paste client config", () => {
    const config = emitProfileClientConfig("publisher", "claude-desktop") as any;
    expect(config.mcpServers).toBeDefined();
    expect(config.mcpServers.mindvault).toBeDefined();
    expect(config.mcpServers.mindvault.env.MINDVAULT_PROFILE).toBe("publisher");
  });

  it("emits Cursor/VS Code client config", () => {
    const config = emitProfileClientConfig("buyer", "cursor") as any;
    expect(config.name).toBe("mindvault");
    expect(config.env.MINDVAULT_PROFILE).toBe("buyer");
  });
});
