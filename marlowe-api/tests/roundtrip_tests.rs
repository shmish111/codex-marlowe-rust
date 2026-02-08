use std::fs;

use marlowe_api::{
    ast::{
        Action, Bound, Case, ChoiceId, Contract, Observation, Party, PayeeTarget, Timeout, Token,
        Value,
    },
    contract_to_yaml_string, parse_contract_yaml,
};
use num_bigint::BigInt;
use proptest::prelude::*;

fn ident() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[A-Za-z_][A-Za-z0-9_]{0,8}").expect("valid regex")
}

fn party_strategy() -> impl Strategy<Value = Party> {
    prop_oneof![
        ident().prop_map(Party::Role),
        ident().prop_map(|s| Party::Address(format!("addr_{s}"))),
        ident().prop_map(Party::Hole),
    ]
}

fn token_strategy() -> impl Strategy<Value = Token> {
    prop_oneof![
        (ident(), ident()).prop_map(|(currency_symbol, token_name)| Token::Token {
            currency_symbol,
            token_name
        }),
        ident().prop_map(Token::Hole),
    ]
}

fn choice_id_strategy() -> impl Strategy<Value = ChoiceId> {
    prop_oneof![
        (ident(), party_strategy()).prop_map(|(name, party)| ChoiceId::ChoiceId { name, party }),
        ident().prop_map(ChoiceId::Hole),
    ]
}

fn value_leaf_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i64>().prop_map(|n| Value::Constant(BigInt::from(n))),
        ident().prop_map(Value::Param),
        ident().prop_map(Value::Hole),
        ident().prop_map(Value::UseValue),
        choice_id_strategy().prop_map(Value::ValueFromChoice),
        (party_strategy(), token_strategy())
            .prop_map(|(account, token)| Value::AvailableMoney { account, token }),
        Just(Value::TimeIntervalStart),
        Just(Value::TimeIntervalEnd),
    ]
}

fn value_strategy() -> impl Strategy<Value = Value> {
    value_leaf_strategy().prop_recursive(4, 32, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Value::Add(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Value::Sub(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Value::Mul(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Value::Div(Box::new(a), Box::new(b))),
            inner.clone().prop_map(|x| Value::Neg(Box::new(x))),
            inner.clone().prop_map(|x| Value::Abs(Box::new(x))),
        ]
    })
}

fn observation_leaf_strategy() -> impl Strategy<Value = Observation> {
    prop_oneof![
        Just(Observation::True),
        Just(Observation::False),
        ident().prop_map(Observation::Hole),
        choice_id_strategy().prop_map(Observation::ChoseSomething),
    ]
}

fn observation_strategy() -> impl Strategy<Value = Observation> {
    observation_leaf_strategy().prop_recursive(4, 32, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| Observation::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| Observation::Or(Box::new(a), Box::new(b))),
            inner.clone().prop_map(|x| Observation::Not(Box::new(x))),
            (value_strategy(), value_strategy())
                .prop_map(|(a, b)| Observation::ValueGE(Box::new(a), Box::new(b))),
            (value_strategy(), value_strategy())
                .prop_map(|(a, b)| Observation::ValueGT(Box::new(a), Box::new(b))),
            (value_strategy(), value_strategy())
                .prop_map(|(a, b)| Observation::ValueLT(Box::new(a), Box::new(b))),
            (value_strategy(), value_strategy())
                .prop_map(|(a, b)| Observation::ValueLE(Box::new(a), Box::new(b))),
            (value_strategy(), value_strategy())
                .prop_map(|(a, b)| Observation::ValueEQ(Box::new(a), Box::new(b))),
        ]
    })
}

fn timeout_strategy() -> impl Strategy<Value = Timeout> {
    prop_oneof![
        any::<i64>().prop_map(|n| Timeout::PosixTime(BigInt::from(n))),
        ident().prop_map(Timeout::Param),
        ident().prop_map(Timeout::Hole),
    ]
}

fn bound_strategy() -> impl Strategy<Value = Bound> {
    prop_oneof![
        ident().prop_map(Bound::Hole),
        (value_strategy(), value_strategy()).prop_map(|(from, to)| Bound::Bound { from, to }),
    ]
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        ident().prop_map(Action::Hole),
        (
            party_strategy(),
            party_strategy(),
            token_strategy(),
            value_strategy()
        )
            .prop_map(|(into, by, token, amount)| Action::Deposit {
                into,
                by,
                token,
                amount,
            }),
        (
            choice_id_strategy(),
            prop::collection::vec(bound_strategy(), 0..3)
        )
            .prop_map(|(id, bounds)| Action::Choice { id, bounds }),
        observation_strategy().prop_map(|if_| Action::Notify { if_ }),
    ]
}

fn payee_strategy() -> impl Strategy<Value = PayeeTarget> {
    prop_oneof![
        party_strategy().prop_map(PayeeTarget::ToParty),
        party_strategy().prop_map(PayeeTarget::ToAccount),
    ]
}

fn case_strategy(
    contract: impl Strategy<Value = Contract> + Clone + 'static,
) -> impl Strategy<Value = Case> {
    prop_oneof![
        ident().prop_map(Case::Hole),
        (action_strategy(), contract).prop_map(|(action, then)| Case::Case {
            action,
            then: Box::new(then)
        }),
    ]
}

fn contract_strategy() -> impl Strategy<Value = Contract> {
    let leaf = prop_oneof![ident().prop_map(Contract::Hole), Just(Contract::Close),];

    leaf.prop_recursive(4, 48, 2, |inner| {
        prop_oneof![
            (
                party_strategy(),
                payee_strategy(),
                token_strategy(),
                value_strategy(),
                inner.clone()
            )
                .prop_map(|(from, to, token, amount, then)| Contract::Pay {
                    from,
                    to,
                    token,
                    amount,
                    then: Box::new(then),
                }),
            (observation_strategy(), inner.clone(), inner.clone()).prop_map(
                |(cond, then, else_)| Contract::If {
                    cond,
                    then: Box::new(then),
                    else_: Box::new(else_),
                }
            ),
            (
                prop::collection::vec(case_strategy(inner.clone()), 0..3),
                timeout_strategy(),
                inner.clone()
            )
                .prop_map(|(cases, timeout, timeout_continuation)| Contract::When {
                    cases,
                    timeout,
                    timeout_continuation: Box::new(timeout_continuation),
                }),
            (ident(), value_strategy(), inner.clone()).prop_map(|(name, value, then)| {
                Contract::Let {
                    name,
                    value,
                    then: Box::new(then),
                }
            }),
            (observation_strategy(), inner.clone()).prop_map(|(cond, then)| Contract::Assert {
                cond,
                then: Box::new(then),
            }),
        ]
    })
}

#[test]
fn examples_round_trip_parse_serialize_parse() {
    let examples_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("examples");

    let entries = fs::read_dir(examples_dir).expect("examples dir exists");
    for entry in entries {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
            continue;
        }
        let input = fs::read_to_string(&path).expect("example readable");
        let ast = parse_contract_yaml(&input).expect("example parses");
        let yaml = contract_to_yaml_string(&ast).expect("serialize yaml");
        let reparsed = parse_contract_yaml(&yaml).expect("reparse yaml");
        assert_eq!(
            ast,
            reparsed,
            "round trip mismatch for {:?}",
            path.file_name()
        );
    }
}

proptest! {
    #[test]
    fn generated_contract_round_trip(contract in contract_strategy()) {
        let yaml = contract_to_yaml_string(&contract).expect("serialize");
        let reparsed = parse_contract_yaml(&yaml).expect("reparse");
        prop_assert_eq!(contract, reparsed);
    }
}
