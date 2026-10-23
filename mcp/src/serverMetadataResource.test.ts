import { describe, expect, it } from "vitest";
import { ErrorCode, McpError } from "@modelcontextprotocol/sdk/types.js";
import {
  MCP_TOOL_CAPABILITY_VERSION,
  SERVER_METADATA_URI,
  readServerMetadata,
  serverMetadataResource,
} from "./serverMetadataResource.js";

describe("server metadata resource", () => {
  const metadata = {
    activeNetwork: "testnet",
    registryId: "CABC123",
    toolCapabilityVersion: MCP_TOOL_CAPABILITY_VERSION,
    status: "running" as const,
  };

  it("advertises a stable read-only metadata URI", () => {
    expect(serverMetadataResource()).toMatchObject({
      uri: SERVER_METADATA_URI,
      name: "MindVault Server Metadata",
      mimeType: "application/json",
    });
  });

  it("reads current metadata without including wallet or secret fields", () => {
    const contents = readServerMetadata(SERVER_METADATA_URI, metadata);
    expect(JSON.parse(contents.text)).toEqual(metadata);
    expect(contents.text).not.toMatch(/wallet|secret|apiKey|private/i);
  });

  it("reflects profile changes when metadata is read again", () => {
    const first = JSON.parse(readServerMetadata(SERVER_METADATA_URI, metadata).text);
    const next = JSON.parse(
      readServerMetadata(SERVER_METADATA_URI, {
        ...metadata,
        activeNetwork: "mainnet",
        registryId: "CDEF456",
      }).text,
    );

    expect(first.activeNetwork).toBe("testnet");
    expect(next).toMatchObject({ activeNetwork: "mainnet", registryId: "CDEF456" });
  });

  it("rejects unknown URIs", () => {
    try {
      readServerMetadata("mindvault://server/other", metadata);
      throw new Error("expected URI rejection");
    } catch (error) {
      expect(error).toBeInstanceOf(McpError);
      expect((error as McpError).code).toBe(ErrorCode.InvalidParams);
    }
  });
});