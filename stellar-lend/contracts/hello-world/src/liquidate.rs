// src/lliquidate.rs
//
// Liquidation Engine
// ══════════════════
// Handles both full and partial liquidations.  When collateral is insufficient
// to cover the entire debt the residual is routed to `bad_debt_accounting`.
//
// Two liquidation paths
// ─────────────────────
// 1. **Normal liquidation** — liquidator repays up to `close_factor` of the
//    debt and receives collateral plus a bonus.  The position remains open if
//    partially liquidated.
//
// 2. **Emergency / full liquidation** — protocol governance can trigger a full
//    liquidation ignoring the close factor.  Used during emergency shutdown.
//    Any shortfall is written off immediately.
//
// Close factor
// ────────────
// By default, a liquidator may repay at most 50% of the outstanding borrow in
// a single call (CLOSE_FACTOR_BPS = 5_000).  This protects borrowers from
// being fully liquidated unnecessarily when only partially under-water.

use soroban_sdk::{Address, Env};

use crate::bad_debt_accounting;
use crate::oracle;
use crate::storage;
use crate::types::{BadDebtEvent, LendingError};

/// Maximum fraction of debt a liquidator may repay in one call (50%).
pub const CLOSE_FACTOR_BPS: i128 = 5_000;

// ── Structs ──────────────────────────────────────────────────────────────────

pub struct LiquidationResult {
    /// Collateral tokens transferred to the liquidator.
    pub collateral_seized: i128,
    /// Debt repaid by the liquidator.
    pub debt_repaid: i128,
    /// Bad-debt event (if any shortfall was written off).
    pub bad_debt_event: Option<BadDebtEvent>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Executes a normal liquidation of `borrower`'s `borrow_asset` position.
///
/// The liquidator specifies how much debt they wish to repay (`repay_amount`).
/// The function caps this at `CLOSE_FACTOR_BPS` of the outstanding borrow and
/// ensures the position is actually unhealthy before proceeding.
pub fn liquidate(
    env: &Env,
    _liquidator: &Address, // Would receive seized collateral in a production token-transfer impl.
    borrower: &Address,
    borrow_asset: &Address,
    collateral_asset: &Address,
    repay_amount: i128,
) -> Result<LiquidationResult, LendingError> {
    if repay_amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    // Guard: no new liquidations during shutdown (use emergency_liquidate).
    if storage::is_shutdown(env) {
        return Err(LendingError::EmergencyShutdown);
    }

    let borrow_market = storage::get_market(env, borrow_asset)?;
    if !borrow_market.is_active {
        return Err(LendingError::MarketNotFound);
    }

    let user_borrow = storage::get_user_borrow(env, borrower, borrow_asset);
    if user_borrow == 0 {
        return Err(LendingError::InvalidAmount);
    }

    // Verify position is unhealthy.
    check_position_unhealthy(env, borrower, borrow_asset, collateral_asset)?;

    // Apply close factor cap.
    let max_repay = user_borrow
        .checked_mul(CLOSE_FACTOR_BPS)
        .ok_or(LendingError::InvalidAmount)?
        / oracle::BPS_DENOM;
    let actual_repay = repay_amount.min(max_repay);

    // Compute collateral to seize (including liquidation bonus).
    let bonus_bps = storage::get_liquidation_bonus(env, collateral_asset);
    let collateral_seized = compute_collateral_seized(
        env,
        borrow_asset,
        collateral_asset,
        actual_repay,
        bonus_bps,
    )?;

    let user_collateral = storage::get_user_deposit(env, borrower, collateral_asset);
    let actual_seized = collateral_seized.min(user_collateral);

    // Update positions.
    let new_borrow = (user_borrow - actual_repay).max(0);
    storage::set_user_borrow(env, borrower, borrow_asset, new_borrow);
    storage::set_user_deposit(
        env,
        borrower,
        collateral_asset,
        (user_collateral - actual_seized).max(0),
    );

    // Update market totals.
    let mut borrow_mkt = storage::get_market(env, borrow_asset)?;
    borrow_mkt.total_borrows = (borrow_mkt.total_borrows - actual_repay).max(0);
    storage::set_market(env, borrow_asset, &borrow_mkt);

    let mut col_mkt = storage::get_market(env, collateral_asset)?;
    col_mkt.total_deposits = (col_mkt.total_deposits - actual_seized).max(0);
    storage::set_market(env, collateral_asset, &col_mkt);

    // Check if partial liquidation left residual bad debt.
    // (This happens only when actual_seized < collateral_seized, i.e. the
    // borrower had less collateral than the bonus-adjusted repay amount.)
    let bad_debt_event = if actual_seized < collateral_seized && new_borrow == 0 {
        let seized_value = oracle::usd_value(env, collateral_asset, actual_seized)?;
        let residual = (actual_repay - seized_value).max(0);
        if residual > 0 {
            Some(bad_debt_accounting::record_bad_debt(
                env,
                borrower,
                borrow_asset,
                residual,
                actual_seized,
            )?)
        } else {
            None
        }
    } else {
        None
    };

    Ok(LiquidationResult {
        collateral_seized: actual_seized,
        debt_repaid: actual_repay,
        bad_debt_event,
    })
}

/// Full liquidation regardless of close factor.  Used by governance during
/// emergency shutdown.  Always writes off any residual shortfall.
pub fn emergency_liquidate(
    env: &Env,
    borrower: &Address,
    borrow_asset: &Address,
    collateral_asset: &Address,
) -> Result<LiquidationResult, LendingError> {
    let user_borrow = storage::get_user_borrow(env, borrower, borrow_asset);
    if user_borrow == 0 {
        return Ok(LiquidationResult {
            collateral_seized: 0,
            debt_repaid: 0,
            bad_debt_event: None,
        });
    }

    let user_collateral = storage::get_user_deposit(env, borrower, collateral_asset);

    // Seize all collateral.
    storage::set_user_deposit(env, borrower, collateral_asset, 0);
    // NOTE: do NOT zero user_borrow here — record_bad_debt reads it to
    // correctly decrement total_borrows.  It will zero it as part of I-6.

    let mut col_mkt = storage::get_market(env, collateral_asset)?;
    col_mkt.total_deposits = (col_mkt.total_deposits - user_collateral).max(0);
    storage::set_market(env, collateral_asset, &col_mkt);

    // Compute residual in USD terms.
    let collateral_usd = oracle::usd_value(env, collateral_asset, user_collateral)?;
    let borrow_usd = oracle::usd_value(env, borrow_asset, user_borrow)?;
    let residual = (borrow_usd - collateral_usd).max(0);

    // record_bad_debt zeros the user borrow, decrements total_borrows,
    // and writes off the residual against reserves / bad_debt.
    let bad_debt_event = if residual > 0 {
        Some(bad_debt_accounting::record_bad_debt(
            env,
            borrower,
            borrow_asset,
            residual,
            user_collateral,
        )?)
    } else {
        // No shortfall: manually zero the position and decrement totals.
        let user_borrow_remaining = storage::get_user_borrow(env, borrower, borrow_asset);
        storage::set_user_borrow(env, borrower, borrow_asset, 0);
        let mut borrow_mkt = storage::get_market(env, borrow_asset)?;
        borrow_mkt.total_borrows = (borrow_mkt.total_borrows - user_borrow_remaining).max(0);
        storage::set_market(env, borrow_asset, &borrow_mkt);
        None
    };

    Ok(LiquidationResult {
        collateral_seized: user_collateral,
        debt_repaid: user_borrow,
        bad_debt_event,
    })
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn check_position_unhealthy(
    env: &Env,
    borrower: &Address,
    borrow_asset: &Address,
    collateral_asset: &Address,
) -> Result<(), LendingError> {
    let user_borrow = storage::get_user_borrow(env, borrower, borrow_asset);
    let user_deposit = storage::get_user_deposit(env, borrower, collateral_asset);
    let cf_bps = storage::get_collateral_factor(env, collateral_asset);

    let borrow_value = oracle::usd_value(env, borrow_asset, user_borrow)?;
    let max_borrow = oracle::max_borrow_usd(env, collateral_asset, user_deposit, cf_bps)?;

    if borrow_value <= max_borrow {
        return Err(LendingError::PositionSolvent);
    }
    Ok(())
}

fn compute_collateral_seized(
    env: &Env,
    borrow_asset: &Address,
    collateral_asset: &Address,
    repay_amount: i128,
    bonus_bps: i128,
) -> Result<i128, LendingError> {
    let borrow_price = oracle::get_price(env, borrow_asset)?;
    let collateral_price = oracle::get_price(env, collateral_asset)?;

    // repay_amount expressed in collateral units, grossed up by bonus.
    let seized = repay_amount
        .checked_mul(borrow_price)
        .ok_or(LendingError::InvalidAmount)?
        .checked_mul(bonus_bps)
        .ok_or(LendingError::InvalidAmount)?
        / (collateral_price * oracle::BPS_DENOM);

    Ok(seized)
}