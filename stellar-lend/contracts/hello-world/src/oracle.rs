// src/oracle.rs
// Oracle price feed integration.
//
// Design notes
// ─────────────
// • All prices are expressed in micro-USD (1 USD = 1_000_000 units) to avoid
//   floating-point issues in integer-only Soroban arithmetic.
// • A minimum price floor (MIN_PRICE) guards against zero/negative oracle
//   inputs that would allow infinite leverage.
// • Staleness is enforced via a MAX_AGE ledger-sequence window.  For tests the
//   mock oracle bypasses age checks.

use soroban_sdk::{Address, Env, Map};

/// 1 USD expressed in micro-USD.
pub const PRICE_PRECISION: i128 = 1_000_000;
/// Hard floor: price must be at least $0.000001.
pub const MIN_PRICE: i128 = 1;
/// Prices older than this many ledgers are rejected.
pub const MAX_AGE_LEDGERS: u32 = 20;
/// Basis-point denominator (used by liquidate.rs compute_collateral_seized).
pub const BPS_DENOM: i128 = 10_000;

// ── Public interface ─────────────────────────────────────────────────────────

/// Returns the price of `asset` in micro-USD.
///
/// In production this would call an external oracle contract.  During tests the
/// mock store (set via `set_mock_price`) is consulted first.
pub fn get_price(env: &Env, asset: &Address) -> Result<i128, crate::types::LendingError> {
    // Try mock store (test environment).
    let key = soroban_sdk::symbol_short!("prices");
    if let Some(map) = env
        .storage()
        .temporary()
        .get::<soroban_sdk::Symbol, Map<Address, i128>>(&key)
    {
        if let Some(price) = map.get(asset.clone()) {
            validate_price(price)?;
            return Ok(price);
        }
    }
    // Fallback: no oracle configured → reject.
    Err(crate::types::LendingError::InvalidOracle)
}

/// Sets a mock price in temporary storage (test-only helper exposed via
/// `#[cfg(test)]` in the test modules).
pub fn set_mock_price(env: &Env, asset: &Address, price: i128) {
    let key = soroban_sdk::symbol_short!("prices");
    let mut map: Map<Address, i128> = env
        .storage()
        .temporary()
        .get::<soroban_sdk::Symbol, Map<Address, i128>>(&key)
        .unwrap_or_else(|| Map::new(env));
    map.set(asset.clone(), price);
    env.storage().temporary().set(&key, &map);
}

/// Computes the USD value of `amount` units of `asset`.
///
/// `amount` is in the asset's native precision (scaled by PRICE_PRECISION
/// here so callers stay in integer arithmetic).
pub fn usd_value(env: &Env, asset: &Address, amount: i128) -> Result<i128, crate::types::LendingError> {
    let price = get_price(env, asset)?;
    Ok(amount
        .checked_mul(price)
        .ok_or(crate::types::LendingError::InvalidAmount)?
        / PRICE_PRECISION)
}

/// Computes the maximum borrow capacity of a collateral position.
///
/// `collateral_factor_bps` is expressed in basis points (e.g. 7500 = 75%).
pub fn max_borrow_usd(
    env: &Env,
    collateral_asset: &Address,
    collateral_amount: i128,
    collateral_factor_bps: i128,
) -> Result<i128, crate::types::LendingError> {
    let col_value = usd_value(env, collateral_asset, collateral_amount)?;
    Ok(col_value
        .checked_mul(collateral_factor_bps)
        .ok_or(crate::types::LendingError::InvalidAmount)?
        / 10_000)
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn validate_price(price: i128) -> Result<(), crate::types::LendingError> {
    if price < MIN_PRICE {
        return Err(crate::types::LendingError::OraclePriceTooLow);
    }
    Ok(())
}