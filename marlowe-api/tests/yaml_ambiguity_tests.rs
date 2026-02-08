use marlowe_api::parse_contract_yaml;

#[test]
fn unquoted_true_constructor_is_rejected() {
    let yaml = r#"
If:
  cond: { True: {} }
  then: { Close: {} }
  else: { Close: {} }
"#;

    let err = parse_contract_yaml(yaml).expect_err("unquoted True should fail");
    assert!(err.message.contains("constructor key must be a string"));
}

#[test]
fn quoted_true_constructor_is_accepted() {
    let yaml = r#"
If:
  cond: { "True": {} }
  then: { Close: {} }
  else: { Close: {} }
"#;

    parse_contract_yaml(yaml).expect("quoted True should parse");
}

#[test]
fn unquoted_true_role_name_is_rejected_as_non_string() {
    let yaml = r#"
Pay:
  from: { Role: true }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    let err = parse_contract_yaml(yaml).expect_err("yaml bool role should fail");
    assert_eq!(err.path, "$.Pay.from.Role".to_owned());
    assert_eq!(err.message, "expected string".to_owned());
}

#[test]
fn quoted_on_role_name_is_accepted() {
    let yaml = r#"
Pay:
  from: { Role: "true" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    parse_contract_yaml(yaml).expect("quoted role should parse");
}

#[test]
fn null_string_fields_are_rejected() {
    let yaml = r#"
Pay:
  from: { Role: "alice" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: null, token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    let err = parse_contract_yaml(yaml).expect_err("null should fail");
    assert_eq!(err.path, "$.Pay.token.Token.currency_symbol".to_owned());
    assert_eq!(err.message, "expected string".to_owned());
}

#[test]
fn quoted_null_literal_is_accepted() {
    let yaml = r#"
Pay:
  from: { Role: "alice" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "null", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    parse_contract_yaml(yaml).expect("quoted null literal should parse");
}
