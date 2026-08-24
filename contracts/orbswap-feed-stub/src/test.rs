use super::*;
use soroban_sdk::testutils::Address as _;

fn setup(decimals: u32) -> (Env, FeedStubClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let base = Address::generate(&env);
    let id = env.register(FeedStub, ());
    let client = FeedStubClient::new(&env, &id);
    client.initialize(&admin, &decimals, &base);
    (env, client, admin, base)
}

#[test]
fn reports_configured_decimals_and_base() {
    let (_, c, _, base) = setup(14);
    assert_eq!(c.decimals(), 14);
    assert_eq!(c.base(), Asset::Stellar(base));
}

#[test]
fn set_price_then_lastprice_round_trips() {
    let (env, c, _, _) = setup(14);
    let asset = Address::generate(&env);
    c.set_price(&asset, &123_456);
    let p = c.lastprice(&Asset::Stellar(asset)).unwrap();
    assert_eq!(p.price, 123_456);
    assert_eq!(p.timestamp, env.ledger().timestamp());
}

#[test]
fn unknown_asset_returns_none() {
    let (env, c, _, _) = setup(14);
    assert!(c
        .lastprice(&Asset::Stellar(Address::generate(&env)))
        .is_none());
}

#[test]
fn other_asset_kind_returns_none() {
    let (env, c, _, _) = setup(14);
    assert!(c
        .lastprice(&Asset::Other(soroban_sdk::Symbol::new(&env, "XLM")))
        .is_none());
}

#[test]
fn rejects_non_positive_price() {
    let (env, c, _, _) = setup(14);
    let asset = Address::generate(&env);
    assert!(c.try_set_price(&asset, &0).is_err());
    assert!(c.try_set_price(&asset, &-1).is_err());
    let _ = env;
}

#[test]
fn cannot_initialize_twice() {
    let (env, c, admin, base) = setup(14);
    assert!(c.try_initialize(&admin, &14, &base).is_err());
    let _ = env;
}

#[test]
fn assets_lists_everything_priced() {
    let (env, c, _, _) = setup(14);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    c.set_price(&a, &1);
    c.set_price(&b, &2);
    assert_eq!(c.assets().len(), 2);
}
