# Contract Interface Quick Reference

> **Sync note**: This file must stay in sync with
> `stellar-lend/contracts/lending/src/lib.rs`. After any `pub fn` change in
> that file, update the tables below and run
> `bash docs/scripts/check_interface_sync.sh` to verify.

---

## 1. Unit Scales & Precisions

| Parameter | Scale | Description |
|-----------|-------|-------------|
| Amounts | raw `i128` | No automatic decimal shifting. Callers supply and receive raw integer amounts. |
| Health Factor | 10^4 base | `1.0 = 10000`. Values `< 10000` are eligible for liquidation. `1_000_000` is the sentinel for a debt-free position. |
| Basis Points (BPS) | 10^4 | `1% = 100 BPS`. Used for interest rates, fees, and risk thresholds. |
| Timestamps | Seconds | Unix epoch seconds from `env.ledger().timestamp()`. |

---

## 2. Implemented Function Reference

### Initialization

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `initialize` | `(admin: Address)` | — | `()` |
| `get_admin` | `()` | — | `Address` |
| `propose_admin` | `(new_admin: Address)` | current admin | `()` |
| `accept_admin` | `()` | proposed admin | `()` |

### User Operations

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `deposit` | `(user: Address, amount: i128)` | `user` | `i128` — new collateral balance |
| `withdraw` | `(user: Address, amount: i128)` | `user` | `i128` — new collateral balance |
| `borrow` | `(user: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — debt principal |
| `repay` | `(user: Address, amount: i128)` | `user` | `i128` — remaining debt principal |
| `liquidate` | `(liquidator: Address, borrower: Address, amount: i128)` | `liquidator` | `Result<i128, Error>` — debt actually repaid |

### Flash Loans

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `flash_loan` | `(receiver: Address, asset: Address, amount: i128, params: Bytes)` | `receiver` | `()` |
| `repay_flash_loan` | `(asset: Address, amount: i128)` | invoker (receiver contract) | `()` |

### View Functions

| Function | Signature | Returns |
|---|---|---|
| `get_position` | `(user: Address)` | `PositionSummary { collateral: i128, debt: i128, health_factor: i128 }` |
| `get_debt_position` | `(user: Address)` | `DebtPosition { principal: i128, last_accrual: u64 }` |
| `get_min_borrow` | `()` | `i128` |

### Admin & Risk Controls

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `set_min_borrow` | `(min_borrow: i128)` | admin | `Result<(), LendingError>` |
| `set_debt_ceiling` | `(ceiling: i128)` | admin | `Result<(), LendingError>` |
| `set_emergency_state` | `(new_state: EmergencyState)` | admin or guardian | `()` |

---

## 3. Return Types

### `PositionSummary`

```rust
pub struct PositionSummary {
    pub collateral: i128,    // Raw collateral balance
    pub debt: i128,          // Effective debt (principal + accrued interest)
    pub health_factor: i128, // (collateral * 8000) / debt; 1_000_000 if debt == 0
}
```

### `DebtPosition`

```rust
pub struct DebtPosition {
    pub principal: i128,    // Borrowed principal (before interest)
    pub last_accrual: u64,  // Timestamp of last interest calculation
}
```

### `EmergencyState`

```rust
pub enum EmergencyState {
    Normal,    // All operations permitted
    Shutdown,  // All operations blocked
    Recovery,  // Only repay and withdraw permitted
}
```

---

## 4. Error Codes

| Code | Variant | Meaning | Suggested UI Message |
|------|---------|---------|----------------------|
| 1008 | `LendingError::BelowMinimumBorrow` | Borrow amount below protocol minimum | "Amount is below the minimum borrow. Please increase your amount." |
| 1009 | `LendingError::NotInitialized` | Contract not yet initialized | "Contract is not ready. Contact the administrator." |
| 1010 | `LendingError::AlreadyInitialized` | `initialize` called twice | "Contract already initialized." |
| 2001 | `LendingError::DebtCeilingExceeded` | Borrow would exceed global debt ceiling | "Protocol debt limit reached. Try a smaller amount." |
| 2002 | `LendingError::DepositCapExceeded` | Deposit would exceed total cap | "Deposit cap reached. Try a smaller amount." |
| 2003 | `LendingError::Overflow` | Checked arithmetic would overflow | "Arithmetic error. Amount may be too large." |
| 2004 | `Error::PositionHealthy` | Liquidation rejected — position is healthy | "This position cannot be liquidated." |

---

## 5. Emergency State Permissions

| State | Deposit | Borrow | Repay | Withdraw | Liquidate |
|---|---|---|---|---|---|
| `Normal` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `Shutdown` | ❌ | ❌ | ❌ | ❌ | ❌ |
| `Recovery` | ❌ | ❌ | ✅ | ✅ | ❌ |

---

## 6. Events Emitted

| Event Topic | Payload | Emitted When |
|---|---|---|
| `EmergencyStateChanged` | `(old_state: EmergencyState, new_state: EmergencyState)` | `set_emergency_state` completes |

> Additional events for `deposit`, `borrow`, `repay`, `liquidate`, and `flash_loan` are **planned** but not yet emitted by the current implementation.

---

## 7. 🔮 Planned — Not Yet Implemented

The following functions and events are **not** present in `src/lib.rs` and should not be called. They are documented here for roadmap visibility.

| Function / Event | Tracking |
|---|---|
| `get_health_factor(user)` | Planned — today embedded in `get_position` |
| `get_emergency_state()` | Planned public view (state visible via events today) |
| `set_guardian(admin, guardian)` | Planned setter for guardian role |
| `set_oracle(admin, oracle)` | Planned — required for multi-asset health factor |
| `set_liquidation_threshold_bps(admin, bps)` | Planned — currently hardcoded 8000 BPS |
| `set_close_factor_bps(admin, bps)` | Planned — currently hardcoded 5000 BPS |
| `set_pause(admin, pause_type, paused)` | Planned granular per-operation pause |
| `get_collateral_value(user)` | Planned — requires oracle |
| `get_debt_value(user)` | Planned — requires oracle |
| `get_max_liquidatable_amount(user)` | Planned convenience helper |
| `upgrade_*` functions | Planned multisig upgrade governance |
| `data_*` functions | Planned persistent data-store management |
| `BorrowEvent`, `RepayEvent`, `LiquidationEvent` | Planned contract events |

---

## 8. Integration Checklist

- [ ] Use raw `i128` for all amounts — no automatic decimal conversion.
- [ ] Call `get_position(user)` before allowing further borrows; check `health_factor >= 10000`.
- [ ] Ensure wallet connector handles `user.require_auth()` for all user-facing calls.
- [ ] Confirm `EmergencyState::Normal` before presenting deposit / borrow UI to users.
- [ ] Flash loan receivers must implement an `on_flash_loan(initiator, asset, amount, fee, params)` endpoint.
