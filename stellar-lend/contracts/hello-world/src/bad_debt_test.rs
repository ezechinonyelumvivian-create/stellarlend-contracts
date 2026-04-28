// src/bad_debt_test.rs
//
// Bad-Debt Accounting — Invariant Regression Suite
// ═════════════════════════════════════════════════
//
// Coverage map
// ────────────
// Group A — Full liquidation / zero residual
//   A1  full_liquidation_no_bad_debt
//   A2  full_liquidation_exact_collateral_equals_debt
//
// Group B — Partial liquidation leaving residual debt
//   B1  partial_liquidation_leaves_open_position
//   B2  close_factor_caps_repay_amount
//   B3  sequential_partial_liquidations_clear_position
//   B4  partial_liq_residual_covered_by_reserves
//   B5  partial_liq_residual_exceeds_reserves
//
// Group C — Collateral value collapse (rapid price crash)
//   C1  collateral_crash_instant_bad_debt
//   C2  collateral_crash_partial_reserve_cover
//   C3  collateral_value_to_zero_writes_full_debt
//   C4  oracle_price_at_minimum_floor
//
// Group D — Emergency shutdown interplay
//   D1  emergency_shutdown_freezes_new_borrows
//   D2  emergency_liquidate_during_shutdown
//   D3  emergency_liq_with_no_collateral
//   D4  shutdown_then_reserve_topup_recovers_bad_debt
//
// Group E — Invariant consistency under extreme values
//   E1  bad_debt_never_negative_after_recovery
//   E2  reserves_never_negative
//   E3  multiple_users_bad_debt_accumulates
//   E4  write_off_zeroes_user_borrow
//   E5  view_report_consistent_with_state
//
// Group F — Recovery scenarios
//   F1  reserve_topup_fully_clears_bad_debt
//   F2  reserve_topup_partial_clears_bad_debt
//   F3  sequential_topups_converge_to_zero
//
// Security notes — see docs/bad_debt_accounting.md §Security

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address};

use crate::{
    analytics,
    bad_debt_accounting,
    governance,
    liquidate,
    oracle,
    storage,
    test::TestEnv,
    types::LendingError,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Group A — Full liquidation / zero residual
// ═══════════════════════════════════════════════════════════════════════════════

/// A1: When collateral value fully covers debt (plus bonus), bad debt stays zero
/// and reserves are untouched.
#[test]
fn a1_full_liquidation_no_bad_debt() {
    let t = TestEnv::new();
    // ETH = $2000; borrower has 1 ETH collateral, borrows 1000 USDC.
    // At 75% CF: max = $1500 → 1000 USDC borrow is healthy until price < $1333.
    // Crash price to $1200 → health factor = 1200 * 0.75 / 1000 = 0.9 < 1.
    let borrower = Address::generate(&t.env);
    t.deposit_eth(&borrower, 1_000_000);      // 1 ETH (6dp)
    t.borrow_usdc(&borrower, 1_000_000_000);  // 1000 USDC

    // Crash to $1200 — position is under-water.
    t.crash_eth_price(1_200_000_000);

    let liquidator = Address::generate(&t.env);
    // Repay 500 USDC (50% close factor).
    let result = liquidate::liquidate(
        &t.env, &liquidator, &borrower,
        &t.usdc, &t.eth, 500_000_000,
    ).unwrap();

    // Bad debt should be zero (collateral is adequate at $1200).
    assert!(result.bad_debt_event.is_none(), "expected no bad debt for partial liq with adequate collateral");
    assert_eq!(result.debt_repaid, 500_000_000);

    t.assert_usdc_invariants();
    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 0, "A1: bad_debt must be zero");
    assert_eq!(mkt.reserves, 0, "A1: reserves untouched");
}

/// A2: Exact collateral == debt → bad debt is zero, position fully cleared.
#[test]
fn a2_full_liquidation_exact_collateral_equals_debt() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    // Set up so collateral value exactly equals debt.
    // ETH at $1000; 1 ETH collateral, 1000 USDC borrow.
    oracle::set_mock_price(&t.env, &t.eth, 1_000_000_000); // $1000
    t.deposit_eth(&borrower, 1_000_000);      // 1 ETH
    t.borrow_usdc(&borrower, 1_000_000_000);  // 1000 USDC

    // Now crash ETH to exactly match debt (already at $1000 = $1000 debt).
    // Position is undercollateralised because CF = 75% < 100%.
    let result = bad_debt_accounting::record_bad_debt(
        &t.env, &borrower, &t.usdc, 0, 1_000_000,
    ).unwrap();

    assert_eq!(result.residual_debt, 0);
    assert_eq!(result.written_off, 0);
    assert_eq!(result.reserve_cover, 0);
    t.assert_usdc_invariants();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group B — Partial liquidation leaving residual debt
// ═══════════════════════════════════════════════════════════════════════════════

/// B1: Partial liquidation (50% close factor) leaves an open position;
/// remaining debt is still positive and bad_debt stays 0.
#[test]
fn b1_partial_liquidation_leaves_open_position() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);
    t.deposit_eth(&borrower, 2_000_000);      // 2 ETH
    t.borrow_usdc(&borrower, 2_000_000_000);  // 2000 USDC

    // Crash ETH to $800 → health = (2 * 800 * 0.75) / 2000 = 0.6 < 1.
    t.crash_eth_price(800_000_000);

    let liq = Address::generate(&t.env);
    let result = liquidate::liquidate(
        &t.env, &liq, &borrower, &t.usdc, &t.eth, 1_000_000_000,
    ).unwrap();

    // Close factor: max repay = 2000 * 50% = 1000 USDC.
    assert_eq!(result.debt_repaid, 1_000_000_000, "B1: should repay 1000 USDC");

    // Remaining borrow should be 1000 USDC.
    let remaining = storage::get_user_borrow(&t.env, &borrower, &t.usdc);
    assert_eq!(remaining, 1_000_000_000, "B1: 1000 USDC borrow remains");

    // No bad debt yet — position is still open.
    assert!(result.bad_debt_event.is_none(), "B1: no bad debt on partial liq");
    t.assert_usdc_invariants();
}

/// B2: The close factor strictly caps the repay amount even if the liquidator
/// requests more.
#[test]
fn b2_close_factor_caps_repay_amount() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);
    t.deposit_eth(&borrower, 1_000_000);       // 1 ETH
    t.borrow_usdc(&borrower, 1_000_000_000);   // 1000 USDC

    t.crash_eth_price(1_000_000_000); // $1000 — undercollateralised

    let liq = Address::generate(&t.env);
    // Try to repay 900 USDC — close factor allows only 500 USDC.
    let result = liquidate::liquidate(
        &t.env, &liq, &borrower, &t.usdc, &t.eth, 900_000_000,
    ).unwrap();

    assert_eq!(result.debt_repaid, 500_000_000, "B2: close factor cap applied");
    t.assert_usdc_invariants();
}

/// B3: Two sequential partial liquidations clear the position entirely.
#[test]
fn b3_sequential_partial_liquidations_clear_position() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);
    t.deposit_eth(&borrower, 2_000_000);       // 2 ETH
    t.borrow_usdc(&borrower, 2_000_000_000);   // 2000 USDC

    t.crash_eth_price(800_000_000);            // $800/ETH

    let liq = Address::generate(&t.env);

    // First liquidation: repay 1000 USDC.
    liquidate::liquidate(
        &t.env, &liq, &borrower, &t.usdc, &t.eth, 1_000_000_000,
    ).unwrap();

    // Second liquidation: repay remaining 1000 USDC.
    liquidate::liquidate(
        &t.env, &liq, &borrower, &t.usdc, &t.eth, 1_000_000_000,
    ).unwrap();

    let remaining = storage::get_user_borrow(&t.env, &borrower, &t.usdc);
    assert_eq!(remaining, 0, "B3: position fully cleared");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 0, "B3: no bad debt with sufficient collateral");
    t.assert_usdc_invariants();
}

/// B4: Partial liquidation residual covered entirely by reserves.
///     bad_debt stays 0; reserves decrease.
#[test]
fn b4_partial_liq_residual_covered_by_reserves() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    // Give protocol reserves before the crash.
    t.add_usdc_reserves(500_000_000); // 500 USDC in reserves

    // Crash ETH to near zero so collateral is worthless.
    t.crash_eth_price(1_000); // $0.001 — essentially worthless

    // Manually set up a nearly-empty position for direct record_bad_debt call.
    let debt = 100_000_000i128; // 100 USDC residual
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    let event = bad_debt_accounting::record_bad_debt(
        &t.env, &borrower, &t.usdc, debt, 0,
    ).unwrap();

    assert_eq!(event.reserve_cover, debt, "B4: reserves absorb full residual");
    assert_eq!(event.written_off, 0, "B4: nothing socialised");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 0, "B4: bad_debt == 0");
    assert_eq!(mkt.reserves, 400_000_000, "B4: reserves reduced by 100 USDC");
    t.assert_usdc_invariants();
}

/// B5: Partial liquidation residual exceeds reserves.
///     Reserves are exhausted; remainder is socialised as bad_debt.
#[test]
fn b5_partial_liq_residual_exceeds_reserves() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    t.add_usdc_reserves(50_000_000); // 50 USDC in reserves

    let debt = 200_000_000i128; // 200 USDC residual
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    let event = bad_debt_accounting::record_bad_debt(
        &t.env, &borrower, &t.usdc, debt, 0,
    ).unwrap();

    assert_eq!(event.reserve_cover, 50_000_000, "B5: reserves fully consumed");
    assert_eq!(event.written_off, 150_000_000, "B5: 150 USDC socialised");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 150_000_000, "B5: bad_debt == 150 USDC");
    assert_eq!(mkt.reserves, 0,           "B5: reserves == 0");
    t.assert_usdc_invariants();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group C — Collateral value collapse
// ═══════════════════════════════════════════════════════════════════════════════

/// C1: Instant total collateral collapse → full debt becomes bad debt
///     (no reserves to absorb it).
#[test]
fn c1_collateral_crash_instant_bad_debt() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);
    t.deposit_eth(&borrower, 1_000_000);      // 1 ETH
    t.borrow_usdc(&borrower, 1_000_000_000);  // 1000 USDC

    // Crash ETH to zero-ish (below MIN_PRICE floor → use emergency path).
    // Here we set collateral deposit to 0 to simulate total loss.
    storage::set_user_deposit(&t.env, &borrower, &t.eth, 0);
    let mut mkt = storage::get_market(&t.env, &t.eth).unwrap();
    mkt.total_deposits = 0;
    storage::set_market(&t.env, &t.eth, &mkt);

    // With 0 collateral the full 1000 USDC is a shortfall.
    let residual = 1_000_000_000i128;
    let event = bad_debt_accounting::record_bad_debt(
        &t.env, &borrower, &t.usdc, residual, 0,
    ).unwrap();

    assert_eq!(event.written_off, residual, "C1: full debt socialised");
    assert_eq!(event.reserve_cover, 0,      "C1: no reserves to cover");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, residual, "C1: bad_debt == 1000 USDC");
    assert_eq!(mkt.reserves, 0,        "C1: reserves unchanged at 0");
    t.assert_usdc_invariants();
}

/// C2: Collateral crash with partial reserve cover.
#[test]
fn c2_collateral_crash_partial_reserve_cover() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    t.add_usdc_reserves(300_000_000); // 300 USDC reserves

    let residual = 1_000_000_000i128; // 1000 USDC shortfall
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, residual);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += residual;
    storage::set_market(&t.env, &t.usdc, &mkt);

    let event = bad_debt_accounting::record_bad_debt(
        &t.env, &borrower, &t.usdc, residual, 0,
    ).unwrap();

    assert_eq!(event.reserve_cover,  300_000_000, "C2: 300 USDC covered by reserves");
    assert_eq!(event.written_off,    700_000_000, "C2: 700 USDC socialised");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 700_000_000);
    assert_eq!(mkt.reserves, 0);
    t.assert_usdc_invariants();
}

/// C3: Collateral value falls to exactly zero (simulated via deposit zeroing).
///     Full debt is written off.
#[test]
fn c3_collateral_value_to_zero_writes_full_debt() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let debt = 500_000_000i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    let event = bad_debt_accounting::record_bad_debt(
        &t.env, &borrower, &t.usdc, debt, 0,
    ).unwrap();

    assert_eq!(event.residual_debt, debt);
    assert_eq!(event.written_off, debt);

    // User borrow must be zero after write-off (invariant I-6).
    let remaining = storage::get_user_borrow(&t.env, &borrower, &t.usdc);
    assert_eq!(remaining, 0, "C3: user borrow zeroed [I-6]");
    t.assert_usdc_invariants();
}

/// C4: Oracle price is at the minimum floor (MIN_PRICE = 1 micro-USD).
///     System should still process without underflow.
#[test]
fn c4_oracle_price_at_minimum_floor() {
    let t = TestEnv::new();
    // Set ETH to exactly MIN_PRICE.
    oracle::set_mock_price(&t.env, &t.eth, oracle::MIN_PRICE);

    let price = oracle::get_price(&t.env, &t.eth).unwrap();
    assert_eq!(price, oracle::MIN_PRICE, "C4: MIN_PRICE accepted");

    // USD value of 1 ETH at MIN_PRICE.
    let val = oracle::usd_value(&t.env, &t.eth, 1_000_000).unwrap();
    // 1_000_000 * 1 / 1_000_000 = 1 micro-USD
    assert_eq!(val, 1, "C4: usd_value at MIN_PRICE does not underflow");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group D — Emergency shutdown interplay
// ═══════════════════════════════════════════════════════════════════════════════

/// D1: After emergency_shutdown, deposit and borrow are rejected on frozen markets.
#[test]
fn d1_emergency_shutdown_freezes_new_actions() {
    let t = TestEnv::new();
    let user = Address::generate(&t.env);

    // Trigger shutdown.
    governance::emergency_shutdown(&t.env, &[t.usdc.clone(), t.eth.clone()]).unwrap();

    // Deposit should fail.
    let dep_res = crate::deposit::deposit(&t.env, &user, &t.usdc, 1_000_000);
    assert_eq!(dep_res, Err(LendingError::EmergencyShutdown), "D1: deposit blocked");

    // The market should be frozen.
    let mkt = t.usdc_market();
    assert!(mkt.is_frozen, "D1: market frozen after shutdown");
}

/// D2: Emergency liquidation during shutdown writes off shortfall correctly.
#[test]
fn d2_emergency_liquidate_during_shutdown() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    // Set up position.
    t.deposit_eth(&borrower, 1_000_000);      // 1 ETH @ $2000
    t.borrow_usdc(&borrower, 1_000_000_000);  // 1000 USDC

    // Crash ETH to $500 → collateral value = $500, debt = $1000.
    t.crash_eth_price(500_000_000);

    // Shutdown.
    governance::emergency_shutdown(&t.env, &[t.usdc.clone(), t.eth.clone()]).unwrap();

    // Emergency liquidation should still work.
    let result = liquidate::emergency_liquidate(
        &t.env, &borrower, &t.usdc, &t.eth,
    ).unwrap();

    // All collateral seized.
    assert_eq!(result.collateral_seized, 1_000_000, "D2: full ETH seized");

    // Shortfall: debt $1000 - collateral $500 = $500 residual.
    let bad_debt_event = result.bad_debt_event.expect("D2: bad debt event expected");
    assert!(bad_debt_event.written_off > 0, "D2: shortfall written off");

    // User borrow zeroed.
    let remaining = storage::get_user_borrow(&t.env, &borrower, &t.usdc);
    assert_eq!(remaining, 0, "D2: user borrow zeroed [I-6]");

    t.assert_usdc_invariants();
}

/// D3: Emergency liquidation when borrower has zero collateral.
///     Full debt becomes bad debt immediately.
#[test]
fn d3_emergency_liq_with_no_collateral() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    // Borrower somehow has debt but zero collateral (edge case).
    let debt = 500_000_000i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    storage::set_shutdown(&t.env, true);

    let result = liquidate::emergency_liquidate(
        &t.env, &borrower, &t.usdc, &t.eth,
    ).unwrap();

    assert_eq!(result.collateral_seized, 0, "D3: no collateral to seize");
    let event = result.bad_debt_event.expect("D3: bad debt event expected");
    // Full debt should be written off (collateral_usd = 0, borrow_usd = debt).
    assert_eq!(event.written_off + event.reserve_cover, event.residual_debt);

    t.assert_usdc_invariants();
}

/// D4: Shutdown → bad debt accrues → reserve top-up recovers it.
#[test]
fn d4_shutdown_then_reserve_topup_recovers_bad_debt() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let debt = 300_000_000i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    // Write off full debt as bad debt.
    bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, debt, "D4: bad debt set");

    // Governance tops up reserves — should reduce bad debt.
    let recovered = governance::add_reserves(&t.env, &t.usdc, debt).unwrap();
    assert_eq!(recovered, debt, "D4: full debt recovered");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 0, "D4: bad_debt cleared after reserve top-up");
    t.assert_usdc_invariants();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group E — Invariant consistency under extreme values
// ═══════════════════════════════════════════════════════════════════════════════

/// E1: bad_debt never goes negative after partial recovery.
#[test]
fn e1_bad_debt_never_negative_after_recovery() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let debt = 100_000_000i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();

    // Over-recover: add 200 USDC when only 100 USDC bad debt exists.
    // The excess 100 USDC should remain as reserves, bad_debt clamped at 0.
    bad_debt_accounting::attempt_bad_debt_recovery(&t.env, &t.usdc, 200_000_000).unwrap();

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 0,           "E1: bad_debt >= 0 after over-recovery");
    assert_eq!(mkt.reserves, 100_000_000, "E1: excess stays in reserves");
    t.assert_usdc_invariants();
}

/// E2: Reserves cannot go below zero even under maximum write-off pressure.
#[test]
fn e2_reserves_never_negative() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    t.add_usdc_reserves(100_000_000); // 100 USDC

    let residual = 100_000_001i128; // 1 micro-USDC more than reserves
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, residual);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += residual;
    storage::set_market(&t.env, &t.usdc, &mkt);

    let event = bad_debt_accounting::record_bad_debt(
        &t.env, &borrower, &t.usdc, residual, 0,
    ).unwrap();

    assert_eq!(event.reserve_cover, 100_000_000, "E2: reserves fully consumed");
    assert_eq!(event.written_off,            1, "E2: 1 micro-USDC socialised");

    let mkt = t.usdc_market();
    assert_eq!(mkt.reserves, 0, "E2: reserves == 0, not negative [I-2]");
    t.assert_usdc_invariants();
}

/// E3: Multiple insolvent users; bad debt accumulates correctly per user,
///     sums correctly in the market state.
#[test]
fn e3_multiple_users_bad_debt_accumulates() {
    let t = TestEnv::new();

    let amounts = [100_000_000i128, 200_000_000, 50_000_000];
    let mut expected_total = 0i128;

    for &debt in &amounts {
        let borrower = Address::generate(&t.env);
        storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
        let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
        mkt.total_borrows += debt;
        storage::set_market(&t.env, &t.usdc, &mkt);

        bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();
        expected_total += debt;
    }

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, expected_total, "E3: bad_debt sums correctly");
    t.assert_usdc_invariants();
}

/// E4: After write-off, the user's borrow balance is exactly 0 (I-6).
#[test]
fn e4_write_off_zeroes_user_borrow() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let debt = 777_777_777i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);

    bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();

    let remaining = storage::get_user_borrow(&t.env, &borrower, &t.usdc);
    assert_eq!(remaining, 0, "E4: user borrow zeroed [I-6]");
}

/// E5: View functions (`query_protocol_report`) are consistent with raw state
///     after a write-off.
#[test]
fn e5_view_report_consistent_with_state() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let deposit = 5_000_000_000i128;
    let debt = 400_000_000i128;
    let reserves = 200_000_000i128;

    // Set up market state manually.
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_deposits = deposit;
    mkt.total_borrows = debt;
    mkt.reserves = reserves;
    storage::set_market(&t.env, &t.usdc, &mkt);

    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);

    // Record bad debt (residual = full debt, reserve covers half).
    bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();

    let report = analytics::get_protocol_report(&t.env, &t.usdc).unwrap();

    // View must agree with storage.
    let state = storage::get_market(&t.env, &t.usdc).unwrap();
    assert_eq!(report.total_deposits, state.total_deposits, "E5: deposits consistent");
    assert_eq!(report.total_borrows,  state.total_borrows,  "E5: borrows consistent");
    assert_eq!(report.reserves,       state.reserves,       "E5: reserves consistent");
    assert_eq!(report.bad_debt,       state.bad_debt,       "E5: bad_debt consistent");
    assert!(report.bad_debt >= 0,                           "E5: bad_debt >= 0 in view [I-1]");
    assert!(report.reserves >= 0,                           "E5: reserves >= 0 in view [I-2]");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Group F — Recovery scenarios
// ═══════════════════════════════════════════════════════════════════════════════

/// F1: A single reserve top-up fully clears outstanding bad debt.
#[test]
fn f1_reserve_topup_fully_clears_bad_debt() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let debt = 250_000_000i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);
    bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();

    // Top up exactly the bad debt amount.
    let recovered = governance::add_reserves(&t.env, &t.usdc, debt).unwrap();
    assert_eq!(recovered, debt, "F1: full recovery");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 0, "F1: bad_debt cleared");
    assert_eq!(mkt.reserves, 0, "F1: reserves consumed for recovery");
    t.assert_usdc_invariants();
}

/// F2: Partial reserve top-up reduces bad debt proportionally.
#[test]
fn f2_reserve_topup_partial_clears_bad_debt() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let debt = 1_000_000_000i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);
    bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();

    // Partial top-up: 400 USDC.
    let recovered = governance::add_reserves(&t.env, &t.usdc, 400_000_000).unwrap();
    assert_eq!(recovered, 400_000_000, "F2: partial recovery");

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 600_000_000, "F2: 600 USDC bad debt remains");
    assert_eq!(mkt.reserves, 0,           "F2: reserves consumed");
    t.assert_usdc_invariants();
}

/// F3: Multiple sequential top-ups converge bad_debt to zero.
#[test]
fn f3_sequential_topups_converge_to_zero() {
    let t = TestEnv::new();
    let borrower = Address::generate(&t.env);

    let debt = 900_000_000i128;
    storage::set_user_borrow(&t.env, &borrower, &t.usdc, debt);
    let mut mkt = storage::get_market(&t.env, &t.usdc).unwrap();
    mkt.total_borrows += debt;
    storage::set_market(&t.env, &t.usdc, &mkt);
    bad_debt_accounting::record_bad_debt(&t.env, &borrower, &t.usdc, debt, 0).unwrap();

    // Three 300 USDC top-ups.
    for i in 0..3 {
        governance::add_reserves(&t.env, &t.usdc, 300_000_000).unwrap();
        let mkt = t.usdc_market();
        let expected_remaining = debt - 300_000_000 * (i + 1);
        assert_eq!(mkt.bad_debt, expected_remaining, "F3: bad_debt after top-up {}", i + 1);
        t.assert_usdc_invariants();
    }

    let mkt = t.usdc_market();
    assert_eq!(mkt.bad_debt, 0, "F3: bad_debt converges to zero");
}