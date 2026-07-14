#![no_std]
//! Orbswap router — stateless multi-hop swap + liquidity orchestrator.
//!
//! A swap `path` is a list of **pool addresses**. The router chains
//! `pool.swap(user, …)` calls with `from = user` at every hop, so the **user**
//! holds the intermediate tokens (the router never takes custody) and the user's
//! authorization covers the whole invocation tree. Only the final output is
//! slippage-checked (`min_out`); intermediate hops pass `min_out = 0`.

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
}

#[contract]
pub struct OrbswapRouter;

#[contractimpl]
impl OrbswapRouter {
    /// Swap `amount_in` of `token_in` through `pools`, returning the final output.
    /// `user` authorizes; each hop delivers to `user`. Final output must be ≥ `min_out`.
    pub fn swap_exact_in(
        env: Env,
        user: Address,
        pools: Vec<Address>,
        token_in: Address,
        amount_in: i128,
        min_out: i128,
        deadline: u64,
    ) -> Result<i128, RouterError> {
        if pools.is_empty() {
            return Err(RouterError::EmptyPath);
        }
        let mut cur_token = token_in;
        let mut cur_amount = amount_in;
        for i in 0..pools.len() {
            let pool = PoolClient::new(&env, &pools.get_unchecked(i));
            let token_out = other_token(&pool, &cur_token)?;
            // Intermediate hops: no per-hop slippage (only the final output matters).
            cur_amount = pool.swap(&user, &cur_token, &cur_amount, &token_out, &0, &deadline);
            cur_token = token_out;
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
        token_out: Address,
        amount_out: i128,
        max_in: i128,
        deadline: u64,
    ) -> Result<i128, RouterError> {
        let n = pools.len();
        if n == 0 {
            return Err(RouterError::EmptyPath);
        }
        // Backward pass: reversed vectors (index 0 = last hop).
        let mut rev_in: Vec<Address> = Vec::new(&env);
        let mut rev_out: Vec<Address> = Vec::new(&env);
        let mut rev_amt: Vec<i128> = Vec::new(&env);
        let mut cur_out_token = token_out;
        let mut cur_out_amount = amount_out;
        let mut i = n;
        while i > 0 {
            i -= 1;
            let pool = PoolClient::new(&env, &pools.get_unchecked(i));
            let in_token = other_token(&pool, &cur_out_token)?;
            let in_amt = pool.quote_exact_out(&in_token, &cur_out_token, &cur_out_amount);
            rev_in.push_back(in_token.clone());
            rev_out.push_back(cur_out_token.clone());
            rev_amt.push_back(cur_out_amount);
            cur_out_token = in_token;
            cur_out_amount = in_amt;
        }
        // cur_out_amount is the total input required at hop 0.
        if cur_out_amount > max_in {
            return Err(RouterError::SlippageExceeded);
        }
        // Forward execution: forward hop j ↔ reversed index (n-1-j).
        let mut total_in = 0i128;
        for j in 0..n {
            let ri = n - 1 - j;
            let pool = PoolClient::new(&env, &pools.get_unchecked(j));
            let paid = pool.swap_exact_out(
                &user,
                &rev_in.get_unchecked(ri),
                &rev_out.get_unchecked(ri),
                &rev_amt.get_unchecked(ri),
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
        token_in: Address,
        amount_in: i128,
    ) -> Result<i128, RouterError> {
        if pools.is_empty() {
            return Err(RouterError::EmptyPath);
        }
        let mut cur_token = token_in;
        let mut cur_amount = amount_in;
        for i in 0..pools.len() {
            let pool = PoolClient::new(&env, &pools.get_unchecked(i));
            let token_out = other_token(&pool, &cur_token)?;
            cur_amount = pool.quote(&cur_token, &cur_amount, &token_out);
            cur_token = token_out;
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
        PoolClient::new(&env, &pool).withdraw(&from, &shares, &min_amounts, &deadline)
    }
}

/// The other token of a 2-token pool, given the current input token.
fn other_token(pool: &PoolClient, current: &Address) -> Result<Address, RouterError> {
    let cfg = pool.get_config();
    let a = cfg.tokens.get_unchecked(0);
    let b = cfg.tokens.get_unchecked(1);
    if current == &a {
        Ok(b)
    } else if current == &b {
        Ok(a)
    } else {
        Err(RouterError::TokenNotInPool)
    }
}
