/**
 * MCP tool result helpers (#553).
 *
 * Handlers still produce a human-readable text block. Tools that advertise an
 * `outputSchema` also return the same data as `structuredContent` so an agent
 * can read an id, price, or tx hash without parsing prose.
 *
 * Failures stay on the CallTool `catch` path (`isError: true`, no payload).
 */

import { Ajv, type AnySchema, type ValidateFunction } from "ajv";
import { logger } from "./logger.js";

export type ToolOutcome = string | { text: string; structured?: Record<string, unknown> };

const ajv = new Ajv({ allErrors: true, allowUnionTypes: true, strict: false });
const compiledSchemas = new WeakMap<object, ValidateFunction>();

export interface CallToolSuccess {
  [key: string]: unknown;
  content: [{ type: "text"; text: string }];
  structuredContent?: Record<string, unknown>;
}

/** Parse handler text as a JSON object. Arrays and scalars are ignored. */
export function parseStructuredObject(text: string): Record<string, unknown> | undefined {
  try {
    const parsed = JSON.parse(text);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Fallback payload when a tool declares `outputSchema` but the handler text is
 * not JSON (insufficient funds, verification rejected, missing tx hash, …).
 * The text block is unchanged; agents still get a stable object.
 */
export function textFallback(text: string): Record<string, unknown> {
  return { status: "text", message: text };
}

function validatorFor(schema: object): ValidateFunction {
  const cached = compiledSchemas.get(schema);
  if (cached) return cached;
  const validator = ajv.compile(schema as AnySchema);
  compiledSchemas.set(schema, validator);
  return validator;
}

/** Return schema paths only; validation diagnostics must never include payload values. */
export function outputValidationErrors(schema: object, value: unknown): string[] {
  try {
    const validator = validatorFor(schema);
    if (validator(value)) return [];
    return (validator.errors ?? []).map(
      (error) => `${error.instancePath || "/"} (${error.keyword})`,
    );
  } catch {
    return ["schema compilation failed"];
  }
}

function invalidOutputResult(): CallToolSuccess {
  return {
    content: [{ type: "text", text: "Internal tool output failed schema validation." }],
    isError: true,
  };
}

export function outcomeText(outcome: ToolOutcome): string {
  return typeof outcome === "string" ? outcome : outcome.text;
}

/**
 * Attach `structuredContent` when a sidecar is provided, when the text is a
 * JSON object, or (for tools with an advertised schema) as a text fallback.
 * Tools without a schema stay text-only.
 */
export function normalizeToolResult(
  name: string,
  outcome: ToolOutcome,
  outputSchemaFor: (tool: string) => Record<string, unknown> | undefined,
): CallToolSuccess {
  const text = outcomeText(outcome);
  const explicit = typeof outcome === "string" ? undefined : outcome.structured;
  const parsed = explicit ?? parseStructuredObject(text);
  const schema = outputSchemaFor(name);
  const structured = parsed ?? (schema ? textFallback(text) : undefined);

  if (schema && structured) {
    const errors = outputValidationErrors(schema, structured);
    if (errors.length > 0) {
      logger.warn("MCP output contract mismatch for %s (%s)", name, errors.join(", "));
      return invalidOutputResult();
    }
  }

  return structured
    ? { content: [{ type: "text", text }], structuredContent: structured }
    : { content: [{ type: "text", text }] };
}
