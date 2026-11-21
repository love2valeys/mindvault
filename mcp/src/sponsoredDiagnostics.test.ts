import { describe, it, expect } from "vitest";

import { mapTransportError } from "./errorMapping.js";
import {
  diagnoseSponsoredOutage,
  diagnoseSponsoredWallet,
  formatSponsoredOutage,
  formatSponsoredWalletDiagnostic,
  mapSponsoredHttpFailure,
  mapSponsoredTransportFailure,
  parseRetryAfter,
  sanitizeServiceUrl,
  sponsoredFailureDetail,
} from "./sponsoredDiagnostics.js";

const SERVICE = "https://stellar-sponsored-agent-account.onrender.com";
const OPERATION = "mindvault_setup_wallet failed to create wallet";

describe("sanitizeServiceUrl", () => {
  it("keeps a plain service URL intact", () => {
    expect(sanitizeServiceUrl(SERVICE)).toBe(SERVICE);
  });

  it("strips embedded credentials, query, and fragment", () => {
    const sanitized = sanitizeServiceUrl(
      "https://user:hunter2@sponsor.example.com/?token=abc#frag",
    );
    expect(sanitized).not.toContain("hunter2");
    expect(sanitized).not.toContain("token");
    expect(sanitized).not.toContain("frag");
    expect(sanitized).toContain("sponsor.example.com");
  });

  it("drops a trailing slash so the endpoint reads cleanly", () => {
    expect(sanitizeServiceUrl("https://sponsor.example.com/")).toBe("https://sponsor.example.com");
  });

  it("reports an unparseable URL instead of echoing it", () => {
    expect(sanitizeServiceUrl("not a url")).toBe("(invalid SPONSORED_ACCOUNT_URL)");
  });
});

describe("sponsoredFailureDetail", () => {
  it("quotes a recognized message field", () => {
    expect(sponsoredFailureDetail({ error: "service temporarily unavailable" })).toBe(
      "service temporarily unavailable",
    );
  });

  it("withholds the values of an unrecognized error body", () => {
    const detail = sponsoredFailureDetail({
      internalErrorCode: "SPONSOR_DB_FAILED",
      debugStackTrace: "at Function.doSomething...",
    });
    expect(detail).not.toContain("SPONSOR_DB_FAILED");
    expect(detail).not.toContain("at Function");
    expect(detail).toContain("2 fields withheld");
  });

  it("counts a single withheld field in the singular", () => {
    expect(sponsoredFailureDetail({ traceId: "abc" })).toContain("1 field withheld");
  });

  it("describes an empty or absent body", () => {
    expect(sponsoredFailureDetail(null)).toBe("no response body");
    expect(sponsoredFailureDetail("   ")).toBe("empty response body");
  });

  it("redacts a Stellar secret key that leaked into a message", () => {
    const secret = `S${"A".repeat(55)}`;
    const detail = sponsoredFailureDetail({ error: `could not fund ${secret}` });
    expect(detail).not.toContain(secret);
    expect(detail).toContain("REDACTED");
  });

  it("bounds a long message so one reply cannot flood the agent", () => {
    const detail = sponsoredFailureDetail({ error: "x".repeat(5000) });
    expect(detail.length).toBeLessThan(250);
    expect(detail.endsWith("…")).toBe(true);
  });
});

describe("parseRetryAfter", () => {
  it("reads a delay in seconds", () => {
    expect(parseRetryAfter({ "retry-after": "30" })).toBe(30);
  });

  it("reads an HTTP date relative to now", () => {
    const now = Date.parse("2026-01-01T00:00:00Z");
    const at = new Date(now + 45_000).toUTCString();
    expect(parseRetryAfter({ "retry-after": at }, now)).toBe(45);
  });

  it("ignores a header that is absent, unparseable, or in the past", () => {
    const now = Date.parse("2026-01-01T00:00:00Z");
    expect(parseRetryAfter(undefined)).toBeUndefined();
    expect(parseRetryAfter({ "retry-after": "soon" })).toBeUndefined();
    expect(parseRetryAfter({ "retry-after": "" })).toBeUndefined();
    expect(
      parseRetryAfter({ "retry-after": new Date(now - 60_000).toUTCString() }, now),
    ).toBeUndefined();
  });

  it("ignores an implausibly distant delay", () => {
    expect(parseRetryAfter({ "retry-after": "999999999" })).toBeUndefined();
  });
});

describe("diagnoseSponsoredOutage", () => {
  it("classifies an unreachable service as not reachable and retryable", () => {
    const outage = diagnoseSponsoredOutage({ serviceUrl: SERVICE, category: "network" });
    expect(outage.kind).toBe("unreachable");
    expect(outage.reachable).toBe(false);
    expect(outage.retryable).toBe(true);
    expect(outage.status).toBeUndefined();
    expect(outage.guidance.join(" ")).toMatch(/network|connect/i);
  });

  it("classifies a transport timeout separately from an unreachable host", () => {
    const outage = diagnoseSponsoredOutage({ serviceUrl: SERVICE, category: "timeout" });
    expect(outage.kind).toBe("timeout");
    expect(outage.guidance.join(" ")).toContain("MINDVAULT_HTTP_TIMEOUT_MS");
  });

  it.each([502, 503, 504])("treats %i as the service being unavailable", (status) => {
    const outage = diagnoseSponsoredOutage({
      serviceUrl: SERVICE,
      category: status === 504 ? "timeout" : "server",
      status,
    });
    expect(outage.kind).toBe("unavailable");
    expect(outage.reachable).toBe(true);
    expect(outage.retryable).toBe(true);
    expect(outage.guidance.join(" ")).toMatch(/restarting/);
  });

  it("classifies 429 as rate limited and surfaces Retry-After", () => {
    const outage = diagnoseSponsoredOutage({
      serviceUrl: SERVICE,
      category: "rate_limit",
      status: 429,
      headers: { "retry-after": "12" },
    });
    expect(outage.kind).toBe("rate_limited");
    expect(outage.retryAfterSeconds).toBe(12);
    expect(outage.guidance.join(" ")).toContain("12s");
  });

  it("classifies 500 as a server error that is safe to retry", () => {
    const outage = diagnoseSponsoredOutage({
      serviceUrl: SERVICE,
      category: "server",
      status: 500,
    });
    expect(outage.kind).toBe("server_error");
    expect(outage.retryable).toBe(true);
  });

  it("marks a 4xx rejection as not retryable", () => {
    const outage = diagnoseSponsoredOutage({
      serviceUrl: SERVICE,
      category: "validation",
      status: 400,
    });
    expect(outage.kind).toBe("rejected");
    expect(outage.retryable).toBe(false);
    expect(outage.guidance.join(" ")).toMatch(/malformed/);
  });

  it("points a 404 at the SPONSORED_ACCOUNT_URL configuration", () => {
    const outage = diagnoseSponsoredOutage({
      serviceUrl: SERVICE,
      category: "not_found",
      status: 404,
    });
    expect(outage.guidance.join(" ")).toContain("SPONSORED_ACCOUNT_URL");
    expect(outage.guidance.join(" ")).toContain("/create");
  });

  it("explains a 401 as a deployment that wants credentials", () => {
    const outage = diagnoseSponsoredOutage({
      serviceUrl: SERVICE,
      category: "auth",
      status: 401,
    });
    expect(outage.retryable).toBe(false);
    expect(outage.guidance.join(" ")).toMatch(/credentials/i);
  });

  it("never leaves an answered status without guidance", () => {
    for (const status of [400, 401, 403, 404, 409, 422, 429, 500, 502, 503, 504]) {
      const outage = diagnoseSponsoredOutage({
        serviceUrl: SERVICE,
        category: "server",
        status,
      });
      expect(outage.guidance.length, `status ${status} has no guidance`).toBeGreaterThan(0);
    }
  });

  it("is deterministic for the same input", () => {
    const input = { serviceUrl: SERVICE, category: "server" as const, status: 503 };
    expect(diagnoseSponsoredOutage(input)).toEqual(diagnoseSponsoredOutage(input));
  });
});

describe("formatSponsoredOutage", () => {
  it("keeps the labels the tool contract already reports", () => {
    const line = formatSponsoredOutage(
      diagnoseSponsoredOutage({ serviceUrl: SERVICE, category: "server", status: 500 }),
    );
    expect(line).toContain("Service:");
    expect(line).toContain("Status:");
    expect(line).toContain("Issue:");
    expect(line).toContain("Endpoint: POST /create");
    expect(line).toContain("Reachable: yes");
    expect(line).toContain("Retryable: yes");
  });

  it("omits the status when the service never answered", () => {
    const line = formatSponsoredOutage(
      diagnoseSponsoredOutage({ serviceUrl: SERVICE, category: "network" }),
    );
    expect(line).not.toContain("Status:");
    expect(line).toContain("Reachable: no");
  });
});

describe("mapSponsoredHttpFailure", () => {
  it("reports the operation, the detail, and the diagnostics line", () => {
    const mapped = mapSponsoredHttpFailure({
      operation: OPERATION,
      serviceUrl: SERVICE,
      status: 503,
      data: { error: "service temporarily unavailable" },
    });
    expect(mapped.source).toBe("sponsored");
    expect(mapped.status).toBe(503);
    expect(mapped.summary).toContain(OPERATION);
    expect(mapped.summary).toContain("service temporarily unavailable");
    expect(mapped.summary).toContain("Issue: unavailable");
    expect(mapped.action).toMatch(/restarting/);
  });

  it("does not leak an unrecognized error body into the summary", () => {
    const mapped = mapSponsoredHttpFailure({
      operation: OPERATION,
      serviceUrl: SERVICE,
      status: 500,
      data: { internalErrorCode: "SPONSOR_DB_FAILED", stack: "at Function.x" },
    });
    expect(mapped.summary).not.toContain("SPONSOR_DB_FAILED");
    expect(mapped.summary).not.toContain("at Function");
  });

  it("does not leak credentials embedded in the service URL", () => {
    const mapped = mapSponsoredHttpFailure({
      operation: OPERATION,
      serviceUrl: "https://ops:s3cret@sponsor.example.com",
      status: 500,
      data: { error: "boom" },
    });
    expect(mapped.summary).not.toContain("s3cret");
  });
});

describe("mapSponsoredTransportFailure", () => {
  it("diagnoses a refused connection, which never reaches the HTTP path", () => {
    const mapped = mapSponsoredTransportFailure({
      operation: OPERATION,
      serviceUrl: SERVICE,
      mapped: mapTransportError({
        operation: "Sponsored-account request failed",
        source: "sponsored",
        error: new Error("ECONNREFUSED: Connection refused"),
      }),
    });
    expect(mapped.category).toBe("network");
    expect(mapped.status).toBeUndefined();
    expect(mapped.summary).toContain(OPERATION);
    expect(mapped.summary).toContain("ECONNREFUSED");
    expect(mapped.summary).toContain("Reachable: no");
    expect(mapped.action).toMatch(/network|connect/i);
  });

  it("diagnoses an abort as a timeout rather than an unreachable host", () => {
    const error = new Error("The operation was aborted");
    error.name = "AbortError";
    const mapped = mapSponsoredTransportFailure({
      operation: OPERATION,
      serviceUrl: SERVICE,
      mapped: mapTransportError({
        operation: "Sponsored-account request failed",
        source: "sponsored",
        error,
      }),
    });
    expect(mapped.category).toBe("timeout");
    expect(mapped.summary).toContain("Issue: timeout");
  });
});

describe("diagnoseSponsoredWallet – half-completed sponsored creation (#218)", () => {
  const PUBLIC_A = `G${"A".repeat(55)}`;
  const PUBLIC_B = `G${"B".repeat(55)}`;
  const SECRET_A = `S${"A".repeat(55)}`;
  const SECRET_B = `S${"B".repeat(55)}`;

  const derive = (secret: string): string => {
    if (secret === SECRET_A) return PUBLIC_A;
    if (secret === SECRET_B) return PUBLIC_B;
    throw new Error("invalid seed");
  };

  it("flags a funded address with no persisted local wallet as stale and unowned", () => {
    const diag = diagnoseSponsoredWallet({
      reportedAddress: PUBLIC_A,
      localWallet: null,
      derivePublicKey: derive,
    });

    expect(diag.status).toBe("stale_unowned");
    expect(diag.owned).toBe(false);
    expect(diag.stale).toBe(true);
    expect(diag.reportedAddress).toBe(PUBLIC_A);
    expect(diag.localOwnedAddress).toBeNull();
    expect(diag.summary).toContain("not owned by the local keystore");

    const rendered = formatSponsoredWalletDiagnostic(diag);
    expect(rendered).toContain("Wallet status: stale_unowned");
    expect(rendered).toContain("Owned: no");
    expect(rendered).toContain("Stale: yes");
    expect(rendered).toContain(PUBLIC_A);
  });

  it("flags a stored profile whose secret belongs to a different account without leaking the secret", () => {
    const diag = diagnoseSponsoredWallet({
      localWallet: { publicKey: PUBLIC_A, secretKey: SECRET_B },
      derivePublicKey: derive,
    });

    expect(diag.status).toBe("stale_unowned");
    expect(diag.owned).toBe(false);
    expect(diag.stale).toBe(true);
    expect(diag.integrityCode).toBe("address_mismatch");
    expect(diag.reportedAddress).toBe(PUBLIC_A);
    expect(diag.localOwnedAddress).toBe(PUBLIC_B);

    const rendered = formatSponsoredWalletDiagnostic(diag);
    expect(rendered).not.toContain(SECRET_B);
    expect(rendered).toContain("NOT spendable");
  });

  it("flags a reported funded address that diverges from the locally persisted wallet", () => {
    const diag = diagnoseSponsoredWallet({
      reportedAddress: PUBLIC_A,
      localWallet: { publicKey: PUBLIC_B, secretKey: SECRET_B },
      derivePublicKey: derive,
    });

    expect(diag.status).toBe("stale_unowned");
    expect(diag.owned).toBe(false);
    expect(diag.stale).toBe(true);
    expect(diag.reportedAddress).toBe(PUBLIC_A);
    expect(diag.localOwnedAddress).toBe(PUBLIC_B);
  });

  it("reports owned when the local keystore derives the sponsored address", () => {
    const diag = diagnoseSponsoredWallet({
      reportedAddress: PUBLIC_A,
      localWallet: { publicKey: PUBLIC_A, secretKey: SECRET_A },
      derivePublicKey: derive,
    });

    expect(diag.status).toBe("owned");
    expect(diag.owned).toBe(true);
    expect(diag.stale).toBe(false);
    expect(diag.localOwnedAddress).toBe(PUBLIC_A);
    expect(diag.guidance).toEqual([]);
  });

  it("reports missing when neither a reported address nor a local wallet exists", () => {
    const diag = diagnoseSponsoredWallet({
      reportedAddress: null,
      localWallet: null,
      derivePublicKey: derive,
    });

    expect(diag.status).toBe("missing");
    expect(diag.owned).toBe(false);
    expect(diag.stale).toBe(false);
  });

  it("warns in mapSponsoredHttpFailure when a 504 error body references an unpersisted funded address", () => {
    const mapped = mapSponsoredHttpFailure({
      operation: OPERATION,
      serviceUrl: SERVICE,
      status: 504,
      data: { error: "timeout after funding", publicKey: PUBLIC_A },
    });

    expect(mapped.action).toContain(PUBLIC_A);
    expect(mapped.action).toContain("NOT persisted");
    expect(mapped.action).toContain("NOT owned by the local keystore");
  });
});
