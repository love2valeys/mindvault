import { describe, it, expect } from "vitest";
import { escapeCsvCell, receiptsToCsv } from "../export.js";
import type { Receipt } from "../types.js";

describe("escapeCsvCell", () => {
  it("leaves plain values untouched", () => {
    expect(escapeCsvCell("Coffee 3.50")).toBe("Coffee 3.50");
    expect(escapeCsvCell("123.45")).toBe("123.45");
  });

  it("neutralizes cells beginning with formula characters", () => {
    expect(escapeCsvCell("=SUM(A1:A2)")).toBe("'=SUM(A1:A2)");
    expect(escapeCsvCell("+1+1")).toBe("'+1+1");
    expect(escapeCsvCell("-2-2")).toBe("'-2-2");
    expect(escapeCsvCell("@cmd")).toBe("'@cmd");
  });

  it("neutralizes cells with leading whitespace", () => {
    expect(escapeCsvCell(" =1+1")).toBe("' =1+1");
    expect(escapeCsvCell("\t=1+1")).toBe("'\t=1+1");
  });

  it("quotes cells containing delimiters or quotes", () => {
    expect(escapeCsvCell('a,b')).toBe('"a,b"');
    expect(escapeCsvCell('say "hi"')).toBe('"say ""hi"""');
  });
});

describe("receiptsToCsv", () => {
  const baseReceipt: Receipt = {
    id: "r1",
    merchant: "Cafe",
    total: 3.5,
    currency: "USD",
    date: "2024-01-01",
  };

  it("escapes formula-like merchant values in exported rows", () => {
    const csv = receiptsToCsv([
      { ...baseReceipt, merchant: "=HYPERLINK(\"http://evil\")" },
      { ...baseReceipt, id: "r2", merchant: "+1+1" },
      { ...baseReceipt, id: "r3", merchant: "-2-2" },
      { ...baseReceipt, id: "r4", merchant: "@cmd" },
      { ...baseReceipt, id: "r5", merchant: " =1+1" },
    ]);

    expect(csv).toContain("'=HYPERLINK");
    expect(csv).toContain("'+1+1");
    expect(csv).toContain("'-2-2");
    expect(csv).toContain("'@cmd");
    expect(csv).toContain("' =1+1");
  });

  it("preserves legitimate receipt values", () => {
    const csv = receiptsToCsv([baseReceipt]);
    expect(csv).toContain("Cafe");
    expect(csv).toContain("3.5");
    expect(csv).toContain("USD");
  });
});
