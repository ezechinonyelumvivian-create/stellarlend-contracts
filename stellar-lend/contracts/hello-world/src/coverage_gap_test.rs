// src/coverage_gap_test.rs
//
// Coverage Gap Tests — Edge Cases & Boundary Conditions
// ══════════════════════════════════════════════════════
//
// These tests complement bad_debt_test.rs by exercising paths that are
// difficult to hit through the normal flow:
//
// Group G — Oracle constraints
//   G1  oracle_price_below_floor_rejected
//   G2  oracle_missing_returns_error
//   G3  usd_value_overflow_safe (large amounts)
//   G4  max_borrow_usd_with_zero_collateral
//
// Group H — Input validation
//   H1  record_bad_debt_zero_residual_is_noop
//   H2  record_bad_debt_negative_residual_rejected
//   H3  deposit_zero_amount_rejected
//   H4  borrow_zero_amount_rejected
//   H5  repay_more_than_owed_clamps_to_balance
//   H6  withdraw_more_than_deposited_rejected
//
// Group I — Market state guard rails
//   I1  liquidate_healthy_position_rejected
//   I2  liquidate_unknown_market_rejected
//   I3  borrow_exceeds_collateral_factor_rejected
//   I4  recovery_with_zero_amount_rejected
//
// Group J — Write-off audit trail
//   J1  write_off_audit_record_stored
//   J2  write_off_audit_record_per_user
//
// Group K — Utilisation rate
//   K1  utilisation_zero_when_no_deposits
//   K2  utilisation_100pct_when_fully_lent
//   K3  utilisation_bps_capped_correctly

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address};

use crate::{
    analytics,
    bad_debt_accounting,
    borrow, deposit,
    governance,
    liquidate,
    oracle,
    repay,
    storage,
    test::TestEnv,
    types::LendingError,
    withdraw,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Group G — Oracle constraints
// ═══════════════════════════════════════════════════════════════════════════════

/// G1: A price below the MIN_PRICE floor is rejected by the oracle.
#[test]
fn g1_oracle_price_below_floor_rejected() {
    let t = TestEnv::new();
    // Set price to 0 (below MIN_PRICE = 1).
    oracle::set_mock_price(&t.env, &t.usdc, 0);
    let result = oracle::get_price(&t.env, &t.usdc);
    assert_eq!(result, Err(LendingError::OraclePriceTooLow), "G1: zero price rejected");
}

/// G2: Querying price for an asset with no oracle entry returns InvalidOracle.
#[test]
fn g2_oracle_missing_returns_error() {
    let t = TestEnv::new();
    let unknown_asset = Address::generate(&t.env);
    let result = oracle::get_price(&t.env, &unknown_asset);
    assert_eq!(result, Err(LendingError::InvalidOracle), "G2: missing oracle entry");
}

/// G3: usd_value handles large amounts without overflow (uses checked_mul).
#[test]
fn g3_usd_value_large_amount_safe() {
    let t = TestEnv::new();
    // ETH = $2000; amount = 1_000_000_000_000 (1 million ETH in 6dp units).
    // Value = 1_000_000_000_000 * 2_000_000_000 / 1_000_000
    //       = 2_000_000_000_000_000  — fits in i128.
    let val = oracle::usd_value(&t.env, &t.eth, 1_000_000_000_000).unwrap();
    assert!(val > 0, "G3: large amount computes without panic");
}

/// G4: max_borrow_usd with zero collateral returns zero.
#[test]
fn g4_max_borrow_usd_with_zero_collateral() {
    let t = TestEnv::new();
    let max = oracle::max_borrow_usd(&t.env, &t.eth, 0, 7_500).unwrap();
    assert_eq!(max, 0, "G4: zero collateral → zero borrow capacity");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group H — Input validation
// ═══════════════════════════════════════════════════════════════════════════════

/// H1: record_bad_debt with zero residual is a no-op (returns Ok event with zeros).
#[test]
fn h1_record_bad_debt_zero_residual_is_noop() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);
    let event = bad_debt_accounting::record_bad_debt(&t.env, &user, &t.usdc, 0, 0).unwrap();
    assert_eq!(event.residual_debt, 0);
    assert_eq!(event.written_off,   0);
    assert_eq!(event.reserve_cover, 0);
    assert_eq!(t.usdc_market().bad_debt, 0, "H1: no-op doesn't change bad_debt");
}

/// H2: Negative residual_debt is rejected.
#[test]
fn h2_record_bad_debt_negative_residual_rejected() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);
    let result = bad_debt_accounting::record_bad_debt(&t.env, &user, &t.usdc, -1, 0);
    assert_eq!(result, Err(LendingError::BadDebtNegative), "H2: negative residual rejected");
}

/// H3: deposit with amount = 0 is rejected.
#[test]
fn h3_deposit_zero_amount_rejected() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);
    let result = deposit::deposit(&t.env, &user, &t.usdc, 0);
    assert_eq!(result, Err(LendingError::InvalidAmount), "H3: zero deposit rejected");
}

/// H4: borrow with amount = 0 is rejected.
#[test]
fn h4_borrow_zero_amount_rejected() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);
    let result = borrow::borrow(&t.env, &user, &t.usdc, &t.eth, 0);
    assert_eq!(result, Err(LendingError::InvalidAmount), "H4: zero borrow rejected");
}

/// H5: repay more than outstanding balance clamps to actual balance.
#[test]
fn h5_repay_more_than_owed_clamps() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);

    let debt = 100_000_000i128;
    storage::set_user_borrow(&t.env, &user, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    // Attempt to repay 200 USDC; actual borrow is 100.
    let actual = repay::repay(&t.env, &user, &t.usdc, 200_000_000).unwrap();
    assert_eq!(actual, debt, "H5: repay clamped to actual borrow");

    let remaining = storage::get_user_borrow(&t.env, &user, &t.usdc);
    assert_eq!(remaining, 0, "H5: borrow zeroed");
}

/// H6: Withdrawing more than deposited is rejected.
#[test]
fn h6_withdraw_more_than_deposited_rejected() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);
    t.deposit_eth(&user, 1_000_000); // 1 ETH

    let result = withdraw::withdraw(&t.env, &user, &t.eth, 2_000_000); // 2 ETH
    assert_eq!(result, Err(LendingError::InsufficientLiquidity), "H6: over-withdraw rejected");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group I — Market guard rails
// ═══════════════════════════════════════════════════════════════════════════════

/// I1: Liquidating a healthy position is rejected with PositionSolvent.
#[test]
fn i1_liquidate_healthy_position_rejected() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    // 1 ETH @ $2000, borrow 500 USDC → well within 75% CF ($1500 capacity).
    t.deposit_eth(&borrower, 1_000_000);
    t.borrow_usdc(&borrower, 500_000_000);

    let liq = Address::generate(&t.env);
    let result = liquidate::liquidate(
        &t.env, &liq, &borrower, &t.usdc, &t.eth, 250_000_000,
    );
    assert_eq!(result, Err(LendingError::PositionSolvent), "I1: healthy position can't be liquidated");
}

/// I2: Liquidating against an unknown market returns MarketNotFound.
#[test]
fn i2_liquidate_unknown_market_rejected() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);
    let liq = Address::generate(&t.env);
    let fake_asset = Address::generate(&t.env);

    let result = liquidate::liquidate(
        &t.env, &liq, &borrower, &fake_asset, &t.eth, 100_000_000,
    );
    assert_eq!(result, Err(LendingError::MarketNotFound), "I2: unknown market rejected");
}

/// I3: Borrowing beyond the collateral factor is rejected.
#[test]
fn i3_borrow_exceeds_collateral_factor_rejected() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);

    // 1 ETH @ $2000; CF = 75%; max borrow = $1500.
    t.deposit_eth(&user, 1_000_000); // 1 ETH

    // Try to borrow $1600 worth of USDC (> $1500 cap).
    let result = borrow::borrow(&t.env, &user, &t.usdc, &t.eth, 1_600_000_000);
    assert_eq!(result, Err(LendingError::InsufficientCollateral), "I3: over-borrow rejected");
}

/// I4: Attempting to recover with a zero or negative amount is rejected.
#[test]
fn i4_recovery_with_zero_amount_rejected() {
    let t = TestEnv::new();
    let result = bad_debt_accounting::attempt_bad_debt_recovery(&t.env, &t.usdc, 0);
    assert_eq!(result, Err(LendingError::InvalidAmount), "I4: zero recovery rejected");

    let result2 = bad_debt_accounting::attempt_bad_debt_recovery(&t.env, &t.usdc, -5);
    assert_eq!(result2, Err(LendingError::InvalidAmount), "I4: negative recovery rejected");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group J — Write-off audit trail
// ═══════════════════════════════════════════════════════════════════════════════

/// J1: After a write-off, the per-user audit record is stored.
#[test]
fn j1_write_off_audit_record_stored() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);

    let debt = 123_456_789i128;
    storage::set_user_borrow(&t.env, &user, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    bad_debt_accounting::record_bad_debt(&t.env, &user, &t.usdc, debt, 0).unwrap();

    let audit = analytics::get_user_write_off(&t.env, &user, &t.usdc);
    assert_eq!(audit, debt, "J1: write-off audit record matches residual");
}

/// J2: Write-off records are per-user, not shared.
#[test]
fn j2_write_off_audit_record_per_user() {
    let t = TestEnv::new();
    let user_a = Address::generate(&t.env);
    let user_b = Address::generate(&t.env);

    let debt_a = 100_000_000i128;
    let debt_b = 200_000_000i128;

    for (user, debt) in [(&user_a, debt_a), (&user_b, debt_b)] {
        storage::set_user_borrow(&t.env, user, &t.usdc, debt);
        let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
        mkt.total_borrows += debt;
        storage::set_market(&t.env, &t.usdc, &mkt);
        bad_debt_accounting::record_bad_debt(&t.env, user, &t.usdc, debt, 0).unwrap();
    }

    let audit_a = analytics::get_user_write_off(&t.env, &user_a, &t.usdc);
    let audit_b = analytics::get_user_write_off(&t.env, &user_b, &t.usdc);

    assert_eq!(audit_a, debt_a, "J2: user A audit correct");
    assert_eq!(audit_b, debt_b, "J2: user B audit correct");
    assert_ne!(audit_a, audit_b, "J2: per-user records are independent");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group K — Utilisation rate
// ═══════════════════════════════════════════════════════════════════════════════

/// K1: Utilisation is 0 bps when there are no deposits.
#[test]
fn k1_utilisation_zero_when_no_deposits() {
    let t = TestEnv::new();
    let report = analytics::get_protocol_report(&t.env, &t.usdc).unwrap();
    assert_eq!(report.utilisation_bps, 0, "K1: 0 utilisation with no deposits");
}

/// K2: Utilisation is 10_000 bps (100%) when all deposits are borrowed.
#[test]
fn k2_utilisation_100pct_when_fully_lent() {
    let t = TestEnv::new();
    let amount = 1_000_000_000i128;

    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_deposits = amount;
    mkt.total_borrows = amount;
    storage::set_market(&t.env, &t.usdc, &mkt);

    let report = analytics::get_protocol_report(&t.env, &t.usdc).unwrap();
    assert_eq!(report.utilisation_bps, 10_000, "K2: 100% utilisation = 10000 bps");
}

/// K3: Utilisation rounds down and does not exceed 10_000 bps in normal cases.
#[test]
fn k3_utilisation_bps_within_bounds() {
    let t = TestEnv::new();

    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_deposits = 3_000_000_000i128;
    mkt.total_borrows = 1_500_000_000i128; // 50%
    storage::set_market(&t.env, &t.usdc, &mkt);

    let report = analytics::get_protocol_report(&t.env, &t.usdc).unwrap();
    assert_eq!(report.utilisation_bps, 5_000, "K3: 50% utilisation = 5000 bps");
    assert!(report.utilisation_bps <= 10_000, "K3: utilisation never exceeds 10_000 bps");
}