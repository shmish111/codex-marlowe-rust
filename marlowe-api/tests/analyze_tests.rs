use marlowe_api::{
    analyze::{
        analyze_authorization_safety, analyze_deadline_safety, apply_auto_repair_patch,
        AuthorizationAction, AuthorizationRule, CounterexampleRequest, CounterexampleResult,
    },
    ast::Party,
    parse_contract_yaml,
};

#[test]
fn deadline_safety_detects_non_close_timeout_branch() {
    let contract = parse_contract_yaml(
        r#"
When:
  cases: []
  timeout: { Timeout: 7 }
  timeout_continuation:
    Assert:
      cond: { "True": {} }
      then: { Close: {} }
"#,
    )
    .expect("valid contract");

    let result = analyze_deadline_safety(&contract, &CounterexampleRequest::default());
    match result {
        CounterexampleResult::CounterexampleFound { counterexample, .. } => {
            assert_eq!(counterexample.timeout.map(|v| v.to_string()), Some("7".to_owned()));
            assert_eq!(
                counterexample.witness_time.map(|v| v.to_string()),
                Some("7".to_owned())
            );
            assert_eq!(counterexample.steps.len(), 2);
            assert_eq!(counterexample.steps[0].id, "deadline_safety.step.0");
            assert_eq!(counterexample.steps[0].kind, "timeout_reached");
            assert_eq!(counterexample.steps[0].severity, "info");
            assert!(!counterexample.steps[0].suggested_fix.is_empty());
            assert_eq!(counterexample.steps[1].severity, "high");
            assert!(counterexample.steps[1].suggested_fix.contains("timeout_continuation"));
            let patch = counterexample.auto_repair_patch.expect("repair patch");
            assert_eq!(patch.kind, "set_contract");
            assert!(patch.path.ends_with(".timeout_continuation"));
            assert_eq!(patch.value, "{ Close: {} }");
        }
        other => panic!("expected counterexample, got {other:?}"),
    }
}

#[test]
fn deadline_safety_passes_when_all_timeouts_close() {
    let contract = parse_contract_yaml(
        r#"
When:
  cases:
    - Case:
        action: { Notify: { if: { "True": {} } } }
        then: { Close: {} }
  timeout: { Timeout: 7 }
  timeout_continuation: { Close: {} }
"#,
    )
    .expect("valid contract");

    let result = analyze_deadline_safety(&contract, &CounterexampleRequest::default());
    assert!(matches!(result, CounterexampleResult::PassBounded { .. }));
}

#[test]
fn authorization_safety_detects_unauthorized_choice_party() {
    let contract = parse_contract_yaml(
        r#"
When:
  cases:
    - Case:
        action:
          Choice:
            id: { ChoiceId: { name: "release", party: { Role: "eve" } } }
            bounds: [ { Bound: { from: { Constant: 0 }, to: { Constant: 1 } } } ]
        then: { Close: {} }
  timeout: { Timeout: 7 }
  timeout_continuation: { Close: {} }
"#,
    )
    .expect("valid contract");

    let result = analyze_authorization_safety(
        &contract,
        &CounterexampleRequest::default(),
        &AuthorizationRule {
            action: AuthorizationAction::Choice,
            target: Some("release".to_owned()),
            allowed_parties: vec![Party::Role("alice".to_owned())],
        },
    );

    match result {
        CounterexampleResult::CounterexampleFound { counterexample, .. } => {
            assert_eq!(counterexample.property, "authorization_safety");
            assert_eq!(counterexample.action, Some("choice"));
            assert_eq!(
                counterexample.offending_party,
                Some(Party::Role("eve".to_owned()))
            );
            assert_eq!(counterexample.target, Some("release".to_owned()));
            assert_eq!(counterexample.steps.len(), 1);
            assert_eq!(counterexample.steps[0].id, "authorization_safety.step.0");
            assert_eq!(counterexample.steps[0].kind, "unauthorized_action");
            assert_eq!(counterexample.steps[0].severity, "high");
            assert!(counterexample.steps[0].suggested_fix.contains("allowed"));
            let patch = counterexample.auto_repair_patch.expect("repair patch");
            assert_eq!(patch.kind, "replace_party");
            assert!(patch.path.ends_with(".Choice.id.party"));
        }
        other => panic!("expected authorization counterexample, got {other:?}"),
    }
}

#[test]
fn apply_auto_repair_patch_updates_timeout_continuation() {
    let contract = parse_contract_yaml(
        r#"
When:
  cases: []
  timeout: { Timeout: 7 }
  timeout_continuation:
    Assert:
      cond: { "True": {} }
      then: { Close: {} }
"#,
    )
    .expect("valid contract");
    let analyzed = analyze_deadline_safety(&contract, &CounterexampleRequest::default());
    let patch = match analyzed {
        CounterexampleResult::CounterexampleFound { counterexample, .. } => {
            counterexample.auto_repair_patch.expect("patch")
        }
        other => panic!("expected counterexample, got {other:?}"),
    };
    let repaired = apply_auto_repair_patch(&contract, &patch).expect("patch applies");
    let rechecked = analyze_deadline_safety(&repaired, &CounterexampleRequest::default());
    assert!(matches!(rechecked, CounterexampleResult::PassBounded { .. }));
}
