# Views — Health Factor and Read-Only Position Queries

This document describes the view functions for user collateral value, debt value, health factor, and position summary. These are **read-only**, **gas-efficient** entry points for frontends and liquidation logic.

## Overview

| Function | Description |
|----------|-------------|
| `get_collateral_balance` | User's collateral balance (raw amount). |
| `get_debt_balance` | User's debt balance (principal + accrued interest). |
| `get_collateral_value` | Collateral value in common unit (e.g. USD 8 decimals). |
| `get_debt_value` | Debt value in common unit. |
| `get_health_factor` | Health factor (scaled 10000 = 1.0). |
| `get_user_position` | Full position summary (balances, values, health factor). |

All value and health-factor computations use the **admin-configured oracle** and **liquidation threshold**. If the oracle is not set, `get_collateral_value`, `get_debt_value`, and `get_health_factor` return `0` (and `get_user_position` returns zeros for value/HF fields).

---

## 1. `get_collateral_balance(user: Address) -> i128`

- **Purpose:** Returns the user's collateral balance in raw units (same as `get_user_collateral(user).amount`).
- **Read-only:** Yes. No state changes.
- **Returns:** Collateral amount. `0` if the user has no collateral.

---

## 2. `get_debt_balance(user: Address) -> i128`

- **Purpose:** Returns the user's total debt: principal + accrued interest.
- **Read-only:** Yes. No state changes.
- **Returns:** Total debt in raw units. `0` if the user has no debt.

---

## 3. `get_collateral_value(user: Address) -> i128`

- **Purpose:** Collateral value in a common unit (e.g. USD with 8 decimals), using the configured oracle.
- **Read-only:** Yes. Only reads from storage and calls the oracle (read-only from the protocol’s perspective).
- **Returns:** `collateral_amount * oracle_price / PRICE_SCALE`. `0` if oracle is not set, collateral is zero, or price is invalid.
- **Oracle:** Must be set via `set_oracle(admin, oracle_address)`. Oracle contract must implement `price(asset: Address) -> i128` with 8-decimal scale (`PRICE_SCALE = 100_000_000`).

---

## 4. `get_debt_value(user: Address) -> i128`

- **Purpose:** Debt value in the same common unit as collateral value.
- **Read-only:** Yes.
- **Returns:** `(principal + interest) * oracle_price / PRICE_SCALE`. `0` if oracle is not set or no debt.
- **Oracle:** Same as `get_collateral_value`.

---

## 5. `get_health_factor(user: Address) -> i128`

- **Purpose:** Health factor for liquidations and UI. Computed from collateral value, debt value, and liquidation threshold.
- **Read-only:** Yes.
- **Formula:**  
  `health_factor = (collateral_value * liquidation_threshold_bps / 10000) * HEALTH_FACTOR_SCALE / debt_value`  
  with `HEALTH_FACTOR_SCALE = 10000`, so **10000 = 1.0**.
- **Interpretation:**
  - **> 10000:** Healthy (above liquidation threshold).
  - **< 10000:** Liquidatable.
  - **= 10000:** Boundary (at liquidation threshold).
- **Special values:**
  - No debt: returns `HEALTH_FACTOR_NO_DEBT` (e.g. 100_000_000), meaning “healthy”.
  - Oracle not set or values not computable: returns `0`.
- **Liquidation threshold:** Set by admin via `set_liquidation_threshold_bps(admin, bps)`. Example: `8000` = 80%. Must be in `(0, 10000]`.

---

## 6. `get_user_position(user: Address) -> UserPositionSummary`

- **Purpose:** Single-call summary for frontends and liquidators.
- **Read-only:** Yes.
- **Returns:** A struct with:
  - `collateral_balance: i128`
  - `collateral_value: i128`
  - `debt_balance: i128`
  - `debt_value: i128`
  - `health_factor: i128`

All fields match the corresponding individual getters.

---

## Admin Configuration

- **`set_oracle(admin, oracle: Address)`**  
  Sets the price oracle contract (admin-only). Required for non-zero collateral/debt value and for health factor.

- **`set_liquidation_threshold_bps(admin, bps: i128)`**  
  Sets the liquidation threshold in basis points (admin-only). Must be `0 < bps <= 10000`. Example: `8000` = 80%.

---

## Security Assumptions

1. **No state change:** All view functions only read storage and call the oracle. They do not modify protocol or user state.
2. **Oracle usage:** Values and health factor depend on the admin-configured oracle. Oracle is trusted; a malicious or faulty oracle can report wrong prices and thus wrong health factors.
3. **Liquidation threshold:** Only admin can set it. It is used consistently in the health factor formula.
4. **Overflow:** Value and health factor calculations use checked arithmetic where applicable; edge cases (e.g. zero debt) are handled explicitly.

---

## Gas and Usage

- Views are designed to be callable without authorization and without changing state, so they are suitable for read-only RPC calls and UIs.
- `get_user_position` aggregates one read of collateral, one of debt, and up to two oracle calls (collateral and debt assets), so it is more gas-efficient than calling the four value/HF getters separately when you need the full summary.

---

## View Guarantees (cross-asset position summary invariants)

The view layer is a load-bearing surface for liquidation bots, frontends, and
downstream contracts. The following guarantees are pinned by the invariant
suite in `stellar-lend/contracts/lending/src/views_test.rs` and must never be
weakened without an explicit, audited change.

### G1. Summary–getter consistency

`get_user_position(user)` must return field-for-field exactly what the
individual getters return for the same `user` at the same ledger height:

- `summary.collateral_balance == get_collateral_balance(user)`
- `summary.debt_balance == get_debt_balance(user)`
- `summary.collateral_value == get_collateral_value(user)`
- `summary.debt_value == get_debt_value(user)`
- `summary.health_factor == get_health_factor(user)`

### G2. Stable serialization (idempotence)

The view output is a pure function of `(storage, oracle, ledger height)`.
Repeated calls in any order must yield bit-identical results — no view path
may mutate state, cache stale derived values, or depend on call order.

### G3. Threshold isolation

Changing `liquidation_threshold_bps` may move `health_factor` but must not
move any of `collateral_balance`, `collateral_value`, `debt_balance`, or
`debt_value`. Those four are functions of raw state and oracle output only.

### G4. Missing-asset and missing-oracle behaviour

- A user with no recorded position returns a default summary: zero balances,
  zero values, and `health_factor == HEALTH_FACTOR_NO_DEBT`.
- When the oracle is unconfigured, every value-bearing field reads as `0`
  consistently. Raw balance fields remain exact and non-zero. The contract
  refuses to emit a non-zero `health_factor` without price data so liquidators
  cannot act on stale assumptions.

### G5. Rounding semantics

Health-factor division truncates toward zero. The boundary case
`health_factor == HEALTH_FACTOR_SCALE` (exactly 1.0) is treated as healthy:
`get_max_liquidatable_amount` returns `0` here. Any refactor that switches to
ceiling rounding or float math will break the invariant suite.

### G6. Liquidation-incentive monotonicity

`get_liquidation_incentive_amount(repay)` is monotonic non-decreasing in
`repay`. Negative or zero `repay` always yields `0`. This forbids a future
incentive curve that liquidators could game by splitting repayments.

### G7. Independence across users

Each user's summary depends only on that user's positions and the global
risk parameters. There is no cross-user contamination — pinned by the
"independent users" invariant test.

### Security: no view-based exploitation assumptions

- Views never mutate state, never charge fees, and never trigger external
  contract calls beyond the read-only oracle lookup. Callers may safely
  invoke them off-chain.
- The protocol separately enforces a withdraw invariant: users may not
  withdraw more collateral than they own, and withdrawals that would leave
  collateral below the minimum collateral ratio (currently 1.0, or 100%) are rejected.
- Integrators MUST NOT rely on a view's value beyond the ledger height at
  which it was observed. Oracle prices and risk parameters can change.

---

## Example Commit Message

```
feat: implement health factor and view functions with tests and docs
```

---

## 7. `get_protocol_metrics() -> ProtocolMetrics`

- **Purpose:** Returns a single consistent protocol-wide snapshot for off-chain consumers (APIs, dashboards, liquidation bots). Replaces any per-user aggregation in the off-chain layer with a single O(1) contract read.
- **Read-only:** Yes. No state changes.
- **Returns:** A `ProtocolMetrics` struct with:
  - `total_supply: i128` — Total collateral deposited across all users (sourced from the `TotalDeposits` aggregate key, updated on every `deposit`).
  - `total_borrow: i128` — Total debt principal outstanding across all users (sourced from the `TotalDebt` aggregate key, incremented on `borrow` and decremented on `repay`).
  - `utilization_bps: i128` — Utilization rate in basis points: `(total_borrow × 10_000) / total_supply`. Returns `0` when `total_supply` is zero.
  - `ledger: u32` — Ledger sequence number at the moment the view was evaluated, allowing callers to detect stale reads.

### Field semantics

| Field | Type | Scale | Notes |
|-------|------|-------|-------|
| `total_supply` | `i128` | raw units | Sum of all `deposit` calls minus `withdraw` calls |
| `total_borrow` | `i128` | raw units | Sum of active debt principals; does **not** include accrued interest |
| `utilization_bps` | `i128` | BPS (0–10 000) | 10 000 = 100 %. Clamped to 0 when supply is 0 |
| `ledger` | `u32` | ledger seq | Use to detect cross-request inconsistency |

### Security and consistency

- **Atomic snapshot:** All four fields are read within a single contract invocation, so they are internally consistent at the same ledger height.
- **No interest accrual:** `total_borrow` tracks debt principal only. It will not equal the sum of `get_debt_balance` values (which include accrued interest). This is intentional; utilization is driven by principal.
- **Concurrent-mutation safety:** Aggregate keys (`TotalDeposits`, `TotalDebt`) are updated transactionally inside `deposit`, `withdraw`, `borrow`, and `repay`, so there is no race between concurrent mutators on separate Soroban ledger closures.
- **Integrators MUST NOT** cache the result beyond the ledger at which it was read — `total_supply` and `total_borrow` can change each ledger.

### Example

```rust
let metrics = client.get_protocol_metrics();
// metrics.total_supply    => 1_000_000
// metrics.total_borrow    =>   500_000
// metrics.utilization_bps =>     5_000  // 50 %
// metrics.ledger          =>    123_456
```

