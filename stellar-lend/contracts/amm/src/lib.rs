#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AmmError {
    /// Contract has not been initialised yet.
    NotInitialized = 1,
    /// `initialize_amm_settings` was called a second time.
    AlreadyInitialized = 2,
    /// Caller is not the admin.
    Unauthorized = 3,
    /// `amount_in` must be positive.
    InvalidAmount = 4,
    /// `min_out` must be non-negative.
    InvalidMinOut = 5,
    /// The ledger timestamp has passed `deadline`.
    DeadlineExpired = 6,
    /// Computed `amount_out` is below the caller's `min_out` floor.
    SlippageExceeded = 7,
    /// `default_slippage` or `max_slippage` is out of the permitted range.
    InvalidSlippage = 8,
    /// Integer overflow detected.
    Overflow = 9,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    DefaultSlippageBps,
    MaxSlippageBps,
    AutoSwapThreshold,
}

// ---------------------------------------------------------------------------
// Settings struct (returned by `get_settings`)
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmmSettings {
    pub default_slippage_bps: u32,
    pub max_slippage_bps: u32,
    pub auto_swap_threshold: i128,
}

// ---------------------------------------------------------------------------
// Swap result
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapResult {
    pub amount_out: i128,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct AmmContract;

#[contractimpl]
impl AmmContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// One-time initialisation.  Stores admin and AMM operating parameters.
    ///
    /// # Parameters
    /// - `admin`               – Address authorised to change settings.
    /// - `default_slippage_bps`– Default slippage tolerance in basis points.
    /// - `max_slippage_bps`    – Maximum slippage the contract will ever allow.
    /// - `auto_swap_threshold` – Minimum `amount_in` for automatic swaps.
    ///
    /// # Errors
    /// - `AlreadyInitialized` if called more than once.
    /// - `InvalidSlippage`    if `default_slippage_bps > max_slippage_bps` or
    ///                          either value exceeds 10 000 bps (100 %).
    pub fn initialize_amm_settings(
        env: Env,
        admin: Address,
        default_slippage_bps: u32,
        max_slippage_bps: u32,
        auto_swap_threshold: i128,
    ) -> Result<(), AmmError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(AmmError::AlreadyInitialized);
        }
        if default_slippage_bps > max_slippage_bps || max_slippage_bps > 10_000 {
            return Err(AmmError::InvalidSlippage);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::DefaultSlippageBps, &default_slippage_bps);
        env.storage()
            .instance()
            .set(&DataKey::MaxSlippageBps, &max_slippage_bps);
        env.storage()
            .instance()
            .set(&DataKey::AutoSwapThreshold, &auto_swap_threshold);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    pub fn get_admin(env: Env) -> Result<Address, AmmError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(AmmError::NotInitialized)
    }

    pub fn get_settings(env: Env) -> Result<AmmSettings, AmmError> {
        let default_slippage_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::DefaultSlippageBps)
            .ok_or(AmmError::NotInitialized)?;
        let max_slippage_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MaxSlippageBps)
            .ok_or(AmmError::NotInitialized)?;
        let auto_swap_threshold: i128 = env
            .storage()
            .instance()
            .get(&DataKey::AutoSwapThreshold)
            .ok_or(AmmError::NotInitialized)?;
        Ok(AmmSettings {
            default_slippage_bps,
            max_slippage_bps,
            auto_swap_threshold,
        })
    }

    // -----------------------------------------------------------------------
    // Swap entrypoint
    // -----------------------------------------------------------------------

    /// Execute a swap with slippage and deadline protection.
    ///
    /// Guards (checked **before** any state mutation):
    /// 1. `env.ledger().timestamp() <= deadline`  – stale transaction rejection.
    /// 2. `amount_out >= min_out`                 – sandwich-attack protection.
    ///
    /// The `amount_out` is computed by the constant-product formula:
    ///   `amount_out = amount_in * 997 / 1000`  (0.3 % fee mock, replace with
    ///   real pool math in production).
    ///
    /// # Parameters
    /// - `caller`     – Address invoking the swap; `require_auth()` is called.
    /// - `asset_in`   – Token being sold.
    /// - `asset_out`  – Token being bought.
    /// - `amount_in`  – Exact input amount (must be > 0).
    /// - `min_out`    – Minimum acceptable output amount (must be >= 0).
    ///                  Pass `0` to skip the slippage check (not recommended).
    /// - `deadline`   – Latest ledger timestamp at which this swap is valid
    ///                  (seconds since Unix epoch, same unit as
    ///                  `env.ledger().timestamp()`).
    ///                  Pass `u64::MAX` to opt out of the deadline check (not
    ///                  recommended for production use).
    ///
    /// # Errors
    /// - `NotInitialized`  – contract has not been initialised.
    /// - `InvalidAmount`   – `amount_in <= 0`.
    /// - `InvalidMinOut`   – `min_out < 0`.
    /// - `DeadlineExpired` – current timestamp > `deadline`.
    /// - `SlippageExceeded`– computed `amount_out < min_out`.
    /// - `Overflow`        – intermediate multiplication overflowed.
    pub fn swap(
        env: Env,
        caller: Address,
        asset_in: Address,
        asset_out: Address,
        amount_in: i128,
        min_out: i128,
        deadline: u64,
    ) -> Result<SwapResult, AmmError> {
        // Ensure the contract is initialised before touching any logic.
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(AmmError::NotInitialized);
        }

        // --- Input validation (before auth & state mutation) ---------------

        if amount_in <= 0 {
            return Err(AmmError::InvalidAmount);
        }
        if min_out < 0 {
            return Err(AmmError::InvalidMinOut);
        }

        // Guard 1: deadline check — reject stale / replayed transactions.
        let now = env.ledger().timestamp();
        if now > deadline {
            return Err(AmmError::DeadlineExpired);
        }

        // Authenticate the caller.
        caller.require_auth();

        // --- Compute amount_out (mock constant-product with 0.3 % fee) -----
        // Production code should call the actual on-chain AMM pool here.
        // We use `checked_mul` / `checked_div` to guard against overflow.
        let numerator = amount_in
            .checked_mul(997)
            .ok_or(AmmError::Overflow)?;
        let amount_out = numerator
            .checked_div(1000)
            .ok_or(AmmError::Overflow)?;

        // Guard 2: slippage check — reject if output falls below caller's floor.
        if amount_out < min_out {
            return Err(AmmError::SlippageExceeded);
        }

        // Suppress unused-variable warnings for asset addresses.
        // In production these identify which pool/route to use.
        let _ = asset_in;
        let _ = asset_out;

        Ok(SwapResult { amount_out })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};

    // ── helpers ─────────────────────────────────────────────────────────────

    fn setup() -> (Env, AmmContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        client
            .initialize_amm_settings(&admin, &100, &1000, &1_000_000)
            .unwrap();
        (env, client, admin)
    }

    fn set_time(env: &Env, ts: u64) {
        let mut li = env.ledger().get();
        li.timestamp = ts;
        env.ledger().set(li);
    }

    // ── initialization ───────────────────────────────────────────────────────

    #[test]
    fn test_initialize_stores_settings() {
        let (_env, client, _admin) = setup();
        let s = client.get_settings().unwrap();
        assert_eq!(s.default_slippage_bps, 100);
        assert_eq!(s.max_slippage_bps, 1000);
        assert_eq!(s.auto_swap_threshold, 1_000_000);
    }

    #[test]
    fn test_double_initialize_rejected() {
        let (env, client, admin) = setup();
        let res = client.try_initialize_amm_settings(&admin, &100, &1000, &1_000_000);
        assert!(matches!(res, Err(Ok(AmmError::AlreadyInitialized))));
    }

    #[test]
    fn test_invalid_slippage_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        // default_slippage > max_slippage
        let res = client.try_initialize_amm_settings(&admin, &500, &100, &1_000_000);
        assert!(matches!(res, Err(Ok(AmmError::InvalidSlippage))));
    }

    #[test]
    fn test_max_slippage_over_10000_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let res = client.try_initialize_amm_settings(&admin, &100, &10_001, &1_000_000);
        assert!(matches!(res, Err(Ok(AmmError::InvalidSlippage))));
    }

    // ── swap: happy path ─────────────────────────────────────────────────────

    #[test]
    fn test_valid_swap_returns_amount_out() {
        let (env, client, _admin) = setup();
        set_time(&env, 1_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        let result = client
            .swap(&caller, &asset_in, &asset_out, &1_000, &0, &2_000)
            .unwrap();

        // 1000 * 997 / 1000 = 997
        assert_eq!(result.amount_out, 997);
    }

    #[test]
    fn test_swap_with_min_out_exactly_met() {
        let (env, client, _admin) = setup();
        set_time(&env, 1_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        // min_out == amount_out (boundary: should pass)
        let result = client
            .swap(&caller, &asset_in, &asset_out, &1_000, &997, &2_000)
            .unwrap();
        assert_eq!(result.amount_out, 997);
    }

    // ── swap: deadline guard ─────────────────────────────────────────────────

    #[test]
    fn test_swap_deadline_expired_rejected() {
        let (env, client, _admin) = setup();
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        // Set ledger time beyond the deadline.
        set_time(&env, 5_000);
        let res = client.try_swap(&caller, &asset_in, &asset_out, &1_000, &0, &4_999);
        assert!(
            matches!(res, Err(Ok(AmmError::DeadlineExpired))),
            "expected DeadlineExpired, got {:?}",
            res
        );
    }

    #[test]
    fn test_swap_at_exact_deadline_accepted() {
        let (env, client, _admin) = setup();
        set_time(&env, 3_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        // now == deadline → should be accepted
        let result = client
            .swap(&caller, &asset_in, &asset_out, &1_000, &0, &3_000)
            .unwrap();
        assert_eq!(result.amount_out, 997);
    }

    #[test]
    fn test_swap_deadline_one_second_past_rejected() {
        let (env, client, _admin) = setup();
        set_time(&env, 3_001);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        let res = client.try_swap(&caller, &asset_in, &asset_out, &1_000, &0, &3_000);
        assert!(matches!(res, Err(Ok(AmmError::DeadlineExpired))));
    }

    // ── swap: min_out (slippage) guard ───────────────────────────────────────

    #[test]
    fn test_swap_sub_min_out_rejected() {
        let (env, client, _admin) = setup();
        set_time(&env, 1_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        // amount_out = 997, but caller demands at least 998
        let res = client.try_swap(&caller, &asset_in, &asset_out, &1_000, &998, &9_999);
        assert!(
            matches!(res, Err(Ok(AmmError::SlippageExceeded))),
            "expected SlippageExceeded, got {:?}",
            res
        );
    }

    #[test]
    fn test_swap_min_out_one_above_computed_rejected() {
        let (env, client, _admin) = setup();
        set_time(&env, 1_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        // amount_out = 997_000 for amount_in = 1_000_000; min_out = 997_001
        let res = client.try_swap(&caller, &asset_in, &asset_out, &1_000_000, &997_001, &9_999);
        assert!(matches!(res, Err(Ok(AmmError::SlippageExceeded))));
    }

    // ── swap: invalid inputs ─────────────────────────────────────────────────

    #[test]
    fn test_swap_zero_amount_in_rejected() {
        let (env, client, _admin) = setup();
        set_time(&env, 1_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        let res = client.try_swap(&caller, &asset_in, &asset_out, &0, &0, &9_999);
        assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));
    }

    #[test]
    fn test_swap_negative_amount_in_rejected() {
        let (env, client, _admin) = setup();
        set_time(&env, 1_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        let res = client.try_swap(&caller, &asset_in, &asset_out, &-1, &0, &9_999);
        assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));
    }

    #[test]
    fn test_swap_negative_min_out_rejected() {
        let (env, client, _admin) = setup();
        set_time(&env, 1_000);
        let caller = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);

        let res = client.try_swap(&caller, &asset_in, &asset_out, &1_000, &-1, &9_999);
        assert!(matches!(res, Err(Ok(AmmError::InvalidMinOut))));
    }

    #[test]
    fn test_swap_on_uninitialised_contract_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);
        let caller = Address::generate(&env);
        let a = Address::generate(&env);
        let b = Address::generate(&env);

        let res = client.try_swap(&caller, &a, &b, &1_000, &0, &9_999);
        assert!(matches!(res, Err(Ok(AmmError::NotInitialized))));
    }

    // ── priority ordering: deadline checked before slippage ──────────────────

    #[test]
    fn test_deadline_checked_before_slippage() {
        let (env, client, _admin) = setup();
        // Both guards would fire: deadline expired AND min_out too high.
        // Deadline must be reported first.
        set_time(&env, 9_000);
        let caller = Address::generate(&env);
        let a = Address::generate(&env);
        let b = Address::generate(&env);

        let res = client.try_swap(&caller, &a, &b, &1_000, &1_000_000, &8_999);
        assert!(matches!(res, Err(Ok(AmmError::DeadlineExpired))));
    }
}
