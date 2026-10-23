# MCP Contracts

This document describes the contracts exposed by the MCP server, including the
shape of exported receipt files.

## Receipt exports

Receipts can be exported as JSONL or CSV. Both formats must preserve valid
UTF-8 content (including non-ASCII/Unicode characters) while escaping line
breaks so that a single receipt cannot inject extra rows into the export.

### JSONL

- Each receipt is serialized as exactly one line of JSON.
- String fields are JSON-escaped, so embedded line breaks (`\n`, `\r\n`),
  quotes (`\"`), and backslashes are encoded rather than emitted literally.
- Unicode characters are preserved as valid UTF-8 (they may be emitted directly
  or as `\uXXXX` escapes; both decode to the original value).
- Parsing the export line-by-line and decoding each line round-trips the
  original receipt data, and the number of parsed records equals the number of
  exported receipts.

### CSV

- Fields containing commas, double quotes, CRLF/LF line breaks, or leading
  whitespace are wrapped in double quotes, with embedded double quotes escaped
  by doubling (`""`).
- Unicode content is preserved as valid UTF-8.
- Formula-like values (fields beginning with `=`, `+`, `-`, or `@`) are
  neutralized (for example by prefixing a single quote or tab) so spreadsheet
  consumers do not interpret them as formulas. The neutralization is applied
  consistently so parsed exports retain the original data safely.
- A single receipt maps to a single logical CSV record; embedded line breaks
  remain inside quoted fields and do not produce extra rows.

### Fixtures and tests

Focused tests cover fixtures containing Unicode, CRLF and LF line breaks,
embedded quotes, and formula-like values. Tests assert that parsed exports
retain the original Unicode data and escaped line breaks without producing
additional rows.

### Compatibility

These guarantees are additive: existing clients that read JSONL or CSV exports
continue to work, since escaping only affects values that would otherwise be
ambiguous or unsafe.
