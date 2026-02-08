use std::{collections::BTreeMap, fs, path::PathBuf};

use marlowe_api::{
    parse_contract_yaml, simulate_transaction, AccountId, SimError, SimState, SimTransaction,
    SimTransactionResult,
};
use num_bigint::BigInt;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    expectations: BTreeMap<String, Expectation>,
}

#[derive(Debug, Deserialize)]
struct Expectation {
    expected: String,
    error_code: Option<String>,
    payments: Option<usize>,
    first_payment_amount: Option<String>,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simulator_expectations.yaml")
}

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("examples")
        .join(name)
}

fn base_state_for(example: &str) -> SimState {
    match example {
        "simple-pay.yaml" => {
            let mut state = SimState::default();
            state.accounts.insert(
                AccountId {
                    owner: marlowe_api::ast::Party::Role("alice".to_owned()),
                    token: marlowe_api::ast::Token::Token {
                        currency_symbol: "".to_owned(),
                        token_name: "".to_owned(),
                    },
                },
                BigInt::from(10),
            );
            state
        }
        _ => SimState::default(),
    }
}

fn error_code(error: &SimError) -> &'static str {
    match error {
        SimError::NotReadyToRun => "NotReadyToRun",
        SimError::InvalidInterval { .. } => "InvalidInterval",
        SimError::IntervalInPast { .. } => "IntervalInPast",
        SimError::AmbiguousTimeInterval => "AmbiguousTimeInterval",
        SimError::NoMatchForInput { .. } => "NoMatchForInput",
        SimError::UselessTransaction => "UselessTransaction",
    }
}

#[test]
fn simulator_matches_fixture_expectations() {
    let fixture_text = fs::read_to_string(fixture_path()).expect("fixture exists");
    let fixture: Fixture = serde_yaml::from_str(&fixture_text).expect("fixture parses");

    for (example, expectation) in fixture.expectations {
        let yaml = fs::read_to_string(example_path(&example)).expect("example exists");
        let contract = parse_contract_yaml(&yaml).expect("example parses");
        let state = base_state_for(&example);
        let tx = SimTransaction {
            interval_start: BigInt::from(0),
            interval_end: BigInt::from(1_000_000_000_000i64),
            inputs: vec![],
        };

        let result = simulate_transaction(&contract, &state, &tx);
        match (expectation.expected.as_str(), result) {
            ("error", SimTransactionResult::Error(err)) => {
                let expected_code = expectation.error_code.expect("error_code required");
                assert_eq!(
                    error_code(&err),
                    expected_code,
                    "error mismatch for {example}"
                );
            }
            ("success", SimTransactionResult::Success(success)) => {
                if let Some(expected_payments) = expectation.payments {
                    assert_eq!(
                        success.payments.len(),
                        expected_payments,
                        "payments mismatch for {example}"
                    );
                }
                if let Some(amount) = expectation.first_payment_amount {
                    assert_eq!(
                        success.payments[0].amount.to_string(),
                        amount,
                        "first payment mismatch for {example}"
                    );
                }
            }
            (expected, got) => panic!("expected {expected} for {example}, got {got:?}"),
        }
    }
}
