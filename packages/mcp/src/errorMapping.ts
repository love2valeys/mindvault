export type ErrorCategory =
  | 'network'
  | 'horizon'
  | 'rpc'
  | 'validation'
  | 'auth'
  | 'unknown';

export interface MappedError {
  source: string;
  category: ErrorCategory;
  message: string;
  line: string;
}

const HORIZON_5XX = /\bhorizon\b[^\n]*\b5\d\d\b/i;
const HORIZON_STATUS = /\bhorizon\b/i;
const RPC_TIMEOUT = /\b(soroban\s*rpc|rpc)\b[^\n]*\b(timeout|timed out|etimedout)\b/i;
const RPC_GENERIC = /\b(soroban\s*rpc|rpc)\b/i;
const TIMEOUT = /\b(timeout|timed out|etimedout)\b/i;
const NETWORK = /\b(econnrefused|enotfound|econnreset|network|fetch failed|socket hang up)\b/i;
const AUTH = /\b(unauthorized|forbidden|401|403)\b/i;
const VALIDATION = /\b(invalid|validation|bad request|400|422)\b/i;

function detectSource(input: string): string {
  if (HORIZON_STATUS.test(input)) return 'Horizon';
  if (RPC_GENERIC.test(input)) return 'SorobanRPC';
  return 'Unknown';
}

function detectCategory(input: string): ErrorCategory {
  if (HORIZON_5XX.test(input)) return 'horizon';
  if (RPC_TIMEOUT.test(input)) return 'rpc';
  if (AUTH.test(input)) return 'auth';
  if (VALIDATION.test(input)) return 'validation';
  if (TIMEOUT.test(input) || NETWORK.test(input)) return 'network';
  return 'unknown';
}

export function errorMapping(error: unknown): MappedError {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === 'string'
        ? error
        : JSON.stringify(error);

  const source = detectSource(message);
  const category = detectCategory(message);

  return {
    source,
    category,
    message,
    line: `${source}·${category}`,
  };
}
