import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

import {
  diffFootprint,
  main,
  parseDocumentedFootprint,
  parseFootprintTestOutput,
  updateFootprintDoc,
} from "./refresh-storage-footprint.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const committedDocPath = path.join(repoRoot, "docs", "contract-storage-footprint.md");

function buildTestOutputFromDoc(doc, overrides = {}) {
  const entries = doc.entries.map((row) => ({
    ...row,
    ...(overrides.entries?.[row.entry] ?? {}),
  }));
  const perResource = overrides.perResource ?? doc.aggregates[0];
  const perPayment = overrides.perPayment ?? doc.aggregates[1];

  const lines = [
    "running 1 test",
    "",
    "### Storage footprint (XDR bytes)",
    "",
    "| Entry | Map | Key | Value | Total | Budget |",
    "| ----- | --- | ---:| -----:| -----:| ------:|",
    ...entries.map(
      (row) =>
        `| ${row.entry} | ${row.map} | ${row.key} | ${row.value} | ${row.total} | ${row.budget} |`,
    ),
    "",
    `Per max-size registration (Resource + Index + one TagIndex + one TagCount): ${perResource.bytes} bytes (budget: ${perResource.budget} bytes)`,
    `Per payment (PaymentReceipt + PaymentIndex): ${perPayment.bytes} bytes (budget: ${perPayment.budget} bytes)`,
    "",
    "test test::storage_footprint_report ... ok",
  ];

  return lines.join("\n");
}

describe("refresh-storage-footprint script (#231)", () => {
  it("parses committed docs/contract-storage-footprint.md and matches fresh output", () => {
    const markdown = readFileSync(committedDocPath, "utf8");
    const documented = parseDocumentedFootprint(markdown);
    assert.ok(documented.entries.length >= 20, "expected full entry table");
    assert.equal(documented.aggregates.length, 2, "expected both aggregate rows");

    const measured = parseFootprintTestOutput(buildTestOutputFromDoc(documented));
    const diff = diffFootprint(documented, measured);

    assert.equal(diff.isMatch, true);
    assert.deepEqual(diff.overBudget, []);
    assert.deepEqual(diff.unapprovedGrowth, []);
    assert.deepEqual(diff.changes, []);
  });

  it("also parses legacy aggregate output lines without explicit budget suffix", () => {
    const markdown = readFileSync(committedDocPath, "utf8");
    const documented = parseDocumentedFootprint(markdown);
    const legacyOutput = buildTestOutputFromDoc(documented)
      .replace(/ \(budget: \d+ bytes\)/g, "");

    const measured = parseFootprintTestOutput(legacyOutput);
    assert.equal(measured.aggregates[0].budget, 1900);
    assert.equal(measured.aggregates[1].budget, 850);
  });

  it("flags unapproved storage footprint growth when an entry or aggregate grows", () => {
    const markdown = readFileSync(committedDocPath, "utf8");
    const documented = parseDocumentedFootprint(markdown);

    const grownOutput = buildTestOutputFromDoc(documented, {
      entries: {
        "Resource (max-size)": { value: 1580, total: 1640 },
      },
      perResource: { bytes: 1896, budget: 1900 },
    });
    const measured = parseFootprintTestOutput(grownOutput);
    const diff = diffFootprint(documented, measured);

    assert.equal(diff.isMatch, false);
    assert.equal(diff.overBudget.length, 0);
    assert.equal(diff.unapprovedGrowth.length, 2);
    assert.match(diff.unapprovedGrowth[0], /Resource \(max-size\).*\+28 bytes/);
    assert.match(diff.unapprovedGrowth[1], /One max-size registration.*\+28 bytes/);
  });

  it("flags over-budget entries even before doc comparison", () => {
    const markdown = readFileSync(committedDocPath, "utf8");
    const documented = parseDocumentedFootprint(markdown);

    const overBudgetOutput = buildTestOutputFromDoc(documented, {
      entries: {
        Admin: { value: 60, total: 88, budget: 80 },
      },
    });
    const measured = parseFootprintTestOutput(overBudgetOutput);
    const diff = diffFootprint(documented, measured);

    assert.equal(diff.isMatch, false);
    assert.equal(diff.overBudget.length, 1);
    assert.match(diff.overBudget[0], /Admin.*88 bytes.*80-byte budget/);
  });

  it("updates the footprint tables in place while preserving surrounding prose and idempotency", () => {
    const markdown = readFileSync(committedDocPath, "utf8");
    const documented = parseDocumentedFootprint(markdown);

    const updatedMeasured = parseFootprintTestOutput(
      buildTestOutputFromDoc(documented, {
        entries: {
          "Pause deadline": { key: 32, value: 16, total: 48, budget: 64 },
        },
      }),
    );

    const refreshedOnce = updateFootprintDoc(markdown, updatedMeasured);
    const refreshedTwice = updateFootprintDoc(refreshedOnce, updatedMeasured);

    assert.equal(refreshedOnce, refreshedTwice, "refresh output must be idempotent");
    assert.ok(
      refreshedOnce.includes("## Notes on individual entries"),
      "surrounding documentation sections must be preserved",
    );

    const reparsed = parseDocumentedFootprint(refreshedOnce);
    const diff = diffFootprint(reparsed, updatedMeasured);
    assert.equal(diff.isMatch, true);
  });

  it("CLI --check exits 0 when fresh, 1 on unapproved growth, and --write refreshes the doc", () => {
    const dir = mkdtempSync(path.join(tmpdir(), "mv-footprint-"));
    try {
      const tempDoc = path.join(dir, "contract-storage-footprint.md");
      const tempInput = path.join(dir, "footprint-output.txt");
      const markdown = readFileSync(committedDocPath, "utf8");
      const documented = parseDocumentedFootprint(markdown);
      writeFileSync(tempDoc, markdown, "utf8");

      // Fresh output -> exit 0.
      writeFileSync(tempInput, buildTestOutputFromDoc(documented), "utf8");
      assert.equal(main(["--check", "--doc", tempDoc, "--input", tempInput]), 0);

      // Grown output -> --check exits 1.
      const grownOutput = buildTestOutputFromDoc(documented, {
        entries: {
          AttestationHash: { key: 56, value: 92, total: 148, budget: 160 },
        },
      });
      writeFileSync(tempInput, grownOutput, "utf8");
      assert.equal(main(["--check", "--doc", tempDoc, "--input", tempInput]), 1);

      // Refresh with --write -> subsequent --check exits 0.
      assert.equal(main(["--write", "--doc", tempDoc, "--input", tempInput]), 0);
      assert.equal(main(["--check", "--doc", tempDoc, "--input", tempInput]), 0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
