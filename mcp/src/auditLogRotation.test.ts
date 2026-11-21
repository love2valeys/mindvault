/**
 * Tests for JSONL audit log rotation (#592).
 *
 * Two properties carry the format: every entry is exactly one line (or the log
 * cannot be read back line by line), and the file never grows without bound.
 * The rest is making sure a broken disk degrades into a missing log rather
 * than a failed tool call.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AUDIT_FILE_ENV_VARS,
  DEFAULT_MAX_BYTES,
  DEFAULT_MAX_FILES,
  MIN_MAX_BYTES,
  RotatingJsonlWriter,
  createRotatingWriter,
  formatJsonl,
  resolveRotationConfig,
  rotatedPath,
} from "./auditLogRotation.js";
import {
  getAuditFileWriter,
  initAuditLogging,
  logToolSuccess,
  setAuditFileWriter,
  setAuditLogEnabled,
} from "./auditLog.js";
import { collectAuditTail } from "./debugBundle.js";

let directory: string;
let logPath: string;

beforeEach(() => {
  directory = mkdtempSync(join(tmpdir(), "mv-audit-"));
  logPath = join(directory, "audit.jsonl");
});

afterEach(() => {
  rmSync(directory, { recursive: true, force: true });
  setAuditLogEnabled(false);
  setAuditFileWriter(null);
  vi.restoreAllMocks();
});

function lines(path: string): string[] {
  return readFileSync(path, "utf-8").split("\n").filter(Boolean);
}

describe("resolveRotationConfig", () => {
  it("is off until a path is configured", () => {
    expect(resolveRotationConfig({})).toBeNull();
  });

  it("treats a whitespace-only path as unset", () => {
    expect(resolveRotationConfig({ [AUDIT_FILE_ENV_VARS.file]: "   " })).toBeNull();
  });

  it("uses the documented defaults", () => {
    const config = resolveRotationConfig({ [AUDIT_FILE_ENV_VARS.file]: logPath });

    expect(config).toEqual({
      path: logPath,
      maxBytes: DEFAULT_MAX_BYTES,
      maxFiles: DEFAULT_MAX_FILES,
    });
  });

  it("reads overrides", () => {
    const config = resolveRotationConfig({
      [AUDIT_FILE_ENV_VARS.file]: logPath,
      [AUDIT_FILE_ENV_VARS.maxBytes]: "8192",
      [AUDIT_FILE_ENV_VARS.maxFiles]: "2",
    });

    expect(config?.maxBytes).toBe(8192);
    expect(config?.maxFiles).toBe(2);
  });

  it("falls back to the default on a malformed size", () => {
    // A typo in an optional tuning variable must not stop the server.
    const config = resolveRotationConfig({
      [AUDIT_FILE_ENV_VARS.file]: logPath,
      [AUDIT_FILE_ENV_VARS.maxBytes]: "lots",
    });

    expect(config?.maxBytes).toBe(DEFAULT_MAX_BYTES);
  });

  it("refuses a size below the floor", () => {
    // Rotating every single line would produce thousands of files.
    const config = resolveRotationConfig({
      [AUDIT_FILE_ENV_VARS.file]: logPath,
      [AUDIT_FILE_ENV_VARS.maxBytes]: String(MIN_MAX_BYTES - 1),
    });

    expect(config?.maxBytes).toBe(DEFAULT_MAX_BYTES);
  });

  it("allows keeping zero generations", () => {
    const config = resolveRotationConfig({
      [AUDIT_FILE_ENV_VARS.file]: logPath,
      [AUDIT_FILE_ENV_VARS.maxFiles]: "0",
    });

    expect(config?.maxFiles).toBe(0);
  });
});

describe("formatJsonl", () => {
  it("emits one compact line per entry", () => {
    const line = formatJsonl({ a: 1, b: "two" });

    expect(line).toBe('{"a":1,"b":"two"}\n');
  });

  it("keeps a multi-line string on a single line", () => {
    // The invariant the whole format rests on: one entry can never become two
    // lines, however ugly the payload.
    const line = formatJsonl({ message: "first\nsecond" });

    expect(line.split("\n").filter(Boolean)).toHaveLength(1);
    expect(JSON.parse(line).message).toBe("first\nsecond");
  });

  it("round-trips through JSON.parse", () => {
    const entry = { timestamp: "2026-01-01T00:00:00.000Z", toolName: "x", status: "success" };

    expect(JSON.parse(formatJsonl(entry))).toEqual(entry);
  });
});

describe("RotatingJsonlWriter – writing", () => {
  it("creates the file and its parent directory", () => {
    const nested = join(directory, "deep", "nested", "audit.jsonl");
    const writer = new RotatingJsonlWriter({ path: nested, maxBytes: 4096, maxFiles: 2 });

    writer.write({ n: 1 });

    expect(existsSync(nested)).toBe(true);
  });

  it("appends one line per entry", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: 4096, maxFiles: 2 });

    writer.write({ n: 1 });
    writer.write({ n: 2 });
    writer.write({ n: 3 });

    expect(lines(logPath)).toHaveLength(3);
  });

  it("writes entries that parse back individually", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: 4096, maxFiles: 2 });

    writer.write({ toolName: "mindvault_browse", status: "success" });
    writer.write({ toolName: "mindvault_buy", status: "error" });

    expect(lines(logPath).map((line) => JSON.parse(line).toolName)).toEqual([
      "mindvault_browse",
      "mindvault_buy",
    ]);
  });

  it("resumes an existing file rather than truncating it", () => {
    writeFileSync(logPath, '{"existing":true}\n', "utf-8");
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: 4096, maxFiles: 2 });

    writer.write({ n: 1 });

    // A restart must not destroy the previous session's trail.
    expect(lines(logPath)).toHaveLength(2);
    expect(JSON.parse(lines(logPath)[0]).existing).toBe(true);
  });

  it("tracks the file size it has written", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: 4096, maxFiles: 2 });

    writer.write({ n: 1 });

    expect(writer.currentSize).toBe(statSync(logPath).size);
  });
});

describe("RotatingJsonlWriter – rotation", () => {
  /** ~40 bytes per entry, so a 200-byte budget holds about five. */
  function entry(n: number) {
    return { seq: n, payload: "x".repeat(20) };
  }

  it("rotates once the budget would be exceeded", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 3 });

    for (let n = 0; n < 200; n++) writer.write(entry(n));

    expect(existsSync(rotatedPath(logPath, 1))).toBe(true);
  });

  it("keeps the live file within the budget", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 3 });

    for (let n = 0; n < 200; n++) writer.write(entry(n));

    // Rotation happens before the write, so the limit is a real ceiling.
    expect(statSync(logPath).size).toBeLessThanOrEqual(MIN_MAX_BYTES);
  });

  it("never keeps more than maxFiles generations", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 2 });

    for (let n = 0; n < 500; n++) writer.write(entry(n));

    expect(existsSync(rotatedPath(logPath, 1))).toBe(true);
    expect(existsSync(rotatedPath(logPath, 2))).toBe(true);
    expect(existsSync(rotatedPath(logPath, 3))).toBe(false);
  });

  it("shifts generations so .1 is always the newest rotation", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 3 });

    for (let n = 0; n < 500; n++) writer.write(entry(n));

    const newest = JSON.parse(lines(rotatedPath(logPath, 1))[0]).seq;
    const older = JSON.parse(lines(rotatedPath(logPath, 2))[0]).seq;
    expect(newest).toBeGreaterThan(older);
  });

  it("every rotated file is still valid JSONL", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 3 });

    for (let n = 0; n < 500; n++) writer.write(entry(n));

    for (const path of [logPath, rotatedPath(logPath, 1), rotatedPath(logPath, 2)]) {
      for (const line of lines(path)) {
        expect(() => JSON.parse(line), `unparseable line in ${path}: ${line}`).not.toThrow();
      }
    }
  });

  it("loses no entries across a rotation boundary", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 9 });

    for (let n = 0; n < 100; n++) writer.write(entry(n));

    const seen = new Set<number>();
    for (let generation = 0; generation <= 9; generation++) {
      const path = generation === 0 ? logPath : rotatedPath(logPath, generation);
      if (!existsSync(path)) continue;
      for (const line of lines(path)) seen.add(JSON.parse(line).seq);
    }
    expect(seen.size).toBe(100);
  });

  it("discards the live file when no generations are kept", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 0 });

    for (let n = 0; n < 200; n++) writer.write(entry(n));

    expect(existsSync(rotatedPath(logPath, 1))).toBe(false);
    expect(statSync(logPath).size).toBeLessThanOrEqual(MIN_MAX_BYTES);
  });

  it("writes an over-sized entry rather than dropping it", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 2 });

    writer.write({ payload: "y".repeat(MIN_MAX_BYTES * 2) });

    // Truncating an audit record would be worse than briefly exceeding a
    // size target.
    expect(lines(logPath)).toHaveLength(1);
  });

  it("does not rotate an empty file", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: 10, maxFiles: 2 });

    writer.write({ payload: "z".repeat(100) });

    expect(existsSync(rotatedPath(logPath, 1))).toBe(false);
  });
});

describe("RotatingJsonlWriter – failure handling", () => {
  it("disables itself and reports once when the filesystem refuses", () => {
    const onError = vi.fn();
    // A path whose parent is a file, not a directory.
    writeFileSync(logPath, "", "utf-8");
    const writer = new RotatingJsonlWriter(
      { path: join(logPath, "nested.jsonl"), maxBytes: 4096, maxFiles: 2 },
      onError,
    );

    writer.write({ n: 1 });
    writer.write({ n: 2 });
    writer.write({ n: 3 });

    expect(writer.enabled).toBe(false);
    // One report, not one per tool call.
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("never throws into the caller", () => {
    writeFileSync(logPath, "", "utf-8");
    const writer = new RotatingJsonlWriter({
      path: join(logPath, "nested.jsonl"),
      maxBytes: 4096,
      maxFiles: 2,
    });

    // Failing a paid tool call over a logging problem trades a small loss for
    // a large one.
    expect(() => writer.write({ n: 1 })).not.toThrow();
  });
});

describe("createRotatingWriter", () => {
  it("returns null when the sink is not configured", () => {
    expect(createRotatingWriter({})).toBeNull();
  });

  it("builds a writer when a path is set", () => {
    const writer = createRotatingWriter({ [AUDIT_FILE_ENV_VARS.file]: logPath });

    expect(writer?.config.path).toBe(logPath);
  });
});

describe("audit log integration", () => {
  it("stays stderr-only when no file is configured", () => {
    initAuditLogging({ MINDVAULT_AUDIT_LOG: "1" });

    expect(getAuditFileWriter()).toBeNull();
  });

  it("attaches the file sink when a path is set", () => {
    initAuditLogging({ MINDVAULT_AUDIT_LOG: "1", [AUDIT_FILE_ENV_VARS.file]: logPath });

    expect(getAuditFileWriter()).not.toBeNull();
  });

  it("does not write a file while audit logging is off", () => {
    initAuditLogging({ [AUDIT_FILE_ENV_VARS.file]: logPath });

    expect(getAuditFileWriter()).toBeNull();
  });

  it("writes real entries as JSONL", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    initAuditLogging({ MINDVAULT_AUDIT_LOG: "1", [AUDIT_FILE_ENV_VARS.file]: logPath });

    logToolSuccess("mindvault_browse", 12, { network: "testnet" });

    const [entry] = lines(logPath).map((line) => JSON.parse(line));
    expect(entry.toolName).toBe("mindvault_browse");
    expect(entry.status).toBe("success");
    expect(entry.duration).toBe(12);
  });

  it("keeps writing to stderr as well", () => {
    const stderr = vi.spyOn(console, "error").mockImplementation(() => {});
    initAuditLogging({ MINDVAULT_AUDIT_LOG: "1", [AUDIT_FILE_ENV_VARS.file]: logPath });

    logToolSuccess("mindvault_browse", 1);

    // stderr is for watching a live session; the file is for keeping.
    expect(stderr).toHaveBeenCalled();
  });

  it("rotates a real audit stream", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    initAuditLogging({
      MINDVAULT_AUDIT_LOG: "1",
      [AUDIT_FILE_ENV_VARS.file]: logPath,
      [AUDIT_FILE_ENV_VARS.maxBytes]: String(MIN_MAX_BYTES),
      [AUDIT_FILE_ENV_VARS.maxFiles]: "2",
    });

    for (let n = 0; n < 300; n++) logToolSuccess("mindvault_browse", n, { message: `call ${n}` });

    expect(existsSync(rotatedPath(logPath, 1))).toBe(true);
    expect(statSync(logPath).size).toBeLessThanOrEqual(MIN_MAX_BYTES);
  });
});

describe("RotatingJsonlWriter – read-lock and mutex serialization (#213)", () => {
  function entry(n: number) {
    return { seq: n, payload: "x".repeat(40) };
  }

  function collectAllSeqs(basePath: string, maxFiles: number): number[] {
    const seqs: number[] = [];
    for (let generation = maxFiles; generation >= 1; generation--) {
      const path = rotatedPath(basePath, generation);
      if (!existsSync(path)) continue;
      for (const line of lines(path)) seqs.push(JSON.parse(line).seq);
    }
    if (existsSync(basePath)) {
      for (const line of lines(basePath)) seqs.push(JSON.parse(line).seq);
    }
    return seqs;
  }

  it("defers rotation across a synchronous read lock and flushes buffered tail entries on release", () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 4 });

    // Fill the live file close to the rotation limit.
    for (let n = 0; n < 12; n++) writer.write(entry(n));
    const snapshotBeforeLock = lines(logPath).length;
    expect(existsSync(rotatedPath(logPath, 1))).toBe(false);

    writer.withReadLockSync(() => {
      expect(writer.readerCount).toBe(1);
      // Write enough entries to cross maxBytes while the read lock is held.
      for (let n = 12; n < 30; n++) writer.write(entry(n));

      // Live file is untouched and unrotated while the reader holds the lock.
      expect(lines(logPath)).toHaveLength(snapshotBeforeLock);
      expect(existsSync(rotatedPath(logPath, 1))).toBe(false);
      expect(writer.pendingCount).toBe(18);
    });

    // Once the read lock releases, all buffered entries flush and rotate cleanly.
    expect(writer.readerCount).toBe(0);
    expect(writer.pendingCount).toBe(0);
    expect(existsSync(rotatedPath(logPath, 1))).toBe(true);

    const all = collectAllSeqs(logPath, 4);
    expect(all).toEqual(Array.from({ length: 30 }, (_, i) => i));
  });

  it("serializes concurrent writeAsync and rotateAsync across an async read lock without dropping tail entries", async () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 4 });

    writer.write(entry(0));
    writer.write(entry(1));

    let releaseReader!: () => void;
    const readerGate = new Promise<void>((resolve) => {
      releaseReader = resolve;
    });

    const readerPromise = writer.withReadLock(async () => {
      const seenBefore = lines(logPath).map((l) => JSON.parse(l).seq);
      expect(seenBefore).toEqual([0, 1]);
      await readerGate;
      // Live file must remain intact (not truncated or renamed) until reader exits.
      expect(existsSync(rotatedPath(logPath, 1))).toBe(false);
      expect(lines(logPath).map((l) => JSON.parse(l).seq)).toEqual([0, 1]);
    });

    // Concurrent sync + async appends and explicit rotation while read lock is active.
    writer.write(entry(2));
    const append3 = writer.writeAsync(entry(3));
    const append4 = writer.writeAsync(entry(4));
    const rotatePromise = writer.rotateAsync();
    const append5 = writer.writeAsync(entry(5));

    // Release the reader so queued appends and rotation can serialize and flush.
    releaseReader();
    await Promise.all([readerPromise, append3, append4, rotatePromise, append5]);

    // Tail entries (0..4) must be flushed to the rotated file (.1), and entry 5 in the fresh live file.
    expect(existsSync(rotatedPath(logPath, 1))).toBe(true);
    const rotatedSeqs = lines(rotatedPath(logPath, 1)).map((l) => JSON.parse(l).seq);
    const liveSeqs = lines(logPath).map((l) => JSON.parse(l).seq);

    expect(rotatedSeqs).toEqual([0, 1, 2, 3, 4]);
    expect(liveSeqs).toEqual([5]);
  });

  it("waits for nested/overlapping read locks to drain before rotating", async () => {
    const writer = new RotatingJsonlWriter({ path: logPath, maxBytes: MIN_MAX_BYTES, maxFiles: 3 });
    writer.write(entry(0));

    let releaseOuter!: () => void;
    let releaseInner!: () => void;
    const outerGate = new Promise<void>((r) => {
      releaseOuter = r;
    });
    const innerGate = new Promise<void>((r) => {
      releaseInner = r;
    });

    const outer = writer.withReadLock(async () => {
      await outerGate;
    });
    const inner = writer.withReadLock(async () => {
      await innerGate;
    });

    expect(writer.readerCount).toBe(2);
    writer.write(entry(1));
    writer.rotate();
    writer.write(entry(2));

    releaseOuter();
    await outer;
    // Inner reader still holds a read lock: rotation must still be deferred.
    expect(writer.readerCount).toBe(1);
    expect(existsSync(rotatedPath(logPath, 1))).toBe(false);

    releaseInner();
    await inner;

    expect(writer.readerCount).toBe(0);
    expect(writer.pendingCount).toBe(0);
    expect(lines(rotatedPath(logPath, 1)).map((l) => JSON.parse(l).seq)).toEqual([0, 1]);
    expect(lines(logPath).map((l) => JSON.parse(l).seq)).toEqual([2]);
  });

  it("reads audit tail safely through collectAuditTail alongside active rotating writer", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    initAuditLogging({
      MINDVAULT_AUDIT_LOG: "1",
      [AUDIT_FILE_ENV_VARS.file]: logPath,
      [AUDIT_FILE_ENV_VARS.maxBytes]: String(MIN_MAX_BYTES),
      [AUDIT_FILE_ENV_VARS.maxFiles]: "2",
    });

    logToolSuccess("mindvault_browse", 5);
    logToolSuccess("mindvault_search", 9);

    const tail = collectAuditTail({ [AUDIT_FILE_ENV_VARS.file]: logPath }, 10);
    expect(tail.exists).toBe(true);
    expect(tail.entries).toHaveLength(2);
    expect(tail.entries[0].toolName).toBe("mindvault_browse");
    expect(tail.entries[1].toolName).toBe("mindvault_search");
  });
});
