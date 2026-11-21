/**
 * Encrypted local-state backup and restore for the MindVault MCP server.
 *
 * State holds credentials (wallet secret keys, publisher API keys), so a backup
 * must never leak plaintext secrets. This module encrypts the persisted
 * ProfileState with AES-256-GCM, keyed from a caller-supplied passphrase via
 * scrypt. The output is a single base64 string an agent can copy between
 * environments.
 *
 * Integrity is bound to the ciphertext: any tampering or wrong passphrase fails
 * the GCM auth tag before any state is touched, so restoreState is safe to
 * call on untrusted input.
 *
 * Reset behavior is intentionally untouched — this module only adds export/import.
 */

import { createCipheriv, createDecipheriv, randomBytes, scryptSync } from "crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "fs";
import { homedir } from "os";
import { dirname, join } from "path";
import {
  isValidProfileName,
  migrateState,
  STATE_VERSION,
  type ProfileState,
  type WalletProfile,
} from "./profiles.js";

const STATE_DIR = join(homedir(), ".mindvault");
const STATE_FILE = join(STATE_DIR, "state.json");
const BACKUP_DIR = join(STATE_DIR, "backups");

// scrypt cost params — tuned for interactive passphrase derivation (not hot path).
// N=2^14 keeps OpenSSL maxmem happy in constrained CI/agent envs.
const SCRYPT_N = 1 << 14;
const SCRYPT_R = 8;
const SCRYPT_P = 1;
const SCRYPT_MAXMEM = 64 * 1024 * 1024;
const KEY_LEN = 32;
const SALT_LEN = 16;
const NONCE_LEN = 12;
const AUTH_TAG_LEN = 8 * 2;

/** Deterministic, agent-safe error messages (no internal details leaked). */
export class StateBackupError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "StateBackupError";
  }
}

export interface RestoreStateOptions {
  expectedNetwork?: string;
}

export interface PersistedStateSecretMatch {
  path: string;
  kind: "wallet-secret-key" | "api-key";
}

/**
 * Report secret-bearing fields in an unencrypted persisted-state backup before
 * it is shared. Encrypted `exportState` output is safe to transport instead.
 */
export function scanPersistedStateSecrets(raw: unknown): PersistedStateSecretMatch[] {
  if (!raw || typeof raw !== "object") return [];
  const profiles = (raw as { profiles?: unknown }).profiles;
  if (!profiles || typeof profiles !== "object") return [];

  const matches: PersistedStateSecretMatch[] = [];
  for (const [profileName, value] of Object.entries(profiles as Record<string, unknown>)) {
    if (!value || typeof value !== "object") continue;
    const profile = value as { apiKey?: unknown; wallet?: { secretKey?: unknown } };
    if (typeof profile.wallet?.secretKey === "string" && profile.wallet.secretKey.length > 0) {
      matches.push({ path: `profiles.${profileName}.wallet.secretKey`, kind: "wallet-secret-key" });
    }
    if (typeof profile.apiKey === "string" && profile.apiKey.length > 0) {
      matches.push({ path: `profiles.${profileName}.apiKey`, kind: "api-key" });
    }
  }
  return matches;
}

/**
 * Read the current persisted state. Returns a normalized ProfileState.
 * Throws a deterministic error when the state file is missing or unreadable.
 */
export function readPersistedState(): ProfileState {
  if (!existsSync(STATE_FILE)) {
    throw new StateBackupError("No state file found. Run mindvault_setup_wallet first.");
  }
  try {
    const raw = readFileSync(STATE_FILE, "utf-8");
    const parsed = JSON.parse(raw) as unknown;
    return normalizePersisted(parsed);
  } catch {
    throw new StateBackupError("State file is corrupted and could not be read.");
  }
}

/**
 * Export the current state as an encrypted, base64-encoded string.
 *
 * The output is self-contained: v1:<salt>:<nonce>:<ciphertext+tag>. The
 * passphrase is never stored or logged. Plaintext secrets never appear in the
 * output — only the encrypted profile bytes do.
 */
export function exportState(passphrase: string): string {
  if (!passphrase || passphrase.length < 8) {
    throw new StateBackupError("Passphrase must be at least 8 characters.");
  }
  const state = readPersistedState();
  const payload = JSON.stringify(state);
  const salt = randomBytes(SALT_LEN);
  const nonce = randomBytes(NONCE_LEN);
  const key = scryptSync(passphrase, salt, KEY_LEN, {
    N: SCRYPT_N,
    r: SCRYPT_R,
    p: SCRYPT_P,
    maxmem: SCRYPT_MAXMEM,
  });
  const cipher = createCipheriv("aes-256-gcm", key, nonce);
  const ciphertext = Buffer.concat([cipher.update(payload, "utf8"), cipher.final()]);
  const tag = cipher.getAuthTag();
  const blob = Buffer.concat([ciphertext, tag]);
  return `v1:${salt.toString("base64")}:${nonce.toString("base64")}:${blob.toString("base64")}`;
}

/** Encrypt the current state and write a private recovery file. */
export function exportStateFile(
  passphrase: string,
  now: Date = new Date(),
  directory: string = BACKUP_DIR,
): string {
  const stamp = now.toISOString().replace(/[:.]/g, "-");
  const path = join(directory, `state-${stamp}.backup`);
  writeAtomically(path, `${exportState(passphrase)}\n`, 0o600);
  return path;
}

/**
 * Restore state from an encrypted backup string.
 *
 * Validates the passphrase and integrity (GCM auth tag) before writing anything.
 * On success the in-memory state is replaced and re-persisted (mode 0600).
 * On any failure no state is modified and a deterministic error is thrown.
 */
export function restoreState(
  blob: string,
  passphrase: string,
  write: (state: ProfileState) => void,
  options: RestoreStateOptions = {},
): string {
  if (!passphrase || passphrase.length < 8) {
    throw new StateBackupError("Passphrase must be at least 8 characters.");
  }
  const parts = blob.split(":");
  if (parts.length !== 4 || parts[0] !== "v1") {
    throw new StateBackupError("Invalid backup format.");
  }
  let salt: Buffer;
  let nonce: Buffer;
  let payload: Buffer;
  try {
    salt = Buffer.from(parts[1], "base64");
    nonce = Buffer.from(parts[2], "base64");
    payload = Buffer.from(parts[3], "base64");
  } catch {
    throw new StateBackupError("Invalid backup encoding.");
  }
  if (salt.length !== SALT_LEN || nonce.length !== NONCE_LEN || payload.length < AUTH_TAG_LEN) {
    throw new StateBackupError("Invalid backup payload.");
  }
  const key = scryptSync(passphrase, salt, KEY_LEN, {
    N: SCRYPT_N,
    r: SCRYPT_R,
    p: SCRYPT_P,
    maxmem: SCRYPT_MAXMEM,
  });
  const ciphertext = payload.subarray(0, payload.length - AUTH_TAG_LEN);
  const tag = payload.subarray(payload.length - AUTH_TAG_LEN);
  const decipher = createDecipheriv("aes-256-gcm", key, nonce);
  decipher.setAuthTag(tag);
  let plaintext: string;
  try {
    plaintext = Buffer.concat([decipher.update(ciphertext), decipher.final()]).toString("utf8");
  } catch {
    throw new StateBackupError(
      "Backup integrity check failed (wrong passphrase or tampered data).",
    );
  }
  let state: ProfileState;
  try {
    assertNoDuplicateJsonKeys(plaintext);
    state = validateRestoredState(JSON.parse(plaintext) as unknown);
  } catch (err) {
    if (err instanceof StateBackupError) throw err;
    if (err instanceof Error && err.message.startsWith("Duplicate field at ")) {
      throw new StateBackupError(`Backup contents are not valid state: ${err.message}`);
    }
    throw new StateBackupError("Backup contents are not valid state.");
  }
  if (options.expectedNetwork) {
    const mismatched = Object.entries(state.profiles).find(
      ([, profile]) => profile.network && profile.network !== options.expectedNetwork,
    );
    if (mismatched) {
      throw new StateBackupError(
        `Backup belongs to network "${mismatched[1].network}" but the active profile uses "${options.expectedNetwork}".`,
      );
    }
  }
  write(state);
  return `State restored: ${Object.keys(state.profiles).length} profile(s), active "${state.activeProfile}".`;
}

/**
 * Normalize an unknown parsed value into a valid ProfileState, mirroring the
 * rules in profiles.ts (valid profile names, valid wallets, non-empty API
 * keys). Unrecognized fields are dropped so restored state cannot smuggle junk.
 */
function normalizePersisted(raw: unknown): ProfileState {
  if (!raw || typeof raw !== "object") {
    return { version: STATE_VERSION, activeProfile: "default", profiles: {} };
  }
  const obj = raw as Record<string, unknown>;
  const profiles: Record<string, WalletProfile> = {};
  const rawProfiles =
    obj.profiles && typeof obj.profiles === "object"
      ? (obj.profiles as Record<string, unknown>)
      : {};
  for (const [name, value] of Object.entries(rawProfiles)) {
    if (!/^[A-Za-z0-9._-]{1,64}$/.test(name) || !value || typeof value !== "object") continue;
    const v = value as Record<string, unknown>;
    const profile: WalletProfile = {};
    const w = v.wallet;
    if (w && typeof w === "object") {
      const ww = w as Record<string, unknown>;
      if (
        typeof ww.publicKey === "string" &&
        ww.publicKey.length > 0 &&
        typeof ww.secretKey === "string" &&
        ww.secretKey.length > 0
      ) {
        profile.wallet = { publicKey: ww.publicKey, secretKey: ww.secretKey };
      }
    }
    if (typeof v.apiKey === "string" && v.apiKey.length > 0) profile.apiKey = v.apiKey;
    if (v.network === "testnet" || v.network === "mainnet") profile.network = v.network;
    profiles[name] = profile;
  }
  const requested = typeof obj.activeProfile === "string" ? obj.activeProfile : "default";
  const activeProfile = (requested in profiles ? requested : Object.keys(profiles)[0]) ?? "default";
  return { version: STATE_VERSION, activeProfile, profiles };
}

function validateRestoredState(raw: unknown): ProfileState {
  const issues: string[] = [];
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    throw new StateBackupError("Backup contents are not valid state: state must be an object.");
  }
  const obj = raw as Record<string, unknown>;
  for (const field of Object.keys(obj)) {
    if (!["version", "activeProfile", "profiles"].includes(field)) {
      issues.push(`${field}: unexpected field.`);
    }
  }
  if (obj.version !== STATE_VERSION) issues.push(`version: expected ${STATE_VERSION}.`);
  if (typeof obj.activeProfile !== "string" || !isValidProfileName(obj.activeProfile)) {
    issues.push("activeProfile: expected a valid profile identifier.");
  }
  if (!obj.profiles || typeof obj.profiles !== "object" || Array.isArray(obj.profiles)) {
    issues.push("profiles: expected an object keyed by profile identifier.");
  }

  const profiles: Record<string, WalletProfile> = {};
  const walletOwners = new Map<string, string>();
  if (obj.profiles && typeof obj.profiles === "object" && !Array.isArray(obj.profiles)) {
    for (const [name, value] of Object.entries(obj.profiles as Record<string, unknown>)) {
      if (!isValidProfileName(name)) {
        issues.push(`profiles.${name}: invalid profile identifier.`);
        continue;
      }
      if (!value || typeof value !== "object" || Array.isArray(value)) {
        issues.push(`profiles.${name}: expected an object.`);
        continue;
      }
      const profileInput = value as Record<string, unknown>;
      const profile: WalletProfile = {};
      for (const field of Object.keys(profileInput)) {
        if (!["wallet", "apiKey", "network"].includes(field)) {
          issues.push(`profiles.${name}.${field}: unexpected field.`);
        }
      }
      if ("wallet" in profileInput) {
        const wallet = profileInput.wallet;
        if (!wallet || typeof wallet !== "object" || Array.isArray(wallet)) {
          issues.push(`profiles.${name}.wallet: expected an object.`);
        } else {
          const candidate = wallet as Record<string, unknown>;
          for (const field of Object.keys(candidate)) {
            if (!["publicKey", "secretKey"].includes(field)) {
              issues.push(`profiles.${name}.wallet.${field}: unexpected field.`);
            }
          }
          if (typeof candidate.publicKey !== "string" || candidate.publicKey.length === 0) {
            issues.push(`profiles.${name}.wallet.publicKey: expected a non-empty string.`);
          }
          if (typeof candidate.secretKey !== "string" || candidate.secretKey.length === 0) {
            issues.push(`profiles.${name}.wallet.secretKey: expected a non-empty string.`);
          }
          if (
            typeof candidate.publicKey === "string" &&
            candidate.publicKey.length > 0 &&
            typeof candidate.secretKey === "string" &&
            candidate.secretKey.length > 0
          ) {
            const owner = walletOwners.get(candidate.publicKey);
            if (owner) {
              issues.push(`profiles.${name}.wallet.publicKey: conflicts with profile "${owner}".`);
            } else {
              walletOwners.set(candidate.publicKey, name);
            }
            profile.wallet = {
              publicKey: candidate.publicKey,
              secretKey: candidate.secretKey,
            };
          }
        }
      }
      if ("apiKey" in profileInput) {
        if (typeof profileInput.apiKey !== "string" || profileInput.apiKey.length === 0) {
          issues.push(`profiles.${name}.apiKey: expected a non-empty string.`);
        } else profile.apiKey = profileInput.apiKey;
      }
      if ("network" in profileInput) {
        if (profileInput.network !== "testnet" && profileInput.network !== "mainnet") {
          issues.push(`profiles.${name}.network: expected "testnet" or "mainnet".`);
        } else profile.network = profileInput.network;
      }
      profiles[name] = profile;
    }
  }
  if (
    typeof obj.activeProfile === "string" &&
    isValidProfileName(obj.activeProfile) &&
    obj.profiles &&
    typeof obj.profiles === "object" &&
    !Array.isArray(obj.profiles) &&
    !(obj.activeProfile in (obj.profiles as Record<string, unknown>))
  ) {
    issues.push("activeProfile: must name an entry in profiles.");
  }
  if (issues.length > 0) {
    throw new StateBackupError(`Backup contents are not valid state: ${issues.join(" ")}`);
  }
  return {
    version: STATE_VERSION,
    activeProfile: obj.activeProfile as string,
    profiles,
  };
}

function assertNoDuplicateJsonKeys(source: string): void {
  let index = 0;
  const skipWhitespace = () => {
    while (/\s/.test(source[index] ?? "")) index++;
  };
  const parseString = (): string => {
    const start = index++;
    while (index < source.length) {
      if (source[index] === "\\") index += 2;
      else if (source[index++] === '"') return JSON.parse(source.slice(start, index)) as string;
    }
    throw new Error("Invalid JSON string.");
  };
  const parseValue = (path: string): void => {
    skipWhitespace();
    if (source[index] === "{") {
      index++;
      skipWhitespace();
      const keys = new Set<string>();
      if (source[index] === "}") {
        index++;
        return;
      }
      while (index < source.length) {
        skipWhitespace();
        if (source[index] !== '"') throw new Error("Invalid JSON object key.");
        const key = parseString();
        if (keys.has(key)) throw new Error(`Duplicate field at ${path}.${key}.`);
        keys.add(key);
        skipWhitespace();
        if (source[index++] !== ":") throw new Error("Invalid JSON object.");
        parseValue(`${path}.${key}`);
        skipWhitespace();
        const delimiter = source[index++];
        if (delimiter === "}") return;
        if (delimiter !== ",") throw new Error("Invalid JSON object.");
      }
      throw new Error("Invalid JSON object.");
    }
    if (source[index] === "[") {
      index++;
      skipWhitespace();
      if (source[index] === "]") {
        index++;
        return;
      }
      let item = 0;
      while (index < source.length) {
        parseValue(`${path}[${item++}]`);
        skipWhitespace();
        const delimiter = source[index++];
        if (delimiter === "]") return;
        if (delimiter !== ",") throw new Error("Invalid JSON array.");
      }
      throw new Error("Invalid JSON array.");
    }
    if (source[index] === '"') {
      parseString();
      return;
    }
    const start = index;
    while (index < source.length && !/[\s,}\]]/.test(source[index])) index++;
    if (index === start) throw new Error("Invalid JSON value.");
  };

  parseValue("$");
  skipWhitespace();
  if (index !== source.length) throw new Error("Unexpected trailing JSON.");
}

export interface SelectiveStateExport {
  version: 1;
  activeProfile: string;
  profiles: Record<string, { network?: "testnet" | "mainnet" }>;
}

export function exportSelectiveState(state: ProfileState): string {
  const profiles: SelectiveStateExport["profiles"] = {};
  for (const name of Object.keys(state.profiles).sort()) {
    const network = state.profiles[name].network;
    profiles[name] = network ? { network } : {};
  }
  return JSON.stringify(
    { version: 1, activeProfile: state.activeProfile, profiles } satisfies SelectiveStateExport,
    null,
    2,
  );
}

export function importSelectiveState(blob: string, current: ProfileState): ProfileState {
  let raw: unknown;
  try {
    assertNoDuplicateJsonKeys(blob);
    raw = JSON.parse(blob) as unknown;
  } catch {
    throw new StateBackupError("Selective export is not valid JSON or contains duplicate fields.");
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    throw new StateBackupError("Selective export must be an object.");
  }
  const input = raw as Record<string, unknown>;
  const issues: string[] = [];
  if (input.version !== 1) issues.push("version: expected 1.");
  if (typeof input.activeProfile !== "string" || !isValidProfileName(input.activeProfile)) {
    issues.push("activeProfile: expected a valid profile identifier.");
  }
  if (!input.profiles || typeof input.profiles !== "object" || Array.isArray(input.profiles)) {
    issues.push("profiles: expected an object keyed by profile identifier.");
  }
  const imported =
    input.profiles && typeof input.profiles === "object" && !Array.isArray(input.profiles)
      ? (input.profiles as Record<string, unknown>)
      : {};
  for (const [name, profile] of Object.entries(imported)) {
    if (!isValidProfileName(name)) issues.push(`profiles.${name}: invalid profile identifier.`);
    if (!profile || typeof profile !== "object" || Array.isArray(profile)) {
      issues.push(`profiles.${name}: expected an object.`);
      continue;
    }
    const fields = profile as Record<string, unknown>;
    if (Object.keys(fields).some((field) => field !== "network")) {
      issues.push(`profiles.${name}: only the network preference is allowed.`);
    }
    if ("network" in fields && fields.network !== "testnet" && fields.network !== "mainnet") {
      issues.push(`profiles.${name}.network: expected "testnet" or "mainnet".`);
    }
  }
  if (
    typeof input.activeProfile === "string" &&
    isValidProfileName(input.activeProfile) &&
    !(input.activeProfile in imported)
  ) {
    issues.push("activeProfile: must name an entry in profiles.");
  }
  if (issues.length) throw new StateBackupError(`Invalid selective export: ${issues.join(" ")}`);

  const profiles = { ...current.profiles };
  for (const [name, value] of Object.entries(imported)) {
    const existing = profiles[name] ?? {};
    const network = (value as { network?: "testnet" | "mainnet" }).network;
    profiles[name] = { ...existing, ...(network ? { network } : {}) };
  }
  return {
    version: STATE_VERSION,
    activeProfile: input.activeProfile as string,
    profiles,
  };
}

export function previewPersistedStateMigration(): string {
  let rawText: string;
  try {
    rawText = readFileSync(STATE_FILE, "utf8");
  } catch {
    throw new StateBackupError("No readable state file found to preview migration.");
  }
  let raw: unknown;
  try {
    raw = JSON.parse(rawText) as unknown;
  } catch {
    throw new StateBackupError("State file is corrupted and cannot be migrated.");
  }
  const result = migrateState(raw);
  if (!result.migrated) {
    return `No state migration is pending. Current schema version: ${STATE_VERSION}.`;
  }
  const legacy = result.legacy as Record<string, unknown>;
  const changes = [
    "schema version: legacy -> 1",
    "profile names: single wallet -> default",
    ...(legacy.wallet ? ["wallet fields: moved to profiles.default.wallet"] : []),
    ...(legacy.apiKey ? ["publisher API key: moved to profiles.default.apiKey"] : []),
  ];
  return [
    "Migration preview (read-only).",
    `Migration version: legacy single-wallet -> profile schema v${STATE_VERSION}.`,
    `Changes: ${changes.join("; ")}.`,
    `Backup: will be created as a mode-0600 file before applying.`,
    "No state was changed.",
  ].join("\n");
}

export function persistStateWithMigrationBackup(state: ProfileState): string | null {
  let backupPath: string | null = null;
  if (existsSync(STATE_FILE)) {
    try {
      const rawText = readFileSync(STATE_FILE, "utf8");
      const result = migrateState(JSON.parse(rawText) as unknown);
      if (result.migrated) {
        backupPath =
          `${STATE_FILE}.migration-v${STATE_VERSION}-${Date.now()}-` +
          `${randomBytes(4).toString("hex")}.backup`;
        writeAtomically(backupPath, rawText, 0o600);
      }
    } catch (err) {
      if (err instanceof StateBackupError) throw err;
      throw new StateBackupError(`Could not prepare state migration backup: ${err}`);
    }
  }
  try {
    writeAtomically(STATE_FILE, JSON.stringify(state, null, 2), 0o600);
  } catch (err) {
    throw new StateBackupError(
      `State migration was not applied; the original file is intact: ${err}`,
    );
  }
  return backupPath;
}

/**
 * Write bytes to a temp file in the same directory and atomically rename it over
 * the destination to avoid partial writes. The destination is never left half-
 * written when the rename fails, and the file retains the requested mode.
 */
export function writeAtomically(
  filePath: string,
  data: string,
  mode: number,
  ops: Partial<typeof import("fs")> = {},
): void {
  const dir = dirname(filePath);
  mkdirSync(dir, { recursive: true });

  const tempPath = `${filePath}.tmp-${process.pid}-${Date.now()}-${randomBytes(4).toString("hex")}`;
  const writeSync = ops.writeFileSync ?? writeFileSync;
  const rename = ops.renameSync ?? renameSync;
  const chmod = ops.chmodSync ?? chmodSync;
  const pathExists = ops.existsSync ?? existsSync;
  const remove = ops.unlinkSync ?? unlinkSync;

  try {
    writeSync(tempPath, data, { mode });
    chmod(tempPath, mode);
    rename(tempPath, filePath);
  } catch (err) {
    try {
      if (pathExists(tempPath)) remove(tempPath);
    } catch {
      // ignore cleanup errors; the caller already received the write failure
    }
    throw err;
  }
}

/**
 * Persist a restored ProfileState to disk (mode 0600), mirroring saveState in
 * index.ts. Exposed for testability.
 */
export function persistState(state: ProfileState): void {
  try {
    writeAtomically(STATE_FILE, JSON.stringify(state, null, 2), 0o600);
  } catch (err) {
    throw new StateBackupError(`Failed to persist restored state: ${err}`);
  }
}

/**
 * Preserve an unreadable or structurally invalid state file instead of letting
 * the next `saveState()` overwrite the only copy of it.
 *
 * The corrupted file is moved aside to `<file>.corrupt-<now-ms>` (same owner
 * and mode as the original) so the evidence is never lost and the live path is
 * clean for a fresh start. Returns the quarantine path when it worked and
 * throws otherwise.
 */
export function quarantineStateFile(
  filePath: string = STATE_FILE,
  now: number = Date.now(),
): string {
  const quarantinePath = `${filePath}.corrupt-${now}`;
  try {
    mkdirSync(dirname(filePath), { recursive: true });
    renameSync(filePath, quarantinePath);
    return quarantinePath;
  } catch (err) {
    throw new StateBackupError(`Failed to quarantine corrupted state file: ${err}`);
  }
}

/**
 * Snapshot the pre-migration legacy state before it is re-persisted, so a later
 * migration cannot destroy the only record of the original format.
 *
 * The legacy JSON is written to `<file>.legacy` (mode 0600, like the state file
 * itself). Throws when the snapshot could not be saved so the caller can decide
 * whether to proceed with the migration.
 */
export function preserveLegacyState(raw: unknown, filePath: string = STATE_FILE): void {
  try {
    mkdirSync(dirname(filePath), { recursive: true });
    writeFileSync(`${filePath}.legacy`, JSON.stringify(raw, null, 2), { mode: 0o600 });
  } catch (err) {
    throw new StateBackupError(`Failed to preserve legacy state before migration: ${err}`);
  }
}

// ── State file permission checks ────────────────────────────────────────────

export interface StatePermissionResult {
  exists: boolean;
  mode: string | null;
  expectedMode: string;
  isSafe: boolean;
  message: string;
}

/**
 * Format a numeric mode as a 4-digit octal string (e.g. 0o600 → "0600").
 */
function formatMode(mode: number): string {
  return "0" + (mode & 0o7777).toString(8).padStart(3, "0");
}

/**
 * Check the state file's permissions. Returns whether the file exists, its
 * current mode, whether it matches the expected 0600, and an actionable message.
 *
 * The state file contains wallet secret keys and publisher API keys, so it
 * must be mode 0600 (owner read/write only). Broader permissions (e.g. 0644)
 * allow other users on the system to read the secrets.
 */
export function checkStatePermissions(): StatePermissionResult {
  const EXPECTED = 0o600;
  const expectedStr = formatMode(EXPECTED);

  if (!existsSync(STATE_FILE)) {
    return {
      exists: false,
      mode: null,
      expectedMode: expectedStr,
      isSafe: true,
      message: `State file ${STATE_FILE} does not exist. No secrets to protect.`,
    };
  }

  try {
    const stat = statSync(STATE_FILE);
    // stat.mode includes file type bits; mask to permission bits only.
    const permBits = stat.mode & 0o7777;
    const currentStr = formatMode(permBits);
    const isSafe = permBits === EXPECTED;

    if (isSafe) {
      return {
        exists: true,
        mode: currentStr,
        expectedMode: expectedStr,
        isSafe: true,
        message: `State file permissions are safe (${currentStr}).`,
      };
    }

    // Build an actionable warning for unsafe modes.
    const othersRead = (permBits & 0o004) !== 0;
    const groupRead = (permBits & 0o040) !== 0;
    const issues: string[] = [];
    if (othersRead) issues.push("world-readable (other can read)");
    if (groupRead) issues.push("group-readable");
    if ((permBits & 0o002) !== 0) issues.push("world-writable");
    if ((permBits & 0o020) !== 0) issues.push("group-writable");

    return {
      exists: true,
      mode: currentStr,
      expectedMode: expectedStr,
      isSafe: false,
      message: [
        `State file permissions are UNSAFE (${currentStr}): ${issues.join(", ")}.`,
        `Wallet secret keys and API keys may be readable by other users.`,
        `Fix: chmod ${expectedStr} ${STATE_FILE}`,
      ].join("\n"),
    };
  } catch (err) {
    return {
      exists: true,
      mode: null,
      expectedMode: expectedStr,
      isSafe: false,
      message: `Could not read state file permissions: ${err}`,
    };
  }
}
