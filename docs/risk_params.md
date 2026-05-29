# Risk Parameters

This document provides a consolidated view of all protocol risk parameters, including their purpose, constraints, and how they are configured.

> Verified against contract constants and admin setter constraints where applicable.


## Risk Parameters Table

| Parameter | Meaning | Default | Bounds | Setter Function | Rationale |
|----------|--------|--------|--------|----------------|-----------|
| Close Factor | Maximum portion of a borrow position that can be liquidated in a single transaction | Defined in code | 0% – 100% | Admin-controlled setter | Prevents full liquidation at once, reducing market shock and cascading failures |
| Liquidation Threshold | Collateral ratio below which a position becomes eligible for liquidation | Defined in code | Protocol-defined bounds | Admin-controlled setter | Ensures positions remain sufficiently collateralized and protects lenders |
| Reserve Factor | Percentage of interest allocated to protocol reserves | Defined in code | 0% – 100% | Admin-controlled setter | Builds reserves for protocol stability and risk mitigation |
| Supply Cap | Maximum total supply allowed for a specific asset | Defined in code | ≥ 0 | Admin-controlled setter | Limits exposure to any single asset and reduces systemic risk |
| Borrow Cap | Maximum total borrow allowed for a specific asset | Defined in code | ≥ 0 | Admin-controlled setter | Prevents excessive leverage and liquidity stress |
| Minimum Borrow | Minimum borrowable amount | Defined in code | ≥ 0 | Admin-controlled setter | Avoids inefficient micro-loans and reduces spam |
| Rate Limits | Constraints on how quickly parameters or balances can change | Defined in code | Protocol-defined bounds | Admin-controlled setter | Prevents sudden parameter manipulation and extreme volatility |
| Minimum Collateral Ratio | Required collateral to debt ratio to prevent withdrawals or new borrows (10000 = 1.0) | 10000 | ≥ 10000 | Constant (code) | Prevents protocol insolvency by ensuring all debt is backed by collateral |


## Implementation Notes

- All parameters are enforced at the smart contract level.
- Validation is applied through:
  - Constant definitions
  - Admin setter functions
- Withdraw operations are also constrained by the same minimum collateral
  ratio invariant (`MIN_COLLATERAL_RATIO_BPS`): post-withdraw collateral must remain sufficient to back
  outstanding debt (including accrued interest).
- Any parameter updates must pass bounds checks before being applied.


## Verification

To verify correctness, refer to:

- Contract constants (e.g., `constants.rs`)
- Admin setter implementations in lending modules

Developers should ensure that:
- Documented bounds match enforced ranges
- Default values align with deployed configuration


## Design Considerations

These parameters are designed to balance:

- Protocol safety  
- Capital efficiency  
- Market stability  

Changes to these values should be governed carefully to avoid unintended economic consequences.
