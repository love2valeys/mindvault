#!/usr/bin/env node
/**
 * Refresh or verify `docs/contract-storage-footprint.md` from the contract's
 * `storage_footprint_report` test output (#231).
 *
 * Usage:
 *   node scripts/refresh-storage-footprint.mjs --check   # CI mode: fail on growth or doc drift
 *   node scripts/refresh-storage-footprint.mjs --write   # Regenerate tables in place
 *
 * Optional flags (useful for offline inspection and unit tests):
 *   --doc <path>     Target Markdown file (default: docs/contract-storage-footprint.md)
 *   --input <path>   Read captured `storage_footprint_report` output instead of running cargo
 */

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_PER_RESOURCE_BUDGET = 1900;
const DEFAULT_PER_PAYMENT_BUDGET = 850;

const PER_RESOURCE_LABEL =
  "One max-size registration (`Resource` + `Index` + one `TagIndex` + one `TagCount`)";
const PER_PAYMENT_LABEL = "One payment (`PaymentReceipt` + `PaymentIndex`)";

/**
 * Parse the markdown table and aggregate summary lines printed by
 * `cargo test storage_footprint_report -- --nocapture`.
 */
export function parseFootprintTestOutput(stdout) {
  const lines = stdout.split(/\r?\n/);
  const entries = [];

  let inTable = false;
  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!inTable) {
      if (/^\|\s*Entry\s*\|\s*Map\s*\|\s*Key\s*\|\s*Value\s*\|\s*Total\s*\|\s*Budget\s*\|$/.test(line)) {
        inTable = true;
      }
      continue;
    }

    if (!line.startsWith("|")) {
      if (entries.length > 0) {
        break;
      }
      continue;
    }

    const cells = splitMarkdownRow(line);
    if (cells.length !== 6) {
      continue;
    }
    if (cells.every((cell) => /^:?-{2,}:?$/.test(cell))) {
      continue;
    }

    entries.push({
      entry: cells[0],
      map: cells[1],
      key: parseIntStrict(cells[2], "Key"),
      value: parseIntStrict(cells[3], "Value"),
      total: parseIntStrict(cells[4], "Total"),
      budget: parseIntStrict(cells[5], "Budget"),
    });
  }

  if (entries.length === 0) {
    throw new Error(
      "Failed to parse storage footprint table from `storage_footprint_report` output.",
    );
  }

  const perResourceMatch = stdout.match(
    /Per max-size registration \([^)]+\):\s*(\d+)\s+bytes(?:\s*\(budget:\s*(\d+)\s+bytes\))?/i,
  );
  const perPaymentMatch = stdout.match(
    /Per payment \([^)]+\):\s*(\d+)\s+bytes(?:\s*\(budget:\s*(\d+)\s+bytes\))?/i,
  );

  if (!perResourceMatch || !perPaymentMatch) {
    throw new Error(
      "Failed to parse aggregate storage footprint lines from `storage_footprint_report` output.",
    );
  }

  const aggregates = [
    {
      operation: PER_RESOURCE_LABEL,
      bytes: parseIntStrict(perResourceMatch[1], "Per max-size registration bytes"),
      budget: perResourceMatch[2]
        ? parseIntStrict(perResourceMatch[2], "Per max-size registration budget")
        : DEFAULT_PER_RESOURCE_BUDGET,
    },
    {
      operation: PER_PAYMENT_LABEL,
      bytes: parseIntStrict(perPaymentMatch[1], "Per payment bytes"),
      budget: perPaymentMatch[2]
        ? parseIntStrict(perPaymentMatch[2], "Per payment budget")
        : DEFAULT_PER_PAYMENT_BUDGET,
    },
  ];

  return { entries, aggregates };
}

/**
 * Parse the committed entry and aggregate tables from the
 * `## Current footprint` section of `docs/contract-storage-footprint.md`.
 */
export function parseDocumentedFootprint(markdown) {
  const section = extractCurrentFootprintSection(markdown);
  const tables = extractMarkdownTables(section);
  if (tables.length < 2) {
    throw new Error(
      "Expected both the entry table and aggregate table under `## Current footprint`.",
    );
  }

  const entries = tables[0].rows.map((cells) => {
    if (cells.length !== 6) {
      throw new Error(`Expected 6 columns in entry footprint table row, got ${cells.length}.`);
    }
    return {
      entry: cells[0],
      map: cells[1],
      key: parseIntStrict(cells[2], "Key"),
      value: parseIntStrict(cells[3], "Value"),
      total: parseIntStrict(cells[4], "Total"),
      budget: parseIntStrict(cells[5], "Budget"),
    };
  });

  const aggregates = tables[1].rows.map((cells) => {
    if (cells.length !== 3) {
      throw new Error(`Expected 3 columns in aggregate footprint table row, got ${cells.length}.`);
    }
    return {
      operation: cells[0],
      bytes: parseIntStrict(cells[1], "Bytes"),
      budget: parseIntStrict(cells[2], "Budget"),
    };
  });

  return { entries, aggregates };
}

/**
 * Compare committed documented footprint against freshly measured footprint.
 * Distinguishes budget overages, unapproved storage growth, and general drift.
 */
export function diffFootprint(documented, measured) {
  const overBudget = [];
  const unapprovedGrowth = [];
  const changes = [];

  const docEntriesByName = new Map(documented.entries.map((row) => [row.entry, row]));
  const measuredEntriesByName = new Map(measured.entries.map((row) => [row.entry, row]));

  for (const row of measured.entries) {
    if (row.total > row.budget) {
      overBudget.push(
        `Entry "${row.entry}" is ${row.total} bytes, exceeding its ${row.budget}-byte budget.`,
      );
    }

    const docRow = docEntriesByName.get(row.entry);
    if (!docRow) {
      const msg = `New storage entry "${row.entry}" (${row.map}, ${row.total} bytes, budget ${row.budget}) is not documented.`;
      unapprovedGrowth.push(msg);
      changes.push(msg);
      continue;
    }

    if (row.total > docRow.total) {
      const delta = row.total - docRow.total;
      const msg = `Entry "${row.entry}" grew from ${docRow.total} to ${row.total} bytes (+${delta} bytes).`;
      unapprovedGrowth.push(msg);
      changes.push(msg);
    } else if (row.budget > docRow.budget) {
      const msg = `Entry "${row.entry}" budget increased from ${docRow.budget} to ${row.budget} bytes without updating the report.`;
      unapprovedGrowth.push(msg);
      changes.push(msg);
    } else if (
      row.map !== docRow.map ||
      row.key !== docRow.key ||
      row.value !== docRow.value ||
      row.total !== docRow.total ||
      row.budget !== docRow.budget
    ) {
      changes.push(
        `Entry "${row.entry}" changed from (${docRow.map}, key=${docRow.key}, val=${docRow.value}, total=${docRow.total}, budget=${docRow.budget}) to (${row.map}, key=${row.key}, val=${row.value}, total=${row.total}, budget=${row.budget}).`,
      );
    }
  }

  for (const docRow of documented.entries) {
    if (!measuredEntriesByName.has(docRow.entry)) {
      changes.push(`Documented entry "${docRow.entry}" is no longer emitted by the contract.`);
    }
  }

  const docAggByOp = new Map(documented.aggregates.map((row) => [row.operation, row]));
  const measuredAggByOp = new Map(measured.aggregates.map((row) => [row.operation, row]));

  for (const agg of measured.aggregates) {
    if (agg.bytes > agg.budget) {
      overBudget.push(
        `Aggregate "${agg.operation}" is ${agg.bytes} bytes, exceeding its ${agg.budget}-byte budget.`,
      );
    }

    const docAgg = docAggByOp.get(agg.operation);
    if (!docAgg) {
      const msg = `New aggregate "${agg.operation}" (${agg.bytes} bytes, budget ${agg.budget}) is not documented.`;
      unapprovedGrowth.push(msg);
      changes.push(msg);
      continue;
    }

    if (agg.bytes > docAgg.bytes) {
      const delta = agg.bytes - docAgg.bytes;
      const msg = `Aggregate "${agg.operation}" grew from ${docAgg.bytes} to ${agg.bytes} bytes (+${delta} bytes).`;
      unapprovedGrowth.push(msg);
      changes.push(msg);
    } else if (agg.budget > docAgg.budget) {
      const msg = `Aggregate "${agg.operation}" budget increased from ${docAgg.budget} to ${agg.budget} bytes without updating the report.`;
      unapprovedGrowth.push(msg);
      changes.push(msg);
    } else if (agg.bytes !== docAgg.bytes || agg.budget !== docAgg.budget) {
      changes.push(
        `Aggregate "${agg.operation}" changed from (${docAgg.bytes} bytes, budget ${docAgg.budget}) to (${agg.bytes} bytes, budget ${agg.budget}).`,
      );
    }
  }

  for (const docAgg of documented.aggregates) {
    if (!measuredAggByOp.has(docAgg.operation)) {
      changes.push(`Documented aggregate "${docAgg.operation}" is no longer reported.`);
    }
  }

  return {
    isMatch: overBudget.length === 0 && unapprovedGrowth.length === 0 && changes.length === 0,
    overBudget,
    unapprovedGrowth,
    changes,
  };
}

/**
 * Render the per-entry GFM table with Prettier-compatible padding.
 */
export function renderEntryTable(entries) {
  const headers = ["Entry", "Map", "Key", "Value", "Total", "Budget"];
  const aligns = ["left", "left", "right", "right", "right", "right"];
  const rows = entries.map((row) => [
    row.entry,
    row.map,
    String(row.key),
    String(row.value),
    String(row.total),
    String(row.budget),
  ]);
  return formatMarkdownTable(headers, aligns, rows);
}

/**
 * Render the aggregate operations GFM table with Prettier-compatible padding.
 */
export function renderAggregateTable(aggregates) {
  const headers = ["Operation", "Bytes", "Budget"];
  const aligns = ["left", "right", "right"];
  const rows = aggregates.map((row) => [row.operation, String(row.bytes), String(row.budget)]);
  return formatMarkdownTable(headers, aligns, rows);
}

/**
 * Replace the two footprint tables inside `## Current footprint` while
 * preserving all surrounding prose verbatim.
 */
export function updateFootprintDoc(markdown, measured) {
  const headingMarker = "## Current footprint";
  const sectionStart = markdown.indexOf(headingMarker);
  if (sectionStart === -1) {
    throw new Error("Could not find `## Current footprint` heading in footprint document.");
  }

  const afterHeading = sectionStart + headingMarker.length;
  const nextHeadingOffset = markdown.slice(afterHeading).search(/\n## /);
  const sectionEnd = nextHeadingOffset === -1 ? markdown.length : afterHeading + nextHeadingOffset;
  const section = markdown.slice(sectionStart, sectionEnd);

  const tables = extractMarkdownTables(section);
  if (tables.length < 2) {
    throw new Error(
      "Expected both entry and aggregate tables inside `## Current footprint` section.",
    );
  }

  const firstTable = tables[0];
  const secondTable = tables[1];

  const updatedSection =
    section.slice(0, firstTable.startIndex) +
    renderEntryTable(measured.entries) +
    section.slice(firstTable.endIndex, secondTable.startIndex) +
    renderAggregateTable(measured.aggregates) +
    section.slice(secondTable.endIndex);

  return markdown.slice(0, sectionStart) + updatedSection + markdown.slice(sectionEnd);
}

function splitMarkdownRow(line) {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());
}

function parseIntStrict(raw, label) {
  if (!/^\d+$/.test(raw)) {
    throw new Error(`Invalid integer for ${label}: "${raw}"`);
  }
  return Number.parseInt(raw, 10);
}

function extractCurrentFootprintSection(markdown) {
  const headingMarker = "## Current footprint";
  const start = markdown.indexOf(headingMarker);
  if (start === -1) {
    throw new Error("Could not find `## Current footprint` heading in footprint document.");
  }
  const rest = markdown.slice(start + headingMarker.length);
  const nextHeading = rest.search(/\n## /);
  return nextHeading === -1
    ? markdown.slice(start)
    : markdown.slice(start, start + headingMarker.length + nextHeading);
}

function extractMarkdownTables(text) {
  const tables = [];
  const lines = text.split("\n");
  let offset = 0;
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (line.trim().startsWith("|") && i + 1 < lines.length && isSeparatorRow(lines[i + 1])) {
      const startIndex = offset;
      const headerLine = line;
      offset += line.length + 1;
      i += 1;

      // Skip separator row.
      offset += lines[i].length + 1;
      i += 1;

      const rows = [];
      let endIndex = offset - 1;
      while (i < lines.length && lines[i].trim().startsWith("|")) {
        rows.push(splitMarkdownRow(lines[i]));
        endIndex = offset + lines[i].length;
        offset += lines[i].length + 1;
        i += 1;
      }

      tables.push({
        headers: splitMarkdownRow(headerLine),
        rows,
        startIndex,
        endIndex,
      });
      continue;
    }

    offset += line.length + 1;
    i += 1;
  }

  return tables;
}

function isSeparatorRow(line) {
  const trimmed = line.trim();
  if (!trimmed.startsWith("|")) {
    return false;
  }
  const cells = splitMarkdownRow(trimmed);
  return cells.length > 0 && cells.every((cell) => /^:?-{2,}:?$/.test(cell));
}

function formatMarkdownTable(headers, aligns, rows) {
  const widths = headers.map((header, idx) => {
    const minWidth = aligns[idx] === "right" ? 3 : 3;
    const rowMax = rows.reduce((max, row) => Math.max(max, row[idx].length), 0);
    return Math.max(minWidth, header.length, rowMax);
  });

  const formatRow = (cells) =>
    `| ${cells
      .map((cell, idx) =>
        aligns[idx] === "right" ? cell.padStart(widths[idx]) : cell.padEnd(widths[idx]),
      )
      .join(" | ")} |`;

  const separator = `| ${widths
    .map((width, idx) => (aligns[idx] === "right" ? `${"-".repeat(width - 1)}:` : "-".repeat(width)))
    .join(" | ")} |`;

  return [formatRow(headers), separator, ...rows.map(formatRow)].join("\n");
}

function parseArgs(argv) {
  const opts = {
    check: false,
    write: false,
    docPath: null,
    inputPath: null,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--check") {
      opts.check = true;
    } else if (arg === "--write" || arg === "--refresh") {
      opts.write = true;
    } else if (arg === "--doc") {
      opts.docPath = argv[++i];
    } else if (arg === "--input") {
      opts.inputPath = argv[++i];
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (opts.check && opts.write) {
    throw new Error("Pass either --check or --write, not both.");
  }
  if (!opts.check && !opts.write) {
    opts.write = true;
  }
  return opts;
}

function runFootprintTest(repoRoot) {
  const manifestPath = path.join(repoRoot, "contract", "Cargo.toml");
  const result = spawnSync(
    "cargo",
    [
      "test",
      "--manifest-path",
      manifestPath,
      "storage_footprint_report",
      "--",
      "--nocapture",
    ],
    {
      cwd: repoRoot,
      encoding: "utf8",
    },
  );

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    if (result.stdout) {
      process.stdout.write(result.stdout);
    }
    if (result.stderr) {
      process.stderr.write(result.stderr);
    }
    throw new Error(`storage_footprint_report failed with exit code ${result.status}`);
  }

  return result.stdout ?? "";
}

export function main(argv = process.argv.slice(2)) {
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const repoRoot = path.resolve(scriptDir, "..");
  const opts = parseArgs(argv);
  const docPath = opts.docPath
    ? path.resolve(opts.docPath)
    : path.join(repoRoot, "docs", "contract-storage-footprint.md");

  const rawOutput = opts.inputPath
    ? readFileSync(path.resolve(opts.inputPath), "utf8")
    : runFootprintTest(repoRoot);

  const measured = parseFootprintTestOutput(rawOutput);
  const committedMarkdown = readFileSync(docPath, "utf8");
  const documented = parseDocumentedFootprint(committedMarkdown);
  const diff = diffFootprint(documented, measured);

  if (diff.overBudget.length > 0) {
    console.error("Storage footprint exceeds budget:");
    for (const line of diff.overBudget) {
      console.error(`  - ${line}`);
    }
    return 1;
  }

  if (opts.check) {
    if (!diff.isMatch) {
      if (diff.unapprovedGrowth.length > 0) {
        console.error("Unapproved contract storage footprint growth detected:");
        for (const line of diff.unapprovedGrowth) {
          console.error(`  - ${line}`);
        }
      } else {
        console.error("Contract storage footprint report is out of date:");
        for (const line of diff.changes) {
          console.error(`  - ${line}`);
        }
      }
      console.error(
        "\nIf this schema change is intentional, review the budgets in contract/contracts/vault-registry/src/test/storage_footprint.rs and refresh the report:\n  pnpm contract:footprint:refresh",
      );
      return 1;
    }

    console.log(
      `Storage footprint report is up to date (${measured.entries.length} entries, ${measured.aggregates.length} aggregates).`,
    );
    return 0;
  }

  const nextMarkdown = updateFootprintDoc(committedMarkdown, measured);
  writeFileSync(docPath, nextMarkdown, "utf8");
  console.log(
    `Updated ${path.relative(repoRoot, docPath)} (${measured.entries.length} entries, ${measured.aggregates.length} aggregates).`,
  );
  return 0;
}

const isEntrypoint =
  process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isEntrypoint) {
  try {
    process.exit(main());
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
