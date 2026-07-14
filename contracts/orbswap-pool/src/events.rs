//! Contract events for indexers (soroban `#[contractevent]`).

use soroban_sdk::{contractevent, Address, Env, Symbol, Vec};

#[contractevent]
pub struct Deposit {
    pub from: Address,
    pub shares: i128,
    pub amounts: Vec<i128>,
}

#[contractevent]
pub struct Withdraw {
    pub from: Address,
    pub shares: i128,
    pub amounts: Vec<i128>,
}

#[contractevent]
pub struct Swap {
    pub from: Address,
    pub token_in: Address,
    pub amount_in: i128,
    pub token_out: Address,
    pub amount_out: i128,
}

#[contractevent]
pub struct PauseChanged {
    pub what: Symbol,
    pub paused: bool,
}

#[contractevent]
pub struct ProtocolFeeCollected {
    pub to: Address,
    pub amounts: Vec<i128>,
}

#[contractevent]
pub struct TokenAllowed {
    pub token: Address,
    pub allowed: bool,
}

#[contractevent]
pub struct SharesTransferred {
    pub from: Address,
    pub to: Address,
    pub amount: i128,
}

#[contractevent]
pub struct TickCrossed {
    pub tick: u32,
    pub up: bool,
    pub active_liquidity: i128,
}

pub fn deposit(env: &Env, from: &Address, shares: i128, amounts: &Vec<i128>) {
    Deposit {
        from: from.clone(),
        shares,
        amounts: amounts.clone(),
    }
    .publish(env);
}

pub fn withdraw(env: &Env, from: &Address, shares: i128, amounts: &Vec<i128>) {
    Withdraw {
        from: from.clone(),
        shares,
        amounts: amounts.clone(),
    }
    .publish(env);
}

pub fn swap(
    env: &Env,
    from: &Address,
    token_in: &Address,
    amount_in: i128,
    token_out: &Address,
    amount_out: i128,
) {
    Swap {
        from: from.clone(),
        token_in: token_in.clone(),
        amount_in,
        token_out: token_out.clone(),
        amount_out,
    }
    .publish(env);
}

pub fn paused(env: &Env, what: Symbol, paused: bool) {
    PauseChanged { what, paused }.publish(env);
}

pub fn protocol_collected(env: &Env, to: &Address, amounts: &Vec<i128>) {
    ProtocolFeeCollected {
        to: to.clone(),
        amounts: amounts.clone(),
    }
    .publish(env);
}

pub fn token_allowed(env: &Env, token: &Address, allowed: bool) {
    TokenAllowed {
        token: token.clone(),
        allowed,
    }
    .publish(env);
}

pub fn shares_transferred(env: &Env, from: &Address, to: &Address, amount: i128) {
    SharesTransferred {
        from: from.clone(),
        to: to.clone(),
        amount,
    }
    .publish(env);
}

pub fn tick_crossed(env: &Env, tick: u32, up: bool, active_liquidity: i128) {
    TickCrossed {
        tick,
        up,
        active_liquidity,
    }
    .publish(env);
}
