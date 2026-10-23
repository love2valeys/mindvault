/**
 * Snapshot tests for MCP tool metadata (the ListTools response).
 *
 * Verifies that the tool list exposed to agent clients stays deterministic and
 * complete. Snapshots capture the shape of the most commonly used tools
 * (mindvault_search, mindvault_publish) to prevent regressions when updating
 * descriptions or examples.
 *
 * Full ListTools coverage through the SDK lives in `integration.test.ts`.
 */
import { describe, it, expect } from "vitest";
import { TOOL_DEFINITIONS } from "./tools.js";
import { catalogFilterInputProperties } from "./catalogFilters.js";
import { TEXT_ONLY_TOOLS } from "./outputSchemas.js";

type ToolSideEffect = "none" | "local-write" | "external-write" | "payment" | "destructive";

const TOOL_SIDE_EFFECTS = {
  mindvault_setup_wallet: "local-write",
  mindvault_wallet_info: "none",
  mindvault_repair_sponsored_account: "local-write",
  mindvault_use_profile: "local-write",
  mindvault_list_profiles: "none",
  mindvault_switch_network_profile: "local-write",
  mindvault_browse: "local-write",
  mindvault_search: "local-write",
  mindvault_preview: "none",
  mindvault_compare_catalogs: "none",
  mindvault_register: "external-write",
  mindvault_publish: "payment",
  mindvault_buy: "payment",
  mindvault_buy_lease: "payment",
  mindvault_lease_status: "none",
  mindvault_export_receipts: "none",
  mindvault_register_onchain: "external-write",
  mindvault_agent_status: "none",
  mindvault_registry_info: "none",
  mindvault_contract_preflight: "none",
  mindvault_terms: "external-write",
  mindvault_network_profile: "none",
  mindvault_check_bindings: "none",
  mindvault_check_consistency: "none",
  mindvault_verify_attestation: "none",
  mindvault_registry_lookup: "none",
  mindvault_registry_list: "none",
  mindvault_registry_count: "none",
  mindvault_tx_status: "none",
  mindvault_reset: "destructive",
  mindvault_backup_state: "local-write",
  mindvault_resource_provenance: "none",
  mindvault_resource_change_log: "none",
  mindvault_restore_state: "destructive",
  mindvault_metrics: "local-write",
  mindvault_set_tags: "external-write",
  mindvault_update_metadata: "external-write",
  mindvault_set_price: "external-write",
  mindvault_transfer_ownership: "external-write",
  mindvault_accept_transfer: "external-write",
  mindvault_cancel_transfer: "external-write",
  mindvault_pending_transfer: "none",
  mindvault_set_listed: "external-write",
  mindvault_freeze: "destructive",
  mindvault_fee_config: "none",
  mindvault_royalty: "external-write",
  mindvault_check_state_permissions: "none",
  mindvault_registry_health: "none",
  mindvault_prewarm_catalog: "local-write",
  mindvault_client_config: "none",
  mindvault_mainnet_banner: "none",
  mindvault_import_wallet: "local-write",
  mindvault_rotate_publisher_key: "external-write",
  mindvault_verify_install: "none",
  mindvault_debug_bundle: "none",
  mindvault_support_bundle: "none",
  mindvault_recover_catalog_cache: "local-write",
  mindvault_publish_status: "none",
  mindvault_purchase_history: "none",
  mindvault_purchase_status: "none",
  mindvault_publish_batch: "payment",
} as const satisfies Record<string, ToolSideEffect>;

const NON_IDEMPOTENT_TOOLS = [
  "mindvault_setup_wallet",
  "mindvault_register",
  "mindvault_publish",
  "mindvault_buy",
  "mindvault_buy_lease",
  "mindvault_register_onchain",
  "mindvault_backup_state",
  "mindvault_restore_state",
  "mindvault_metrics",
  "mindvault_transfer_ownership",
  "mindvault_accept_transfer",
  "mindvault_cancel_transfer",
  "mindvault_import_wallet",
  "mindvault_rotate_publisher_key",
  "mindvault_publish_batch",
];

describe("MCP tool metadata", () => {
  it("all tools have required fields", () => {
    for (const tool of TOOL_DEFINITIONS) {
      expect(tool.name).toMatch(/^mindvault_/);
      expect(typeof tool.description).toBe("string");
      expect(tool.description.length).toBeGreaterThan(0);
      expect(tool.inputSchema.type).toBe("object");
    }
  });

  it("every tool declares MCP annotations with a title (#552)", () => {
    for (const tool of TOOL_DEFINITIONS) {
      const annotations = tool.annotations;
      expect(annotations, `tool ${tool.name} has annotations`).toBeDefined();
      expect(typeof annotations.title, `tool ${tool.name} has a title`).toBe("string");
      expect(annotations.title.length).toBeGreaterThan(0);
      expect(typeof annotations.readOnlyHint).toBe("boolean");
      expect(typeof annotations.destructiveHint).toBe("boolean");
      expect(typeof annotations.idempotentHint).toBe("boolean");
    }
  });

  it("read-only tools are not marked destructive (#552)", () => {
    const readOnlyTools = TOOL_DEFINITIONS.filter((tool) => tool.annotations.readOnlyHint);
    expect(readOnlyTools.length).toBeGreaterThan(0);
    for (const tool of readOnlyTools) {
      expect(
        tool.annotations.destructiveHint,
        `read-only tool ${tool.name} is not destructive`,
      ).toBe(false);
    }
  });

  it("destructive tools are not marked read-only (#552)", () => {
    const destructiveTools = TOOL_DEFINITIONS.filter((tool) => tool.annotations.destructiveHint);
    expect(destructiveTools.map((t) => t.name)).toEqual(
      expect.arrayContaining(["mindvault_reset", "mindvault_restore_state"]),
    );
    for (const tool of destructiveTools) {
      expect(tool.annotations.readOnlyHint, `destructive tool ${tool.name} is not read-only`).toBe(
        false,
      );
    }
  });

  it("matches every tool annotation to its audited handler side effects (#109)", () => {
    expect(Object.keys(TOOL_SIDE_EFFECTS).sort()).toEqual(
      TOOL_DEFINITIONS.map((tool) => tool.name).sort(),
    );

    for (const tool of TOOL_DEFINITIONS) {
      const effect = TOOL_SIDE_EFFECTS[tool.name as keyof typeof TOOL_SIDE_EFFECTS];
      expect(tool.annotations.readOnlyHint, `${tool.name} read-only effect`).toBe(effect === "none");
      expect(tool.annotations.destructiveHint, `${tool.name} destructive effect`).toBe(
        effect === "destructive",
      );
      expect(tool.annotations.idempotentHint, `${tool.name} idempotency`).toBe(
        !(NON_IDEMPOTENT_TOOLS as readonly string[]).includes(tool.name),
      );
      if (effect === "payment") {
        expect(tool.annotations.readOnlyHint, `${tool.name} can charge funds`).toBe(false);
      }
    }
  });

  it("exposes the expected tool surface", () => {
    expect(TOOL_DEFINITIONS.map((t) => t.name)).toMatchSnapshot();
  });

  it("mindvault_search inputSchema", () => {
    const searchSchema = {
      type: "object",
      properties: { ...catalogFilterInputProperties },
      required: [],
    };

    expect(searchSchema).toMatchSnapshot();
  });

  it("structured tools declare outputSchema and text-only tools do not (#553)", () => {
    for (const tool of TOOL_DEFINITIONS) {
      if ((TEXT_ONLY_TOOLS as readonly string[]).includes(tool.name)) {
        expect(tool.outputSchema, `${tool.name} should stay text-only`).toBeUndefined();
      } else {
        expect(tool.outputSchema, `${tool.name} should advertise outputSchema`).toBeDefined();
      }
    }
  });

  it("mindvault_publish inputSchema", () => {
    // Snapshot the definition itself, not a copy of it: a hand-written literal
    // drifts from the real schema and then snapshots its own drift.
    const publish = TOOL_DEFINITIONS.find((t) => t.name === "mindvault_publish");
    expect(publish?.inputSchema).toMatchSnapshot();
  });
});
