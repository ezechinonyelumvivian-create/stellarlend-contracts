// src/analytics.rs
// Off-chain view functions for dashboards and monitoring.

use soroban_sdk::{Address, Env};

use crate::{bad_debt_accounting, storage, types::{LendingError, ProtocolReport}};

/// Returns a consistent snapshot of a market's accounting.
pub fn get_protocol_report(env: &Env, asset: &Address) -> Result<ProtocolReport, LendingError> {
    bad_debt_accounting::query_protocol_report(env, asset)
}

/// Returns the current bad-debt balance for an asset.
pub fn get_bad_debt(env: &Env, asset: &Address) -> Result<i128, LendingError> {
    bad_debt_accounting::query_bad_debt(env, asset)
}

/// Returns the total write-off amount attributed to a specific user.
pub fn get_user_write_off(env: &Env, user: &Address, asset: &Address) -> i128 {
    storage::get_bad_debt_write_off(env, user, asset)
}

/// Checks whether a user's position is currently healthy.
pub fn is_position_healthy(
    env: &Env,
    user: &Address,
    borrow_asset: &Address,
    collateral_asset: &Address,
) -> Result<bool, LendingError> {
    use crate::oracle;
    let user_borrow = storage::get_user_borrow(env, user, borrow_asset);
    let user_deposit = storage::get_user_deposit(env, user, collateral_asset);
    let cf_bps = storage::get_collateral_factor(env, collateral_asset);
    let borrow_value = oracle::usd_value(env, borrow_asset, user_borrow)?;
    let max_borrow = oracle::max_borrow_usd(env, collateral_asset, user_deposit, cf_bps)?;
    Ok(borrow_value <= max_borrow)
}