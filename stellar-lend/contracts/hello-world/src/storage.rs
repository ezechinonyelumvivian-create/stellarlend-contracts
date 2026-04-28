// src/storage.rs
// Thin wrappers around Soroban persistent storage.
// Keeping all storage access centralised makes refactoring, auditing, and
// mocking straightforward.

use soroban_sdk::{Address, Env};

use crate::types::{DataKey, LendingError, MarketState};

// ── Market state ─────────────────────────────────────────────────────────────

pub fn get_market(env: &Env, asset: &Address) -> Result<MarketState, LendingError> {
    env.storage()
        .persistent()
        .get::<DataKey, MarketState>(&DataKey::Market(asset.clone()))
        .ok_or(LendingError::MarketNotFound)
}

pub fn set_market(env: &Env, asset: &Address, state: &MarketState) {
    env.storage()
        .persistent()
        .set(&DataKey::Market(asset.clone()), state);
}

pub fn init_market(env: &Env, asset: &Address) {
    let state = MarketState::new();
    env.storage()
        .persistent()
        .set(&DataKey::Market(asset.clone()), &state);
}

// ── User positions ────────────────────────────────────────────────────────────

pub fn get_user_deposit(env: &Env, user: &Address, asset: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<DataKey, i128>(&DataKey::UserDeposit(user.clone(), asset.clone()))
        .unwrap_or(0)
}

pub fn set_user_deposit(env: &Env, user: &Address, asset: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::UserDeposit(user.clone(), asset.clone()), &amount);
}

pub fn get_user_borrow(env: &Env, user: &Address, asset: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<DataKey, i128>(&DataKey::UserBorrow(user.clone(), asset.clone()))
        .unwrap_or(0)
}

pub fn set_user_borrow(env: &Env, user: &Address, asset: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::UserBorrow(user.clone(), asset.clone()), &amount);
}

// ── Bad-debt audit trail ──────────────────────────────────────────────────────

pub fn get_bad_debt_write_off(env: &Env, user: &Address, asset: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<DataKey, i128>(&DataKey::BadDebtWriteOff(user.clone(), asset.clone()))
        .unwrap_or(0)
}

pub fn set_bad_debt_write_off(env: &Env, user: &Address, asset: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::BadDebtWriteOff(user.clone(), asset.clone()), &amount);
}

// ── Collateral & liquidation parameters ──────────────────────────────────────

pub fn get_collateral_factor(env: &Env, asset: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<DataKey, i128>(&DataKey::CollateralFactor(asset.clone()))
        .unwrap_or(7_500) // default 75%
}

pub fn set_collateral_factor(env: &Env, asset: &Address, bps: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::CollateralFactor(asset.clone()), &bps);
}

pub fn get_liquidation_bonus(env: &Env, asset: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<DataKey, i128>(&DataKey::LiquidationBonus(asset.clone()))
        .unwrap_or(10_500) // default 5% bonus  (bps relative to 10_000)
}

pub fn set_liquidation_bonus(env: &Env, asset: &Address, bps: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::LiquidationBonus(asset.clone()), &bps);
}

// ── Emergency shutdown flag ───────────────────────────────────────────────────

pub fn is_shutdown(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get::<DataKey, bool>(&DataKey::EmergencyShutdown)
        .unwrap_or(false)
}

pub fn set_shutdown(env: &Env, flag: bool) {
    env.storage()
        .persistent()
        .set(&DataKey::EmergencyShutdown, &flag);
}