use std::{collections::BTreeMap, fs, path::PathBuf};

use marlowe_api::{parse_contract_yaml, type_check, TypeCheckContext};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    expectations: BTreeMap<String, Expectation>,
}

#[derive(Debug, Deserialize)]
struct Expectation {
    ready_to_run: bool,
    errors: usize,
    warnings: usize,
    holes: Vec<String>,
    params: Vec<String>,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example_expectations.yaml")
}

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("examples")
        .join(name)
}

#[test]
fn examples_match_differential_expectations() {
    let fixture_text = fs::read_to_string(fixture_path()).expect("fixture file exists");
    let fixture: Fixture = serde_yaml::from_str(&fixture_text).expect("valid fixture yaml");

    for (example, expected) in fixture.expectations {
        let yaml = fs::read_to_string(example_path(&example)).expect("example readable");
        let contract = parse_contract_yaml(&yaml).expect("example parses");
        let result = type_check(&contract, &TypeCheckContext::default());

        let mut holes: Vec<String> = result
            .holes
            .iter()
            .map(|s| format!("{}:{}", s.ty.as_str(), s.name))
            .collect();
        holes.sort();
        let mut expected_holes = expected.holes.clone();
        expected_holes.sort();

        let mut params: Vec<String> = result
            .params
            .iter()
            .map(|s| format!("{}:{}", s.ty.as_str(), s.name))
            .collect();
        params.sort();
        let mut expected_params = expected.params.clone();
        expected_params.sort();

        assert_eq!(
            result.ready_to_run, expected.ready_to_run,
            "ready_to_run mismatch for {example}"
        );
        assert_eq!(
            result.errors.len(),
            expected.errors,
            "errors mismatch for {example}"
        );
        assert_eq!(
            result.warnings.len(),
            expected.warnings,
            "warnings mismatch for {example}; warnings: {:?}",
            result.warnings
        );
        assert_eq!(holes, expected_holes, "holes mismatch for {example}");
        assert_eq!(params, expected_params, "params mismatch for {example}");
    }
}
