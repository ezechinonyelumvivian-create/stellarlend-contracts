// src/types.rs
// Shared types, storage keys, and error definitions for StellarLend

use soroban_sdk::{contracterror, contracttype, Address};

// ── Storage key namespace ────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    // Global protocol state
    Admin,
    Oracle,
    EmergencyShutdown,

    // Per-asset markets
    Market(Address),       // MarketState
    TotalDeposits(Address),
    TotalBorrows(Address),
    Reserves(Address),

    // Per-user positions
    UserDeposit(Address, Address),  // (user, asset)
    UserBorrow(Address, Address),

    // Bad-debt accounting
    TotalBadDebt(Address),          // cumulative unrecovered debt per asset
    BadDebtWriteOff(Address, Address), // (user, asset) last write-off amount

    // Liquidation tracking
    LiquidationBonus(Address),      // asset -> bonus bps (e.g. 1050 = 5%)
    CollateralFactor(Address),      // asset -> CF bps  (e.g. 7500 = 75%)
}

// ── Core state structs ───────────────────────────────────────────────────────

/// Snapshot of a user's position in a single asset market.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPosition {
    pub deposited: i128,
    pub borrowed: i128,
}

/// Protocol-level market state for one asset.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketState {
    pub total_deposits: i128,
    pub total_borrows: i128,
    pub reserves: i128,
    pub bad_debt: i128,
    pub is_active: bool,
    pub is_frozen: bool, // frozen during emergency shutdown
}

impl MarketState {
    pub fn new() -> Self {
        Self {
            total_deposits: 0,
            total_borrows: 0,
            reserves: 0,
            bad_debt: 0,
            is_active: true,
            is_frozen: false,
        }
    }

    /// Solvency invariant: net assets must never be negative.
    /// net_assets = reserves + total_deposits - total_borrows - bad_debt
    pub fn check_solvency_invariant(&self) -> bool {
        let net = self.reserves + self.total_deposits - self.total_borrows - self.bad_debt;
        net >= 0
    }

    /// Bad-debt invariant: cumulative bad debt must never be negative.
    pub fn check_bad_debt_non_negative(&self) -> bool {
        self.bad_debt >= 0
    }

    /// Reserves invariant: reserves must never be negative.
    pub fn check_reserves_non_negative(&self) -> bool {
        self.reserves >= 0
    }
}

/// Result returned by view functions for off-chain consumption.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolReport {
    pub asset: Address,
    pub total_deposits: i128,
    pub total_borrows: i128,
    pub reserves: i128,
    pub bad_debt: i128,
    pub utilisation_bps: i128, // borrows / deposits * 10_000
    pub is_solvent: bool,
}

/// Detailed bad-debt event emitted on write-off.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadDebtEvent {
    pub user: Address,
    pub asset: Address,
    pub residual_debt: i128,
    pub collateral_seized: i128,
    pub reserve_cover: i128, // how much reserves absorbed
    pub written_off: i128,   // remainder socialised as bad debt
}

// ── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LendingError {
    NotInitialised       = 1,
    AlreadyInitialised   = 2,
    Unauthorised         = 3,
    MarketNotFound       = 4,
    MarketFrozen         = 5,
    InsufficientLiquidity = 6,
    InsufficientCollateral = 7,
    PositionSolvent      = 8,   // cannot liquidate a healthy position
    InvalidAmount        = 9,
    InvalidOracle        = 10,
    OraclePriceTooLow    = 11,
    ReservesExhausted    = 12,
    BadDebtNegative      = 13,  // invariant violation
    ReservesNegative     = 14,  // invariant violation
    EmergencyShutdown    = 15,
    PartialLiquidationLimit = 16,
}