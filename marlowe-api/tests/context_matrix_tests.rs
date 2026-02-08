use std::collections::HashSet;

use marlowe_api::{
    parse_contract_yaml, type_check, ChoiceRef, PartyRef, TokenRef, TypeCheckContext,
};

fn strict_context() -> TypeCheckContext {
    TypeCheckContext {
        require_known_definitions: true,
        ..TypeCheckContext::default()
    }
}

fn token_ref() -> TokenRef {
    TokenRef {
        currency_symbol: "".to_owned(),
        token_name: "".to_owned(),
    }
}

#[test]
fn pay_from_requires_known_account_owner() {
    let yaml = r#"
Pay:
  from: { Role: "owner" }
  to_party: { Role: "recipient" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");

    let mut context = strict_context();
    context
        .known_parties
        .insert(PartyRef::Role("recipient".to_owned()));
    context.known_tokens.insert(token_ref());

    let fail = type_check(&contract, &context);
    assert!(
        fail.errors
            .iter()
            .any(|e| e.message == "unknown account owner role 'owner'"),
        "errors: {:?}",
        fail.errors
    );

    context
        .known_accounts
        .insert(PartyRef::Role("owner".to_owned()));
    let pass = type_check(&contract, &context);
    assert!(pass.errors.is_empty());
}

#[test]
fn pay_to_party_requires_known_party() {
    let yaml = r#"
Pay:
  from: { Role: "owner" }
  to_party: { Role: "recipient" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");

    let mut context = strict_context();
    context
        .known_accounts
        .insert(PartyRef::Role("owner".to_owned()));
    context.known_tokens.insert(token_ref());

    let fail = type_check(&contract, &context);
    assert!(
        fail.errors
            .iter()
            .any(|e| e.message == "unknown party role 'recipient'"),
        "errors: {:?}",
        fail.errors
    );

    context
        .known_parties
        .insert(PartyRef::Role("recipient".to_owned()));
    let pass = type_check(&contract, &context);
    assert!(pass.errors.is_empty());
}

#[test]
fn pay_to_account_requires_known_account_owner() {
    let yaml = r#"
Pay:
  from: { Role: "owner" }
  to_account: { Role: "vault" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 1 }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");

    let mut context = strict_context();
    context
        .known_accounts
        .insert(PartyRef::Role("owner".to_owned()));
    context.known_tokens.insert(token_ref());

    let fail = type_check(&contract, &context);
    assert!(
        fail.errors
            .iter()
            .any(|e| e.message == "unknown account owner role 'vault'"),
        "errors: {:?}",
        fail.errors
    );

    context
        .known_accounts
        .insert(PartyRef::Role("vault".to_owned()));
    let pass = type_check(&contract, &context);
    assert!(pass.errors.is_empty());
}

#[test]
fn deposit_field_requirements_are_enforced() {
    let yaml = r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "account_owner" }
            by: { Role: "actor" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 1 }
        then: { Close: {} }
  timeout: { Timeout: 1 }
  timeout_continuation: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");
    let mut context = strict_context();

    let fail = type_check(&contract, &context);
    assert_eq!(fail.errors.len(), 3, "errors: {:?}", fail.errors);

    context
        .known_accounts
        .insert(PartyRef::Role("account_owner".to_owned()));
    context
        .known_parties
        .insert(PartyRef::Role("actor".to_owned()));
    context.known_tokens.insert(token_ref());
    let pass = type_check(&contract, &context);
    assert!(pass.errors.is_empty());
}

#[test]
fn available_money_requires_known_account_and_token() {
    let yaml = r#"
Assert:
  cond:
    ValueGT:
      - AvailableMoney:
          account: { Role: "acct" }
          token: { Token: { currency_symbol: "", token_name: "" } }
      - { Constant: 0 }
  then: { Close: {} }
"#;

    let contract = parse_contract_yaml(yaml).expect("parses");

    let mut context = strict_context();
    let fail = type_check(&contract, &context);
    assert_eq!(fail.errors.len(), 2, "errors: {:?}", fail.errors);

    context
        .known_accounts
        .insert(PartyRef::Role("acct".to_owned()));
    context.known_tokens.insert(token_ref());
    let pass = type_check(&contract, &context);
    assert!(pass.errors.is_empty());
}

#[test]
fn choice_id_requires_known_party_and_choice() {
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

    let mut context = strict_context();
    let fail = type_check(&contract, &context);
    assert_eq!(fail.errors.len(), 2, "errors: {:?}", fail.errors);

    context
        .known_parties
        .insert(PartyRef::Role("arbiter".to_owned()));
    let fail_choice = type_check(&contract, &context);
    assert_eq!(
        fail_choice.errors.len(),
        1,
        "errors: {:?}",
        fail_choice.errors
    );

    context.known_choices = HashSet::from([ChoiceRef {
        name: "oracle".to_owned(),
        party: PartyRef::Role("arbiter".to_owned()),
    }]);
    let pass = type_check(&contract, &context);
    assert!(pass.errors.is_empty());
}
