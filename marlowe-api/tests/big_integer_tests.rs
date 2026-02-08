use marlowe_api::{contract_to_yaml_string, parse_contract_yaml, type_check, TypeCheckContext};

#[test]
fn parses_large_positive_and_negative_integer_constants_from_strings() {
    let yaml = r#"
If:
  cond:
    ValueGT:
      - { Constant: "170141183460469231731687303715884105727" }
      - { Constant: "-170141183460469231731687303715884105728" }
  then: { Close: {} }
  else: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("large integers should parse");
    let result = type_check(&contract, &TypeCheckContext::default());
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
}

#[test]
fn parses_large_timeout_integer_from_string() {
    let yaml = r#"
When:
  cases: []
  timeout: { Timeout: "999999999999999999999999999999999999" }
  timeout_continuation: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("large timeout should parse");
    let result = type_check(&contract, &TypeCheckContext::default());
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
}

#[test]
fn rejects_invalid_integer_strings() {
    let yaml = r#"
Pay:
  from: { Role: "alice" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: "12x" }
  then: { Close: {} }
"#;

    let err = parse_contract_yaml(yaml).expect_err("invalid integer should fail");
    assert_eq!(err.path, "$.Pay.amount.Constant".to_owned());
    assert_eq!(err.message, "invalid integer '12x'".to_owned());
}

#[test]
fn large_integers_round_trip_through_serializer() {
    let yaml = r#"
Let:
  name: "v"
  value: { Constant: "1234567890123456789012345678901234567890" }
  then:
    Pay:
      from: { Role: "alice" }
      to_party: { Role: "bob" }
      token: { Token: { currency_symbol: "", token_name: "" } }
      amount: { UseValue: "v" }
      then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");
    let serialized = contract_to_yaml_string(&contract).expect("serializes");
    let reparsed = parse_contract_yaml(&serialized).expect("reparses");

    assert_eq!(contract, reparsed);
}
