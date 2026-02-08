use std::{collections::HashSet, fs, path::PathBuf};

use marlowe_api::{
    ast::{Contract, Timeout},
    parse_contract_yaml, type_check, ChoiceRef, PartyRef, TokenRef, TypeCheckContext,
};

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("examples")
        .join(name)
}

fn parse_example(name: &str) -> Contract {
    let content = fs::read_to_string(example_path(name)).expect("example exists");
    parse_contract_yaml(&content).expect("example parses")
}

#[test]
fn parses_simple_pay_contract() {
    let contract = parse_example("simple-pay.yaml");
    assert!(matches!(contract, Contract::Pay { .. }));
}

#[test]
fn parser_rejects_param_in_contract_position() {
    let yaml = "If:\n  cond: { \"True\": {} }\n  then: $later\n  else: { Close: {} }\n";
    let error = parse_contract_yaml(yaml).expect_err("contract param should fail");
    assert!(error.message.contains("parameter"));
    assert!(error.message.contains("Contract position"));
}

#[test]
fn typecheck_marks_simple_contract_ready() {
    let contract = parse_example("simple-pay.yaml");
    let result = type_check(&contract, &TypeCheckContext::default());
    assert!(result.ready_to_run);
    assert!(result.errors.is_empty());
    assert!(result.params.is_empty());
    assert!(result.holes.is_empty());
}

#[test]
fn typecheck_lists_params_and_holes() {
    let contract = parse_example("with-params.yaml");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert!(!result.ready_to_run);
    assert_eq!(result.params.len(), 2);
    assert_eq!(result.params[0].name, "deadline");
    assert_eq!(result.params[1].name, "deposit_amt");
    assert!(result.holes.is_empty());

    let contract_with_holes = parse_example("with-holes.yaml");
    let holes_result = type_check(&contract_with_holes, &TypeCheckContext::default());
    assert!(!holes_result.ready_to_run);
    assert_eq!(holes_result.holes.len(), 3);
}

#[test]
fn typecheck_detects_conflicting_hole_types() {
    let yaml = r#"
When:
  cases: []
  timeout: ?shared
  timeout_continuation:
    Let:
      name: "x"
      value: ?shared
      then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert!(!result.errors.is_empty());
    assert!(result.errors[0]
        .message
        .contains("conflicting inferred types"));
}

#[test]
fn typecheck_rejects_undefined_use_value() {
    let yaml = r#"
Pay:
  from: { Role: "alice" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { UseValue: "missing" }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("undefined let binding"));
}

#[test]
fn typecheck_emits_let_warnings() {
    let yaml = r#"
Let:
  name: "x"
  value: { Constant: 1 }
  then:
    Let:
      name: "x"
      value: { Constant: 2 }
      then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("shadows")));
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("never used")));
}

#[test]
fn typecheck_validates_bound_ranges() {
    let yaml = r#"
When:
  cases:
    - Case:
        action:
          Choice:
            id: { ChoiceId: { name: "c", party: { Role: "alice" } } }
            bounds:
              - Bound: { from: { Constant: 10 }, to: { Constant: 2 } }
        then: { Close: {} }
  timeout: { Timeout: 1 }
  timeout_continuation: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert!(result
        .errors
        .iter()
        .any(|e| e.message.contains("invalid bound range")));
}

#[test]
fn typecheck_warns_when_bound_not_static() {
    let yaml = r#"
When:
  cases:
    - Case:
        action:
          Choice:
            id: { ChoiceId: { name: "c", party: { Role: "alice" } } }
            bounds:
              - Bound: { from: $lo, to: { Constant: 2 } }
        then: { Close: {} }
  timeout: { Timeout: 1 }
  timeout_continuation: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("cannot be statically validated")));
}

#[test]
fn context_checks_unknown_definitions() {
    let contract = parse_example("simple-pay.yaml");
    let context = TypeCheckContext {
        require_known_definitions: true,
        ..TypeCheckContext::default()
    };

    let result = type_check(&contract, &context);
    assert!(!result.errors.is_empty());
    assert!(result.errors.iter().any(|e| e.message.contains("unknown")));
}

#[test]
fn context_checks_pass_with_known_definitions() {
    let contract = parse_example("simple-pay.yaml");

    let mut known_accounts = HashSet::new();
    known_accounts.insert(PartyRef::Role("alice".to_owned()));

    let mut known_parties = HashSet::new();
    known_parties.insert(PartyRef::Role("bob".to_owned()));

    let mut known_tokens = HashSet::new();
    known_tokens.insert(TokenRef {
        currency_symbol: "".to_owned(),
        token_name: "".to_owned(),
    });

    let context = TypeCheckContext {
        known_accounts,
        known_parties,
        known_tokens,
        known_choices: HashSet::new(),
        require_known_definitions: true,
    };

    let result = type_check(&contract, &context);
    assert!(result.errors.is_empty());
}

#[test]
fn examples_parse_and_typecheck() {
    let cases = [
        "choice-with-let.yaml",
        "escrow.yaml",
        "loan-zero-coupon.yaml",
        "simple-deposit.yaml",
        "simple-pay.yaml",
        "swap.yaml",
        "with-holes.yaml",
        "with-params.yaml",
    ];

    for example in cases {
        let contract = parse_example(example);
        let result = type_check(&contract, &TypeCheckContext::default());
        assert!(
            result.errors.is_empty(),
            "{example} has type errors: {:?}",
            result.errors
        );
    }
}

#[test]
fn parser_supports_escaped_literals() {
    let yaml = r#"
Let:
  name: "$$budget"
  value: { Constant: "42" }
  then:
    Pay:
      from: { Role: "??alice" }
      to_party: { Role: "bob" }
      token: { Token: { currency_symbol: "", token_name: "" } }
      amount: { UseValue: "$budget" }
      then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert!(result.errors.is_empty());
}

#[test]
fn parser_accepts_timeout_param_shorthand() {
    let yaml = r#"
When:
  cases: []
  timeout: $deadline
  timeout_continuation: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    match contract {
        Contract::When { timeout, .. } => {
            assert!(matches!(timeout, Timeout::Param(name) if name == "deadline"));
        }
        _ => panic!("expected when"),
    }
}

#[test]
fn typecheck_choice_usage_works_with_context_choices() {
    let yaml = r#"
Assert:
  cond:
    ChoseSomething:
      ChoiceId:
        name: "oracle"
        party: { Role: "arbiter" }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("valid yaml");
    let mut context = TypeCheckContext {
        require_known_definitions: true,
        ..TypeCheckContext::default()
    };
    context
        .known_parties
        .insert(PartyRef::Role("arbiter".to_owned()));
    context.known_choices.insert(ChoiceRef {
        name: "oracle".to_owned(),
        party: PartyRef::Role("arbiter".to_owned()),
    });

    let result = type_check(&contract, &context);
    assert!(result.errors.is_empty());
}
