// src/governance.rs
// Protocol governance: admin actions, parameter changes, emergency shutdown.

use soroban_sdk::{Address, Env};

use crate::{bad_debt_accounting, storage, types::LendingError};

/// Initialise a new asset market.  Admin only.
pub fn add_market(
    env: &Env,
    asset: &Address,
    collateral_factor_bps: i128,
    liquidation_bonus_bps: i128,
) -> Result<(), LendingError> {
    storage::init_market(env, asset);
    storage::set_collateral_factor(env, asset, collateral_factor_bps);
    storage::set_liquidation_bonus(env, asset, liquidation_bonus_bps);
    Ok(())
}

/// Trigger protocol-wide emergency shutdown.
///
/// Effects:
/// • Sets the global shutdown flag.
/// • Freezes every provided market (no new deposits/borrows).
/// • Liquidations remain open.
pub fn emergency_shutdown(env: &Env, markets: &[Address]) -> Result<(), LendingError> {
    storage::set_shutdown(env, true);
    for asset in markets {
        bad_debt_accounting::freeze_market_for_shutdown(env, asset)?;
    }
    Ok(())
}

/// Top up reserves for an asset (e.g. from treasury or fee income).
/// Excess reserves reduce outstanding bad debt automatically.
pub fn add_reserves(env: &Env, asset: &Address, amount: i128) -> Result<i128, LendingError> {
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    bad_debt_accounting::attempt_bad_debt_recovery(env, asset, amount)
}

/// Update the collateral factor for an asset.
pub fn set_collateral_factor(env: &Env, asset: &Address, bps: i128) -> Result<(), LendingError> {
    if bps > 9_500 || bps < 100 {
        return Err(LendingError::InvalidAmount); // sanity bounds
    }
    storage::set_collateral_factor(env, asset, bps);
    Ok(())
}