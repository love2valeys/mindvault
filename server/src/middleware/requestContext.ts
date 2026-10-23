import { randomBytes } from "node:crypto";
import { createId } from "@paralleldrive/cuid2";
import type { NextFunction, Request, Response } from "express";
import { getLogger, runWithRequestContext } from "../lib/logger.js";

const REQUEST_ID_HEADER = "x-request-id";
const CORRELATION_ID_HEADER = "x-correlation-id";

let processSequence = 0;

/**
 * Issue #120: Generate collision-resistant correlation IDs across worker processes.
 * Incorporates process PID, monotonic sequence counter, timestamp, and cryptographic randomness.
 */
export function generateCorrelationId(): string {
  const pidHex = (process.pid & 0xffff).toString(16).padStart(4, "0");
  const seqHex = ((++processSequence) & 0xffff).toString(16).padStart(4, "0");
  const randHex = randomBytes(4).toString("hex");
  const cuid = createId();
  return `cid_${pidHex}${seqHex}_${randHex}_${cuid}`;
}

function readIncomingRequestId(req: Request): string | undefined {
  const raw = req.headers[CORRELATION_ID_HEADER] ?? req.headers[REQUEST_ID_HEADER];
  const value = Array.isArray(raw) ? raw[0] : raw;
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

export function requestContextMiddleware(req: Request, res: Response, next: NextFunction): void {
  const correlationId = readIncomingRequestId(req) ?? generateCorrelationId();
  res.setHeader(REQUEST_ID_HEADER, correlationId);
  res.setHeader(CORRELATION_ID_HEADER, correlationId);

  runWithRequestContext(correlationId, () => {
    const log = getLogger();
    const start = Date.now();

    res.on("finish", () => {
      log.info(
        {
          event: "http_request",
          correlationId,
          method: req.method,
          path: req.originalUrl || req.url,
          statusCode: res.statusCode,
          durationMs: Date.now() - start,
        },
        "request completed",
      );
    });

    next();
  });
}