use std::time::{Duration, Instant};

use marlowe_api::{parse_contract_yaml, type_check, TypeCheckContext};

fn indent(text: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn deep_let_contract(depth: usize) -> String {
    let mut current = "{ Close: {} }".to_owned();
    for i in (0..depth).rev() {
        current = format!(
            "Let:\n  name: \"v{i}\"\n  value: {{ Constant: {i} }}\n  then:\n{}",
            indent(&current, 4)
        );
    }
    current
}

fn wide_when_contract(width: usize) -> String {
    let mut cases = String::new();
    for i in 0..width {
        let case = format!(
            "    - Case:\n        action:\n          Deposit:\n            into: {{ Role: \"Alice\" }}\n            by: {{ Role: \"Alice\" }}\n            token: {{ Token: {{ currency_symbol: \"\", token_name: \"\" }} }}\n            amount: {{ Constant: {i} }}\n        then: {{ Close: {{}} }}\n"
        );
        cases.push_str(&case);
    }

    format!(
        "When:\n  cases:\n{cases}  timeout: {{ Timeout: 999999999999 }}\n  timeout_continuation: {{ Close: {{}} }}\n"
    )
}

#[test]
fn deep_contract_parse_and_typecheck_within_budget() {
    let yaml = deep_let_contract(40);

    let start = Instant::now();
    let contract = parse_contract_yaml(&yaml).expect("deep contract parses");
    let parse_time = start.elapsed();

    let start = Instant::now();
    let result = type_check(&contract, &TypeCheckContext::default());
    let typecheck_time = start.elapsed();

    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    assert!(
        parse_time < Duration::from_secs(3),
        "deep parse regression: {:?}",
        parse_time
    );
    assert!(
        typecheck_time < Duration::from_secs(3),
        "deep typecheck regression: {:?}",
        typecheck_time
    );
}

#[test]
fn wide_contract_parse_and_typecheck_within_budget() {
    let yaml = wide_when_contract(300);

    let start = Instant::now();
    let contract = parse_contract_yaml(&yaml).expect("wide contract parses");
    let parse_time = start.elapsed();

    let start = Instant::now();
    let result = type_check(&contract, &TypeCheckContext::default());
    let typecheck_time = start.elapsed();

    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    assert!(
        parse_time < Duration::from_secs(4),
        "wide parse regression: {:?}",
        parse_time
    );
    assert!(
        typecheck_time < Duration::from_secs(4),
        "wide typecheck regression: {:?}",
        typecheck_time
    );
}
