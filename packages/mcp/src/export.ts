export type ReceiptRecord = Record<string, unknown>;

const FORMULA_PREFIX = /^[=+\-@\t\r]/;

function stringifyCell(value: unknown): string {
  if (value === null || value === undefined) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value);
}

function escapeJsonlValue(value: unknown): string {
  // JSON.stringify already escapes CR/LF, quotes, and preserves Unicode.
  return JSON.stringify(value === undefined ? null : value);
}

function neutralizeFormula(value: string): string {
  if (FORMULA_PREFIX.test(value)) {
    return `'${value}`;
  }
  return value;
}

function escapeCsvField(value: unknown): string {
  const raw = neutralizeFormula(stringifyCell(value));
  if (/[",\r\n]/.test(raw)) {
    return `"${raw.replace(/"/g, '""')}"`;
  }
  return raw;
}

/**
 * Serialize receipts as JSONL. Each receipt is emitted on exactly one line;
 * embedded CR/LF, quotes, and Unicode are escaped by JSON.stringify so a
 * single receipt can never inject additional rows.
 */
export function toJsonl(receipts: ReceiptRecord[]): string {
  return receipts
    .map((receipt) => {
      const entries = Object.entries(receipt).map(
        ([key, value]) => `${JSON.stringify(key)}:${escapeJsonlValue(value)}`,
      );
      return `{${entries.join(",")}}`;
    })
    .join("\n");
}

/**
 * Serialize receipts as CSV. Fields containing commas, quotes, or line breaks
 * are quoted and escaped; formula-like values are neutralized with a leading
 * apostrophe so spreadsheet consumers do not evaluate them.
 */
export function toCsv(receipts: ReceiptRecord[], columns?: string[]): string {
  const headers =
    columns ??
    Array.from(
      receipts.reduce<Set<string>>((set, receipt) => {
        for (const key of Object.keys(receipt)) {
          set.add(key);
        }
        return set;
      }, new Set<string>()),
    );

  const lines = [headers.map((header) => escapeCsvField(header)).join(",")];
  for (const receipt of receipts) {
    lines.push(
      headers.map((header) => escapeCsvField(receipt[header])).join(","),
    );
  }
  return lines.join("\r\n");
}
