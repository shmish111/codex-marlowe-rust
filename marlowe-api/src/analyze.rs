use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::to_string as to_json_string;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::ast::{Action, Case, ChoiceId, Contract, Party, Timeout};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterexampleRequest {
    pub max_nodes: usize,
}

impl Default for CounterexampleRequest {
    fn default() -> Self {
        Self { max_nodes: 512 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterexampleResult {
    PassBounded {
        checked_nodes: usize,
    },
    CounterexampleFound {
        checked_nodes: usize,
        counterexample: Counterexample,
    },
    Unsupported {
        checked_nodes: usize,
        path: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counterexample {
    pub property: &'static str,
    pub violating_path: String,
    pub explanation: String,
    pub steps: Vec<CounterexampleStep>,
    pub auto_repair_patch: Option<AutoRepairPatch>,
    pub timeout: Option<BigInt>,
    pub witness_time: Option<BigInt>,
    pub offending_party: Option<Party>,
    pub action: Option<&'static str>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterexampleStep {
    pub id: String,
    pub index: usize,
    pub kind: &'static str,
    pub severity: &'static str,
    pub path: String,
    pub actor: Option<Party>,
    pub time: Option<BigInt>,
    pub detail: String,
    pub suggested_fix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoRepairPatch {
    pub kind: &'static str,
    pub path: String,
    pub value: String,
    pub rationale: String,
}

pub fn apply_auto_repair_patch(
    contract: &Contract,
    patch: &AutoRepairPatch,
) -> Result<Contract, String> {
    let mut updated = contract.clone();
    let mut changed = 0usize;
    apply_patch_recursive(&mut updated, "$", patch, &mut changed)?;
    if changed == 0 {
        return Err(format!("patch path was not found: {}", patch.path));
    }
    if changed > 1 {
        return Err(format!(
            "patch path matched multiple locations ({}): {}",
            changed, patch.path
        ));
    }
    Ok(updated)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRule {
    pub action: AuthorizationAction,
    pub target: Option<String>,
    pub allowed_parties: Vec<Party>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationAction {
    Deposit,
    Choice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViolationCandidate {
    path: String,
    timeout: BigInt,
}

pub fn analyze_deadline_safety(
    contract: &Contract,
    request: &CounterexampleRequest,
) -> CounterexampleResult {
    let mut checked_nodes = 0usize;
    let mut candidate = None;
    let mut unsupported = None;
    collect_violation_candidate(
        contract,
        "$",
        request.max_nodes,
        &mut checked_nodes,
        &mut candidate,
        &mut unsupported,
    );

    if let Some((path, reason)) = unsupported {
        return CounterexampleResult::Unsupported {
            checked_nodes,
            path,
            reason,
        };
    }

    if let Some(candidate) = candidate {
        match solve_timeout_witness(&candidate.timeout) {
            Ok(witness_time) => {
                let violating_path = candidate.path.clone();
                CounterexampleResult::CounterexampleFound {
                    checked_nodes,
                    counterexample: Counterexample {
                        property: "deadline_safety",
                        violating_path: violating_path.clone(),
                        explanation: "A timeout can be reached where continuation is not Close."
                            .to_owned(),
                        steps: vec![
                            CounterexampleStep {
                                id: "deadline_safety.step.0".to_owned(),
                                index: 0,
                                kind: "timeout_reached",
                                severity: "info",
                                path: "$.transaction.interval_end".to_owned(),
                                actor: None,
                                time: Some(witness_time.clone()),
                                detail:
                                    "Interval end reaches the timeout and activates timeout continuation."
                                        .to_owned(),
                                suggested_fix:
                                    "No change needed for this step; review the timeout continuation branch."
                                        .to_owned(),
                            },
                            CounterexampleStep {
                                id: "deadline_safety.step.1".to_owned(),
                                index: 1,
                                kind: "non_close_timeout_continuation",
                                severity: "high",
                                path: violating_path.clone(),
                                actor: None,
                                time: Some(witness_time.clone()),
                                detail: "Timeout continuation is not Close.".to_owned(),
                                suggested_fix:
                                    "Set this When.timeout_continuation to Close, or add an explicit guard proving safe post-timeout behavior."
                                        .to_owned(),
                            },
                        ],
                        timeout: Some(candidate.timeout),
                        witness_time: Some(witness_time),
                        auto_repair_patch: Some(AutoRepairPatch {
                            kind: "set_contract",
                            path: format!("{violating_path}.timeout_continuation"),
                            value: "{ Close: {} }".to_owned(),
                            rationale: "Setting timeout continuation to Close enforces deadline safety for this branch."
                                .to_owned(),
                        }),
                        offending_party: None,
                        action: None,
                        target: None,
                    },
                }
            }
            Err(reason) => CounterexampleResult::Unsupported {
                checked_nodes,
                path: candidate.path,
                reason,
            },
        }
    } else {
        CounterexampleResult::PassBounded { checked_nodes }
    }
}

pub fn analyze_authorization_safety(
    contract: &Contract,
    request: &CounterexampleRequest,
    rule: &AuthorizationRule,
) -> CounterexampleResult {
    let mut checked_nodes = 0usize;
    let mut counterexample = None;
    let mut unsupported = None;
    collect_authorization_violation(
        contract,
        "$",
        request.max_nodes,
        &mut checked_nodes,
        rule,
        &mut counterexample,
        &mut unsupported,
    );

    if let Some((path, reason)) = unsupported {
        return CounterexampleResult::Unsupported {
            checked_nodes,
            path,
            reason,
        };
    }
    if let Some(counterexample) = counterexample {
        CounterexampleResult::CounterexampleFound {
            checked_nodes,
            counterexample,
        }
    } else {
        CounterexampleResult::PassBounded { checked_nodes }
    }
}

fn collect_violation_candidate(
    contract: &Contract,
    path: &str,
    max_nodes: usize,
    checked_nodes: &mut usize,
    candidate: &mut Option<ViolationCandidate>,
    unsupported: &mut Option<(String, String)>,
) {
    if candidate.is_some() || unsupported.is_some() || *checked_nodes >= max_nodes {
        return;
    }
    *checked_nodes += 1;

    match contract {
        Contract::Close => {}
        Contract::Pay { then, .. } => {
            collect_violation_candidate(
                then,
                &format!("{path}.Pay.then"),
                max_nodes,
                checked_nodes,
                candidate,
                unsupported,
            );
        }
        Contract::If { then, else_, .. } => {
            collect_violation_candidate(
                then,
                &format!("{path}.If.then"),
                max_nodes,
                checked_nodes,
                candidate,
                unsupported,
            );
            collect_violation_candidate(
                else_,
                &format!("{path}.If.else"),
                max_nodes,
                checked_nodes,
                candidate,
                unsupported,
            );
        }
        Contract::When {
            cases,
            timeout,
            timeout_continuation,
        } => {
            let when_path = format!("{path}.When");
            if !matches!(timeout_continuation.as_ref(), Contract::Close) {
                match timeout {
                    Timeout::PosixTime(value) => {
                        *candidate = Some(ViolationCandidate {
                            path: when_path.clone(),
                            timeout: value.clone(),
                        });
                    }
                    Timeout::Param(name) => {
                        *unsupported = Some((
                            format!("{when_path}.timeout"),
                            format!(
                                "deadline_safety requires concrete timeout; found parameter '${name}'"
                            ),
                        ));
                        return;
                    }
                    Timeout::Hole(name) => {
                        *unsupported = Some((
                            format!("{when_path}.timeout"),
                            format!("deadline_safety requires concrete timeout; found hole '?{name}'"),
                        ));
                        return;
                    }
                }
            }

            for (idx, case_) in cases.iter().enumerate() {
                if candidate.is_some() || unsupported.is_some() {
                    break;
                }
                if let Case::Case { then, .. } = case_ {
                    collect_violation_candidate(
                        then,
                        &format!("{when_path}.cases[{idx}].Case.then"),
                        max_nodes,
                        checked_nodes,
                        candidate,
                        unsupported,
                    );
                }
            }

            if candidate.is_none() && unsupported.is_none() {
                collect_violation_candidate(
                    timeout_continuation,
                    &format!("{when_path}.timeout_continuation"),
                    max_nodes,
                    checked_nodes,
                    candidate,
                    unsupported,
                );
            }
        }
        Contract::Let { then, .. } => {
            collect_violation_candidate(
                then,
                &format!("{path}.Let.then"),
                max_nodes,
                checked_nodes,
                candidate,
                unsupported,
            );
        }
        Contract::Assert { then, .. } => {
            collect_violation_candidate(
                then,
                &format!("{path}.Assert.then"),
                max_nodes,
                checked_nodes,
                candidate,
                unsupported,
            );
        }
        Contract::Hole(name) => {
            *unsupported = Some((
                path.to_owned(),
                format!("deadline_safety cannot analyze unresolved hole '?{name}'"),
            ));
        }
    }
}

fn collect_authorization_violation(
    contract: &Contract,
    path: &str,
    max_nodes: usize,
    checked_nodes: &mut usize,
    rule: &AuthorizationRule,
    counterexample: &mut Option<Counterexample>,
    unsupported: &mut Option<(String, String)>,
) {
    if counterexample.is_some() || unsupported.is_some() || *checked_nodes >= max_nodes {
        return;
    }
    *checked_nodes += 1;

    match contract {
        Contract::Close => {}
        Contract::Pay { then, .. } => {
            collect_authorization_violation(
                then,
                &format!("{path}.Pay.then"),
                max_nodes,
                checked_nodes,
                rule,
                counterexample,
                unsupported,
            );
        }
        Contract::If { then, else_, .. } => {
            collect_authorization_violation(
                then,
                &format!("{path}.If.then"),
                max_nodes,
                checked_nodes,
                rule,
                counterexample,
                unsupported,
            );
            collect_authorization_violation(
                else_,
                &format!("{path}.If.else"),
                max_nodes,
                checked_nodes,
                rule,
                counterexample,
                unsupported,
            );
        }
        Contract::When {
            cases,
            timeout_continuation,
            ..
        } => {
            let when_path = format!("{path}.When");
            for (idx, case_) in cases.iter().enumerate() {
                if counterexample.is_some() || unsupported.is_some() {
                    break;
                }
                if let Case::Case { action, then } = case_ {
                    let action_path = format!("{when_path}.cases[{idx}].Case.action");
                    if let Some(violation) = authorization_violation_for_action(action, &action_path, rule)
                    {
                        *counterexample = Some(violation);
                        break;
                    }
                    collect_authorization_violation(
                        then,
                        &format!("{when_path}.cases[{idx}].Case.then"),
                        max_nodes,
                        checked_nodes,
                        rule,
                        counterexample,
                        unsupported,
                    );
                }
            }

            if counterexample.is_none() && unsupported.is_none() {
                collect_authorization_violation(
                    timeout_continuation,
                    &format!("{when_path}.timeout_continuation"),
                    max_nodes,
                    checked_nodes,
                    rule,
                    counterexample,
                    unsupported,
                );
            }
        }
        Contract::Let { then, .. } => {
            collect_authorization_violation(
                then,
                &format!("{path}.Let.then"),
                max_nodes,
                checked_nodes,
                rule,
                counterexample,
                unsupported,
            );
        }
        Contract::Assert { then, .. } => {
            collect_authorization_violation(
                then,
                &format!("{path}.Assert.then"),
                max_nodes,
                checked_nodes,
                rule,
                counterexample,
                unsupported,
            );
        }
        Contract::Hole(name) => {
            *unsupported = Some((
                path.to_owned(),
                format!("authorization_safety cannot analyze unresolved hole '?{name}'"),
            ));
        }
    }
}

fn authorization_violation_for_action(
    action: &Action,
    action_path: &str,
    rule: &AuthorizationRule,
) -> Option<Counterexample> {
    match (&rule.action, action) {
        (
            AuthorizationAction::Deposit,
            Action::Deposit { by, .. },
        ) => {
            if !rule.allowed_parties.iter().any(|party| party == by) {
                Some(Counterexample {
                    property: "authorization_safety",
                    violating_path: action_path.to_owned(),
                    explanation: "A deposit action can be triggered by a party outside the allowed set."
                        .to_owned(),
                    steps: vec![CounterexampleStep {
                        id: "authorization_safety.step.0".to_owned(),
                        index: 0,
                        kind: "unauthorized_action",
                        severity: "high",
                        path: action_path.to_owned(),
                        actor: Some(by.clone()),
                        time: None,
                        detail: "Deposit action authorizing party is outside allowed_parties."
                            .to_owned(),
                        suggested_fix:
                            "Restrict the Deposit.by party to one of authorization_rule.allowed_parties."
                                .to_owned(),
                    }],
                    timeout: None,
                    witness_time: None,
                    auto_repair_patch: rule.allowed_parties.first().map(|party| AutoRepairPatch {
                        kind: "replace_party",
                        path: format!("{action_path}.Deposit.by"),
                        value: to_json_string(party)
                            .unwrap_or_else(|_| "\"<serialization_error>\"".to_owned()),
                        rationale: "Restricting Deposit.by to an allowed party resolves this authorization violation."
                            .to_owned(),
                    }),
                    offending_party: Some(by.clone()),
                    action: Some("deposit"),
                    target: None,
                })
            } else {
                None
            }
        }
        (
            AuthorizationAction::Choice,
            Action::Choice {
                id: ChoiceId::ChoiceId { name, party },
                ..
            },
        ) => {
            if let Some(target) = &rule.target {
                if target != name {
                    return None;
                }
            }
            if !rule.allowed_parties.iter().any(|allowed| allowed == party) {
                Some(Counterexample {
                    property: "authorization_safety",
                    violating_path: action_path.to_owned(),
                    explanation:
                        "A choice action can be triggered by a party outside the allowed set."
                            .to_owned(),
                    steps: vec![CounterexampleStep {
                        id: "authorization_safety.step.0".to_owned(),
                        index: 0,
                        kind: "unauthorized_action",
                        severity: "high",
                        path: action_path.to_owned(),
                        actor: Some(party.clone()),
                        time: None,
                        detail: "Choice action authorizing party is outside allowed_parties."
                            .to_owned(),
                        suggested_fix:
                            "Change Choice.id.party to an allowed party, or expand authorization_rule.allowed_parties if intentional."
                                .to_owned(),
                    }],
                    timeout: None,
                    witness_time: None,
                    auto_repair_patch: rule.allowed_parties.first().map(|allowed| AutoRepairPatch {
                        kind: "replace_party",
                        path: format!("{action_path}.Choice.id.party"),
                        value: to_json_string(allowed)
                            .unwrap_or_else(|_| "\"<serialization_error>\"".to_owned()),
                        rationale: "Restricting Choice.id.party to an allowed party resolves this authorization violation."
                            .to_owned(),
                    }),
                    offending_party: Some(party.clone()),
                    action: Some("choice"),
                    target: Some(name.clone()),
                })
            } else {
                None
            }
        }
        (
            AuthorizationAction::Choice,
            Action::Choice {
                id: ChoiceId::Hole(name),
                ..
            },
        ) => Some(Counterexample {
            property: "authorization_safety",
            violating_path: action_path.to_owned(),
            explanation: format!(
                "Choice action has unresolved choice hole '?{name}', so authorization cannot be guaranteed."
            ),
            steps: vec![CounterexampleStep {
                id: "authorization_safety.step.0".to_owned(),
                index: 0,
                kind: "unknown_authorizer",
                severity: "medium",
                path: action_path.to_owned(),
                actor: None,
                time: None,
                detail: "Choice id is unresolved; cannot establish authorized party.".to_owned(),
                suggested_fix:
                    "Replace unresolved ChoiceId hole with a concrete party and re-run authorization analysis."
                        .to_owned(),
            }],
            timeout: None,
            witness_time: None,
            auto_repair_patch: None,
            offending_party: None,
            action: Some("choice"),
            target: rule.target.clone(),
        }),
        _ => None,
    }
}

fn solve_timeout_witness(timeout: &BigInt) -> Result<BigInt, String> {
    let timeout_i64 = timeout
        .to_i64()
        .ok_or_else(|| "timeout exceeds supported i64 range in analyzer MVP".to_owned())?;
    let minimized = timeout_i64.max(0);
    let smt = format!(
        "(set-logic QF_LIA)\n\
         (declare-const t Int)\n\
         (assert (>= t {timeout_i64}))\n\
         (assert (>= t 0))\n\
         (assert (= t {minimized}))\n\
         (check-sat)\n"
    );

    let mut child = Command::new("z3")
        .arg("-in")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to start z3 process: {err}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(smt.as_bytes())
            .map_err(|err| format!("failed to write SMT query to z3: {err}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("failed to collect z3 output: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("z3 execution failed: {stderr}"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.lines().next().is_some_and(|line| line.trim() == "sat") {
        Ok(BigInt::from(minimized))
    } else if stdout.lines().next().is_some_and(|line| line.trim() == "unsat") {
        Err("internal analyzer error: expected witness is UNSAT".to_owned())
    } else {
        Err("solver returned unknown for deadline_safety witness".to_owned())
    }
}

fn apply_patch_recursive(
    contract: &mut Contract,
    path: &str,
    patch: &AutoRepairPatch,
    changed: &mut usize,
) -> Result<(), String> {
    match contract {
        Contract::Close | Contract::Hole(_) => {}
        Contract::Pay { then, .. } => {
            apply_patch_recursive(then, &format!("{path}.Pay.then"), patch, changed)?;
        }
        Contract::If { then, else_, .. } => {
            apply_patch_recursive(then, &format!("{path}.If.then"), patch, changed)?;
            apply_patch_recursive(else_, &format!("{path}.If.else"), patch, changed)?;
        }
        Contract::When {
            cases,
            timeout_continuation,
            ..
        } => {
            let when_path = format!("{path}.When");
            let tc_path = format!("{when_path}.timeout_continuation");
            if patch.kind == "set_contract" && tc_path == patch.path {
                if patch.value.trim() != "{ Close: {} }" {
                    return Err(format!(
                        "unsupported set_contract value for timeout continuation: {}",
                        patch.value
                    ));
                }
                *timeout_continuation = Box::new(Contract::Close);
                *changed += 1;
            } else {
                apply_patch_recursive(timeout_continuation, &tc_path, patch, changed)?;
            }

            for (idx, case_) in cases.iter_mut().enumerate() {
                if let Case::Case { action, then } = case_ {
                    let action_path = format!("{when_path}.cases[{idx}].Case.action");
                    apply_patch_to_action(action, &action_path, patch, changed)?;
                    apply_patch_recursive(
                        then,
                        &format!("{when_path}.cases[{idx}].Case.then"),
                        patch,
                        changed,
                    )?;
                }
            }
        }
        Contract::Let { then, .. } => {
            apply_patch_recursive(then, &format!("{path}.Let.then"), patch, changed)?;
        }
        Contract::Assert { then, .. } => {
            apply_patch_recursive(then, &format!("{path}.Assert.then"), patch, changed)?;
        }
    }
    Ok(())
}

fn apply_patch_to_action(
    action: &mut Action,
    action_path: &str,
    patch: &AutoRepairPatch,
    changed: &mut usize,
) -> Result<(), String> {
    if patch.kind != "replace_party" {
        return Ok(());
    }
    let replacement: Party =
        serde_json::from_str(&patch.value).map_err(|err| format!("invalid replacement party JSON: {err}"))?;
    match action {
        Action::Deposit { by, .. } => {
            let by_path = format!("{action_path}.Deposit.by");
            if by_path == patch.path {
                *by = replacement;
                *changed += 1;
            }
        }
        Action::Choice {
            id: ChoiceId::ChoiceId { party, .. },
            ..
        } => {
            let party_path = format!("{action_path}.Choice.id.party");
            if party_path == patch.path {
                *party = replacement;
                *changed += 1;
            }
        }
        _ => {}
    }
    Ok(())
}
