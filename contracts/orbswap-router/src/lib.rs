#![no_std]
//! Orbswap router — stateless multi-hop swap + liquidity orchestrator.
//!
//! A swap path is a list of **pool addresses** plus the **token path** through
//! them: `tokens[i] -> tokens[i+1]` across `pools[i]`, so `tokens.len()` is always
//! `pools.len() + 1`. The token path is explicit because an n-token pool has no
//! single "other" token to infer — a 4-token pool can be entered and left by any
//! of its legs.
//!
//! The router chains `pool.swap(user, …)` calls with `from = user` at every hop,
//! so the **user** holds the intermediate tokens (the router never takes custody).
//! Each entry point calls `require_auth()` on the user first: the host's recording
//! auth mode requires an address's authorization to be rooted at the top-level
//! invocation, so leaving it to the pool's own `require_auth` in a nested frame
//! fails with `Error(Auth, InvalidAction)`.
//!
//! Only the final output is slippage-checked (`min_out`); intermediate hops pass
//! `min_out = 0`.

use orbswap_pool_interface::PoolClient;
use soroban_sdk::{contract, contracterror, contractimpl, Address, Env, Vec};

#[cfg(test)]
mod test;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum RouterError {
    EmptyPath = 1,
    TokenNotInPool = 2,
    SlippageExceeded = 3,
    /// `tokens.len() != pools.len() + 1`.
    PathMismatch = 4,
}

#[contract]
pub struct OrbswapRouter;

#[contractimpl]
impl OrbswapRouter {
    /// Swap `amount_in` along `tokens` through `pools`, returning the final output.
    /// `user` authorizes; each hop delivers to `user`. Final output must be ≥ `min_out`.
    pub fn swap_exact_in(
        env: Env,
        user: Address,
        pools: Vec<Address>,
        tokens: Vec<Address>,
        amount_in: i128,
        min_out: i128,
        deadline: u64,
    ) -> Result<i128, RouterError> {
        // Must be the root of the auth tree — see the module docs.
        user.require_auth();
        check_path(&pools, &tokens)?;
        let mut cur_amount = amount_in;
        for i in 0..pools.len() {
            let pool = PoolClient::new(&env, &pools.get_unchecked(i));
            // Intermediate hops: no per-hop slippage (only the final output matters).
            cur_amount = pool.swap(
                &user,
                &tokens.get_unchecked(i),
                &cur_amount,
                &tokens.get_unchecked(i + 1),
                &0,
                &deadline,
            );
        }
        if cur_amount < min_out {
            return Err(RouterError::SlippageExceeded);
        }
        Ok(cur_amount)
    }

    /// Swap for an **exact** final `amount_out` of `token_out` through `pools`,
    /// paying at most `max_in`. Sizes each hop by quoting backward, then executes
    /// forward. Returns the total input charged.
    pub fn swap_exact_out(
        env: Env,
        user: Address,
        pools: Vec<Address>,
        tokens: Vec<Address>,
        amount_out: i128,
        max_in: i128,
        deadline: u64,
    ) -> Result<i128, RouterError> {
        // Must be the root of the auth tree — see the module docs.
        user.require_auth();
        check_path(&pools, &tokens)?;
        let n = pools.len();
        // Backward pass: size every hop from the required final output.
        // `rev_amt` is filled last-hop-first, so hop j sits at index n-1-j.
        let mut rev_amt: Vec<i128> = Vec::new(&env);
        let mut cur_out_amount = amount_out;
        let mut i = n;
        while i > 0 {
            i -= 1;
            let pool = PoolClient::new(&env, &pools.get_unchecked(i));
            let in_amt = pool.quote_exact_out(
                &tokens.get_unchecked(i),
                &tokens.get_unchecked(i + 1),
                &cur_out_amount,
            );
            rev_amt.push_back(cur_out_amount);
            cur_out_amount = in_amt;
        }
        // cur_out_amount is now the total input required at hop 0.
        if cur_out_amount > max_in {
            return Err(RouterError::SlippageExceeded);
        }
        // Forward execution: forward hop j ↔ reversed index (n-1-j).
        let mut total_in = 0i128;
        for j in 0..n {
            let pool = PoolClient::new(&env, &pools.get_unchecked(j));
            let paid = pool.swap_exact_out(
                &user,
                &tokens.get_unchecked(j),
                &tokens.get_unchecked(j + 1),
                &rev_amt.get_unchecked(n - 1 - j),
                &i128::MAX, // per-hop bound; the total is checked above
                &deadline,
            );
            if j == 0 {
                total_in = paid;
            }
        }
        Ok(total_in)
    }

    /// Quote a multi-hop swap without executing (view).
    pub fn quote_path(
        env: Env,
        pools: Vec<Address>,
        tokens: Vec<Address>,
        amount_in: i128,
    ) -> Result<i128, RouterError> {
        check_path(&pools, &tokens)?;
        let mut cur_amount = amount_in;
        for i in 0..pools.len() {
            let pool = PoolClient::new(&env, &pools.get_unchecked(i));
            cur_amount = pool.quote(
                &tokens.get_unchecked(i),
                &cur_amount,
                &tokens.get_unchecked(i + 1),
            );
        }
        Ok(cur_amount)
    }

    /// Convenience pass-through: deposit into a single pool.
    pub fn add_liquidity(
        env: Env,
        pool: Address,
        from: Address,
        amounts: Vec<i128>,
        min_shares: i128,
        deadline: u64,
    ) -> i128 {
        from.require_auth();
        PoolClient::new(&env, &pool).deposit(&from, &amounts, &min_shares, &deadline)
    }

    /// Convenience pass-through: withdraw from a single pool.
    pub fn remove_liquidity(
        env: Env,
        pool: Address,
        from: Address,
        shares: i128,
        min_amounts: Vec<i128>,
        deadline: u64,
    ) -> Vec<i128> {
        from.require_auth();
        PoolClient::new(&env, &pool).withdraw(&from, &shares, &min_amounts, &deadline)
    }
}

/// A path is well formed when every pool has an input and an output token.
fn check_path(pools: &Vec<Address>, tokens: &Vec<Address>) -> Result<(), RouterError> {
    if pools.is_empty() {
        return Err(RouterError::EmptyPath);
    }
    if tokens.len() != pools.len() + 1 {
        return Err(RouterError::PathMismatch);
    }
    Ok(())
}
