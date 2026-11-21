# MCP Tools

MindVault exposes a set of read-only and write tools over the Model Context Protocol (MCP) so agents can inspect on-chain state and prepare transactions.

## Read-only tools

| Tool | Description |
| --- | --- |
| `mindvault_get_vault` | Read a vault's metadata and balances. |
| `mindvault_list_vaults` | List vaults owned by an address. |
| `mindvault_fee_config` | Read the platform and royalty fee configuration. |

### `mindvault_fee_config`

Wraps the `get_fee_config` contract read and returns the current platform and royalty fee configuration so agents can quote the correct creator payout before setting a price.

**Inputs:** none.

**Response:**

```json
{
  "platformFeeBps": 250,
  "royaltyFeeBps": 500,
  "treasury": "0x0000000000000000000000000000000000000000"
}
```

- `platformFeeBps` — platform fee in basis points (1 bps = 0.01%).
- `royaltyFeeBps` — creator royalty fee in basis points.
- `treasury` — address that receives the platform fee.

This tool is read-only and does not modify any on-chain state. It is registered alongside the existing MCP tools without changing their names, schemas, or client compatibility.
