import {
  ErrorCode,
  McpError,
  type Resource,
  type TextResourceContents,
} from "@modelcontextprotocol/sdk/types.js";

export const SERVER_METADATA_URI = "mindvault://server/metadata";
export const MCP_TOOL_CAPABILITY_VERSION = 1;

export interface ServerMetadata {
  activeNetwork: string;
  registryId: string | null;
  toolCapabilityVersion: number;
  status: "running";
}

export function serverMetadataResource(): Resource {
  return {
    uri: SERVER_METADATA_URI,
    name: "MindVault Server Metadata",
    description: "Current network, registry contract, tool capability version, and process status.",
    mimeType: "application/json",
  };
}

export function serverMetadataContents(metadata: ServerMetadata): TextResourceContents {
  return {
    uri: SERVER_METADATA_URI,
    mimeType: "application/json",
    text: JSON.stringify(metadata, null, 2),
  };
}

export function readServerMetadata(uri: unknown, metadata: ServerMetadata): TextResourceContents {
  if (uri !== SERVER_METADATA_URI) {
    throw new McpError(ErrorCode.InvalidParams, `Unknown server metadata URI: ${SERVER_METADATA_URI}`);
  }
  return serverMetadataContents(metadata);
}