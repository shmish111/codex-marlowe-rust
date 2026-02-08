use std::collections::HashSet;

use marlowe_api::{parse_contract_yaml, type_check, PartyRef, TokenRef, TypeCheckContext};
use proptest::prelude::*;

const SIMPLE_PAY_YAML: &str = r#"
Pay:
  from: { Role: "alice" }
  to_party: { Role: "bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 10 }
  then: { Close: {} }
"#;

fn ident_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[A-Za-z_][A-Za-z0-9_]{0,15}").expect("valid regex")
}

fn count_unknown_errors(result: &marlowe_api::TypeCheckResult) -> usize {
    result
        .errors
        .iter()
        .filter(|e| e.message.contains("unknown"))
        .count()
}

fn context_from_mask(mask: u8) -> TypeCheckContext {
    let mut known_accounts = HashSet::new();
    let mut known_parties = HashSet::new();
    let mut known_tokens = HashSet::new();

    if (mask & 0b001) != 0 {
        known_accounts.insert(PartyRef::Role("alice".to_owned()));
    }
    if (mask & 0b010) != 0 {
        known_parties.insert(PartyRef::Role("bob".to_owned()));
    }
    if (mask & 0b100) != 0 {
        known_tokens.insert(TokenRef {
            currency_symbol: "".to_owned(),
            token_name: "".to_owned(),
        });
    }

    TypeCheckContext {
        known_accounts,
        known_parties,
        known_tokens,
        known_choices: HashSet::new(),
        require_known_definitions: true,
    }
}

fn build_nested_let_contract(names: &[String]) -> String {
    let target = names.last().expect("names non-empty");
    let mut inner = format!(
        "Pay:\n  from: {{ Role: \"alice\" }}\n  to_party: {{ Role: \"bob\" }}\n  token: {{ Token: {{ currency_symbol: \"\", token_name: \"\" }} }}\n  amount: {{ UseValue: \"{target}\" }}\n  then: {{ Close: {{}} }}\n"
    );

    for name in names.iter().rev() {
        inner = format!(
            "Let:\n  name: \"{name}\"\n  value: {{ Constant: 1 }}\n  then:\n{}",
            indent(&inner, 4)
        );
    }

    inner
}

fn indent(text: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

proptest! {
    #[test]
    fn parser_never_panics_on_arbitrary_input(input in any::<String>()) {
        let _ = parse_contract_yaml(&input);
    }

    #[test]
    fn parser_rejects_params_outside_strict_positions(name in ident_strategy()) {
        let yaml = format!(
            "Pay:\n  from: \"${name}\"\n  to_party: {{ Role: \"bob\" }}\n  token: {{ Token: {{ currency_symbol: \"\", token_name: \"\" }} }}\n  amount: {{ Constant: 1 }}\n  then: {{ Close: {{}} }}\n"
        );

        let error = parse_contract_yaml(&yaml).expect_err("party params should be rejected");
        prop_assert!(error.message.contains("Party position"));
    }

    #[test]
    fn shared_param_in_value_and_timeout_causes_type_conflict(name in ident_strategy()) {
        let yaml = format!(
            "When:\n  cases: []\n  timeout: \"${name}\"\n  timeout_continuation:\n    Let:\n      name: \"x\"\n      value: \"${name}\"\n      then: {{ Close: {{}} }}\n"
        );

        let contract = parse_contract_yaml(&yaml).expect("valid yaml");
        let result = type_check(&contract, &TypeCheckContext::default());

        prop_assert!(result.errors.iter().any(|e| e.message.contains("conflicting inferred types")));
    }

    #[test]
    fn unknown_definition_errors_are_monotonic(mask_a in 0u8..8, extra in 0u8..8) {
        let contract = parse_contract_yaml(SIMPLE_PAY_YAML).expect("simple contract parses");

        let mask_b = mask_a | extra;
        let result_a = type_check(&contract, &context_from_mask(mask_a));
        let result_b = type_check(&contract, &context_from_mask(mask_b));

        let unknown_a = count_unknown_errors(&result_a);
        let unknown_b = count_unknown_errors(&result_b);
        prop_assert!(unknown_b <= unknown_a);
    }

    #[test]
    fn well_scoped_use_value_never_reports_undefined_let(
        names in prop::collection::vec(ident_strategy(), 1..6)
    ) {
        let yaml = build_nested_let_contract(&names);
        let contract = parse_contract_yaml(&yaml).expect("generated contract parses");
        let result = type_check(&contract, &TypeCheckContext::default());

        prop_assert!(!result.errors.iter().any(|e| e.message.contains("undefined let binding")));
    }
}
