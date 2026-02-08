use std::collections::BTreeMap;

use marlowe_api::{
    parse_contract_yaml, preview_inputs, simulate_transaction, simulate_transaction_with_trace,
    AccountId, SimError, SimInput, SimState, SimTransaction, SimTransactionResult, TraceReduceRule,
    TraceStep, TransactionWarning,
};
use num_bigint::BigInt;

fn parse(yaml: &str) -> marlowe_api::ast::Contract {
    parse_contract_yaml(yaml).expect("contract parses")
}

fn b(n: i64) -> BigInt {
    BigInt::from(n)
}

fn tx(start: i64, end: i64, inputs: Vec<SimInput>) -> SimTransaction {
    SimTransaction {
        interval_start: b(start),
        interval_end: b(end),
        inputs,
    }
}

#[test]
fn rejects_not_ready_contract() {
    let contract = parse(
        r#"
When:
  cases: []
  timeout: $deadline
  timeout_continuation: { Close: {} }
"#,
    );

    let result = simulate_transaction(&contract, &SimState::default(), &tx(0, 10, vec![]));
    assert_eq!(result, SimTransactionResult::Error(SimError::NotReadyToRun));
}

#[test]
fn invalid_interval_is_rejected() {
    let contract = parse("Close: {}\n");
    let result = simulate_transaction(&contract, &SimState::default(), &tx(10, 9, vec![]));
    assert!(matches!(
        result,
        SimTransactionResult::Error(SimError::InvalidInterval { .. })
    ));
}

#[test]
fn interval_in_past_is_rejected() {
    let contract = parse("Close: {}\n");
    let state = SimState {
        min_time: b(50),
        ..SimState::default()
    };
    let result = simulate_transaction(&contract, &state, &tx(10, 20, vec![]));
    assert!(matches!(
        result,
        SimTransactionResult::Error(SimError::IntervalInPast { .. })
    ));
}

#[test]
fn ambiguous_when_interval_is_rejected() {
    let contract = parse(
        r#"
When:
  cases: []
  timeout: { Timeout: 10 }
  timeout_continuation: { Close: {} }
"#,
    );
    let result = simulate_transaction(&contract, &SimState::default(), &tx(0, 10, vec![]));
    assert_eq!(
        result,
        SimTransactionResult::Error(SimError::AmbiguousTimeInterval)
    );
}

#[test]
fn timeout_continuation_activates_when_start_passes_timeout() {
    let contract = parse(
        r#"
When:
  cases: []
  timeout: { Timeout: 10 }
  timeout_continuation: { Close: {} }
"#,
    );

    let result = simulate_transaction(&contract, &SimState::default(), &tx(10, 12, vec![]));
    match result {
        SimTransactionResult::Success(success) => {
            assert_eq!(success.contract, parse("Close: {}\n"));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn no_match_input_is_rejected() {
    let contract = parse(
        r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#,
    );

    let input = SimInput::Deposit {
        into: marlowe_api::ast::Party::Role("Alice".to_owned()),
        by: marlowe_api::ast::Party::Role("Alice".to_owned()),
        token: marlowe_api::ast::Token::Token {
            currency_symbol: "".to_owned(),
            token_name: "".to_owned(),
        },
        amount: b(4),
    };

    let result = simulate_transaction(&contract, &SimState::default(), &tx(0, 50, vec![input]));
    assert_eq!(
        result,
        SimTransactionResult::Error(SimError::NoMatchForInput { input_index: 0 })
    );
}

#[test]
fn non_positive_deposit_generates_warning() {
    let contract = parse(
        r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 0 }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#,
    );

    let result = simulate_transaction(
        &contract,
        &SimState::default(),
        &tx(
            0,
            50,
            vec![SimInput::Deposit {
                into: marlowe_api::ast::Party::Role("Alice".to_owned()),
                by: marlowe_api::ast::Party::Role("Alice".to_owned()),
                token: marlowe_api::ast::Token::Token {
                    currency_symbol: "".to_owned(),
                    token_name: "".to_owned(),
                },
                amount: b(0),
            }],
        ),
    );

    match result {
        SimTransactionResult::Success(success) => {
            assert!(success
                .warnings
                .iter()
                .any(|w| matches!(w, TransactionWarning::NonPositiveDeposit { .. })));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn partial_pay_warning_and_payment() {
    let contract = parse(
        r#"
Pay:
  from: { Role: "Alice" }
  to_party: { Role: "Bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 10 }
  then: { Close: {} }
"#,
    );

    let account = AccountId {
        owner: marlowe_api::ast::Party::Role("Alice".to_owned()),
        token: marlowe_api::ast::Token::Token {
            currency_symbol: "".to_owned(),
            token_name: "".to_owned(),
        },
    };
    let mut accounts = BTreeMap::new();
    accounts.insert(account, b(6));

    let state = SimState {
        accounts,
        ..SimState::default()
    };

    let result = simulate_transaction(&contract, &state, &tx(0, 1, vec![]));
    match result {
        SimTransactionResult::Success(success) => {
            assert!(success
                .warnings
                .iter()
                .any(|w| matches!(w, TransactionWarning::PartialPay { .. })));
            assert_eq!(success.payments.len(), 1);
            assert_eq!(success.payments[0].amount, b(6));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn let_shadowing_warning_is_reported() {
    let contract = parse(
        r#"
Let:
  name: "x"
  value: { Constant: 1 }
  then:
    Let:
      name: "x"
      value: { Constant: 2 }
      then: { Close: {} }
"#,
    );

    let result = simulate_transaction(&contract, &SimState::default(), &tx(0, 1, vec![]));
    match result {
        SimTransactionResult::Success(success) => {
            assert!(success
                .warnings
                .iter()
                .any(|w| matches!(w, TransactionWarning::Shadowing { .. })));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn assertion_failed_warning_is_reported() {
    let contract = parse(
        r#"
Assert:
  cond: { "False": {} }
  then: { Close: {} }
"#,
    );
    let result = simulate_transaction(&contract, &SimState::default(), &tx(0, 1, vec![]));
    match result {
        SimTransactionResult::Success(success) => {
            assert!(success
                .warnings
                .iter()
                .any(|w| matches!(w, TransactionWarning::AssertionFailed)));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn close_refunds_account() {
    let contract = parse("Close: {}\n");
    let account = AccountId {
        owner: marlowe_api::ast::Party::Role("Alice".to_owned()),
        token: marlowe_api::ast::Token::Token {
            currency_symbol: "".to_owned(),
            token_name: "".to_owned(),
        },
    };

    let mut state = SimState::default();
    state.accounts.insert(account, b(7));

    let result = simulate_transaction(&contract, &state, &tx(0, 1, vec![]));
    match result {
        SimTransactionResult::Success(success) => {
            assert_eq!(success.payments.len(), 1);
            assert_eq!(success.payments[0].amount, b(7));
            assert!(success.state.accounts.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn useless_transaction_is_reported() {
    let contract = parse(
        r#"
When:
  cases: []
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#,
    );
    let result = simulate_transaction(&contract, &SimState::default(), &tx(0, 10, vec![]));
    assert_eq!(
        result,
        SimTransactionResult::Error(SimError::UselessTransaction)
    );
}

#[test]
fn deposit_then_close_succeeds() {
    let contract = parse(
        r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#,
    );

    let input = SimInput::Deposit {
        into: marlowe_api::ast::Party::Role("Alice".to_owned()),
        by: marlowe_api::ast::Party::Role("Alice".to_owned()),
        token: marlowe_api::ast::Token::Token {
            currency_symbol: "".to_owned(),
            token_name: "".to_owned(),
        },
        amount: b(5),
    };

    let result = simulate_transaction(&contract, &SimState::default(), &tx(0, 50, vec![input]));
    match result {
        SimTransactionResult::Success(success) => {
            assert_eq!(success.payments.len(), 1);
            assert_eq!(success.payments[0].amount, b(5));
            assert!(success.state.accounts.is_empty());
            assert_eq!(success.contract, parse("Close: {}\n"));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn division_by_zero_evaluates_to_zero() {
    let contract = parse(
        r#"
Pay:
  from: { Role: "Alice" }
  to_party: { Role: "Bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount:
    Div:
      - { Constant: 10 }
      - { Constant: 0 }
  then: { Close: {} }
"#,
    );

    let mut state = SimState::default();
    state.accounts.insert(
        AccountId {
            owner: marlowe_api::ast::Party::Role("Alice".to_owned()),
            token: marlowe_api::ast::Token::Token {
                currency_symbol: "".to_owned(),
                token_name: "".to_owned(),
            },
        },
        b(50),
    );

    let result = simulate_transaction(&contract, &state, &tx(0, 1, vec![]));
    match result {
        SimTransactionResult::Success(success) => {
            assert!(success
                .warnings
                .iter()
                .any(|w| matches!(w, TransactionWarning::NonPositivePay { .. })));
            assert_eq!(success.payments.len(), 1);
            assert_eq!(success.payments[0].amount, b(50));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn interval_start_is_clamped_to_min_time() {
    let contract = parse(
        r#"
Pay:
  from: { Role: "Alice" }
  to_party: { Role: "Bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { TimeIntervalStart: {} }
  then: { Close: {} }
"#,
    );

    let mut state = SimState {
        min_time: b(100),
        ..SimState::default()
    };
    state.accounts.insert(
        AccountId {
            owner: marlowe_api::ast::Party::Role("Alice".to_owned()),
            token: marlowe_api::ast::Token::Token {
                currency_symbol: "".to_owned(),
                token_name: "".to_owned(),
            },
        },
        b(1000),
    );

    let result = simulate_transaction(&contract, &state, &tx(50, 120, vec![]));
    match result {
        SimTransactionResult::Success(success) => {
            assert_eq!(success.payments[0].amount, b(100));
            assert_eq!(success.state.min_time, b(100));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn trace_mode_records_reduce_and_input_steps() {
    let contract = parse(
        r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
        then:
          Pay:
            from: { Role: "Alice" }
            to_party: { Role: "Bob" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
            then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#,
    );

    let input = SimInput::Deposit {
        into: marlowe_api::ast::Party::Role("Alice".to_owned()),
        by: marlowe_api::ast::Party::Role("Alice".to_owned()),
        token: marlowe_api::ast::Token::Token {
            currency_symbol: "".to_owned(),
            token_name: "".to_owned(),
        },
        amount: b(5),
    };

    let result = simulate_transaction_with_trace(
        &contract,
        &SimState::default(),
        &tx(0, 10, vec![input]),
        true,
    );
    match result {
        SimTransactionResult::Success(success) => {
            assert!(success.trace.iter().any(|step| matches!(
                step,
                TraceStep::InputApplied {
                    input_index: 0,
                    contract_path,
                    ..
                } if contract_path == "$.cases[0]"
            )));
            assert!(success.trace.iter().any(|step| matches!(
                step,
                TraceStep::Reduced {
                    rule: TraceReduceRule::Pay,
                    contract_path,
                    ..
                } if contract_path == "$"
            )));
            assert!(success.trace.iter().any(|step| matches!(
                step,
                TraceStep::InputApplied {
                    delta: Some(delta),
                    ..
                } if !delta.accounts_upserted.is_empty()
            )));
            assert!(success.trace.iter().any(|step| matches!(
                step,
                TraceStep::Reduced {
                    rule: TraceReduceRule::Pay,
                    delta: Some(delta),
                    ..
                } if !delta.accounts_removed.is_empty()
            )));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn trace_mode_reports_bound_value_upsert_for_let() {
    let contract = parse(
        r#"
Let:
  name: "x"
  value: { Constant: 7 }
  then: { Close: {} }
"#,
    );

    let result =
        simulate_transaction_with_trace(&contract, &SimState::default(), &tx(0, 10, vec![]), true);
    match result {
        SimTransactionResult::Success(success) => {
            assert!(success.trace.iter().any(|step| matches!(
                step,
                TraceStep::Reduced {
                    rule: TraceReduceRule::Let,
                    delta: Some(delta),
                    ..
                } if delta.bound_values_upserted.iter().any(|entry| entry.name == "x")
            )));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn trace_mode_timeout_step_has_no_state_delta() {
    let contract = parse(
        r#"
When:
  cases: []
  timeout: { Timeout: 10 }
  timeout_continuation: { Close: {} }
"#,
    );

    let result =
        simulate_transaction_with_trace(&contract, &SimState::default(), &tx(10, 12, vec![]), true);
    match result {
        SimTransactionResult::Success(success) => {
            assert!(success.trace.iter().any(|step| matches!(
                step,
                TraceStep::Reduced {
                    rule: TraceReduceRule::WhenTimeout,
                    delta: None,
                    ..
                }
            )));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn preview_includes_reduction_warnings() {
    let contract = parse(
        r#"
Assert:
  cond: { "False": {} }
  then:
    When:
      cases: []
      timeout: { Timeout: 100 }
      timeout_continuation: { Close: {} }
"#,
    );

    let preview = preview_inputs(&contract, &SimState::default(), &b(0), &b(10)).expect("preview");
    assert!(preview
        .warnings
        .iter()
        .any(|w| matches!(w, TransactionWarning::AssertionFailed)));
}
