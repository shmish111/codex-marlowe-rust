use std::collections::HashSet;

use marlowe_api::{parse_contract_yaml, type_check, PartyRef, TokenRef, TypeCheckContext};

#[test]
fn golden_parse_error_unknown_constructor() {
    let yaml = r#"
Mystery:
  value: 1
"#;

    let err = parse_contract_yaml(yaml).expect_err("should fail parsing");
    assert_eq!(err.path, "$".to_owned());
    assert_eq!(
        err.message,
        "unknown Contract constructor 'Mystery'".to_owned()
    );
}

#[test]
fn golden_parse_error_strict_param_position() {
    let yaml = r#"
Pay:
  from: "$alice"
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    let err = parse_contract_yaml(yaml).expect_err("strict param should fail");
    assert_eq!(err.path, "$.Pay.from".to_owned());
    assert_eq!(
        err.message,
        "parameter '$alice' is not valid in Party position".to_owned()
    );
}

#[test]
fn golden_type_error_undefined_let() {
    let yaml = r#"
Pay:
  from: { Role: "alice" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { UseValue: "missing" }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");
    let result = type_check(&contract, &TypeCheckContext::default());

    let diagnostics: Vec<(String, String)> = result
        .errors
        .iter()
        .map(|d| (d.path.clone(), d.message.clone()))
        .collect();

    assert_eq!(
        diagnostics,
        vec![(
            "$.amount".to_owned(),
            "UseValue references undefined let binding 'missing'".to_owned(),
        )]
    );
}

#[test]
fn golden_type_error_conflicting_hole_types() {
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

    let contract = parse_contract_yaml(yaml).expect("parses");
    let result = type_check(&contract, &TypeCheckContext::default());

    assert_eq!(result.errors.len(), 1);
    assert_eq!(
        result.errors[0].path,
        "$.timeout_continuation.value".to_owned()
    );
    assert_eq!(
        result.errors[0].message,
        "hole 'shared' has conflicting inferred types: Timeout at $.timeout and Value at $.timeout_continuation.value".to_owned(),
    );
}

#[test]
fn golden_type_error_unknown_context_definitions() {
    let yaml = r#"
Pay:
  from: { Role: "alice" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");
    let context = TypeCheckContext {
        known_accounts: HashSet::new(),
        known_parties: HashSet::new(),
        known_tokens: HashSet::from([TokenRef {
            currency_symbol: "ff".to_owned(),
            token_name: "coin".to_owned(),
        }]),
        known_choices: HashSet::new(),
        require_known_definitions: true,
    };

    let result = type_check(&contract, &context);
    let diagnostics: Vec<(String, String)> = result
        .errors
        .iter()
        .map(|d| (d.path.clone(), d.message.clone()))
        .collect();

    assert_eq!(
        diagnostics,
        vec![
            (
                "$.from".to_owned(),
                "unknown account owner role 'alice'".to_owned(),
            ),
            (
                "$.to.party".to_owned(),
                "unknown party role 'bob'".to_owned(),
            ),
            ("$.token".to_owned(), "unknown token '.'".to_owned(),),
        ]
    );
}

#[test]
fn golden_warning_set_for_shadowed_let() {
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

    let contract = parse_contract_yaml(yaml).expect("parses");
    let result = type_check(&contract, &TypeCheckContext::default());

    let warnings: Vec<(String, String)> = result
        .warnings
        .iter()
        .map(|d| (d.path.clone(), d.message.clone()))
        .collect();

    assert_eq!(
        warnings,
        vec![
            (
                "$.then".to_owned(),
                "let binding 'x' shadows an existing binding".to_owned(),
            ),
            (
                "$.then.name".to_owned(),
                "let binding 'x' is never used".to_owned(),
            ),
            (
                "$.name".to_owned(),
                "let binding 'x' is never used".to_owned(),
            ),
        ]
    );
}

#[test]
fn golden_choice_usage_context_path() {
    let yaml = r#"
Assert:
  cond:
    ChoseSomething:
      ChoiceId:
        name: "oracle"
        party: { Role: "arbiter" }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");
    let context = TypeCheckContext {
        known_accounts: HashSet::new(),
        known_parties: HashSet::from([PartyRef::Role("arbiter".to_owned())]),
        known_tokens: HashSet::new(),
        known_choices: HashSet::new(),
        require_known_definitions: true,
    };

    let result = type_check(&contract, &context);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].path, "$.cond.choice".to_owned());
    assert_eq!(
        result.errors[0].message,
        "choice 'oracle' for party is not declared in contract or context".to_owned(),
    );
}
