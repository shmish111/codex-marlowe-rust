use std::collections::BTreeMap;

use num_bigint::BigInt;
use num_traits::{Signed, Zero};

use crate::{
    ast::{
        Action, Bound, Case, ChoiceId, Contract, Observation, Party, PayeeTarget, Timeout, Token,
        Value,
    },
    type_check, TypeCheckContext,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountId {
    pub owner: Party,
    pub token: Token,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimState {
    pub accounts: BTreeMap<AccountId, BigInt>,
    pub choices: BTreeMap<ChoiceId, BigInt>,
    pub bound_values: BTreeMap<String, BigInt>,
    pub min_time: BigInt,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            accounts: BTreeMap::new(),
            choices: BTreeMap::new(),
            bound_values: BTreeMap::new(),
            min_time: BigInt::zero(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimTransaction {
    pub interval_start: BigInt,
    pub interval_end: BigInt,
    pub inputs: Vec<SimInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimInput {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: BigInt,
    },
    Choice {
        id: ChoiceId,
        value: BigInt,
    },
    Notify,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    pub from: Party,
    pub to: PayeeTarget,
    pub token: Token,
    pub amount: BigInt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionWarning {
    NonPositiveDeposit {
        into: Party,
        by: Party,
        token: Token,
        amount: BigInt,
    },
    NonPositivePay {
        from: Party,
        to: PayeeTarget,
        token: Token,
        amount: BigInt,
    },
    PartialPay {
        from: Party,
        to: PayeeTarget,
        token: Token,
        expected: BigInt,
        paid: BigInt,
    },
    Shadowing {
        name: String,
        old: BigInt,
        new: BigInt,
    },
    AssertionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceReduceRule {
    CloseRefund,
    Pay,
    IfBranch,
    WhenTimeout,
    Let,
    Assert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceStep {
    Reduced {
        rule: TraceReduceRule,
        warning: Option<TransactionWarning>,
        payment: Option<Payment>,
    },
    InputApplied {
        input_index: usize,
        input: SimInput,
        warning: Option<TransactionWarning>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    NotReadyToRun,
    InvalidInterval {
        start: BigInt,
        end: BigInt,
    },
    IntervalInPast {
        min_time: BigInt,
        start: BigInt,
        end: BigInt,
    },
    AmbiguousTimeInterval,
    NoMatchForInput {
        input_index: usize,
    },
    UselessTransaction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimTransactionSuccess {
    pub warnings: Vec<TransactionWarning>,
    pub payments: Vec<Payment>,
    pub state: SimState,
    pub contract: Contract,
    pub trace: Vec<TraceStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimTransactionResult {
    Success(Box<SimTransactionSuccess>),
    Error(SimError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewResult {
    pub state: SimState,
    pub contract: Contract,
    pub inputs: Vec<PreviewInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewInput {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: BigInt,
    },
    Choice {
        id: ChoiceId,
        bounds: Vec<(BigInt, BigInt)>,
    },
    Notify {
        can_notify: bool,
    },
}

#[derive(Clone)]
struct Environment {
    start: BigInt,
    end: BigInt,
}

pub fn simulate_transaction(
    contract: &Contract,
    state: &SimState,
    transaction: &SimTransaction,
) -> SimTransactionResult {
    simulate_transaction_with_trace(contract, state, transaction, false)
}

pub fn simulate_transaction_with_trace(
    contract: &Contract,
    state: &SimState,
    transaction: &SimTransaction,
    include_trace: bool,
) -> SimTransactionResult {
    let check = type_check(contract, &TypeCheckContext::default());
    if !check.errors.is_empty() || !check.holes.is_empty() || !check.params.is_empty() {
        return SimTransactionResult::Error(SimError::NotReadyToRun);
    }

    let (environment, fixed_state) = match fix_interval(
        &transaction.interval_start,
        &transaction.interval_end,
        state,
    ) {
        Ok(result) => result,
        Err(err) => return SimTransactionResult::Error(err),
    };

    let applied = match apply_all_inputs(
        &environment,
        &fixed_state,
        contract,
        &transaction.inputs,
        include_trace,
    ) {
        Ok(result) => result,
        Err(err) => return SimTransactionResult::Error(err),
    };

    if !applied.reduced && (!matches!(contract, Contract::Close) || fixed_state.accounts.is_empty())
    {
        return SimTransactionResult::Error(SimError::UselessTransaction);
    }

    SimTransactionResult::Success(Box::new(SimTransactionSuccess {
        warnings: applied.warnings,
        payments: applied.payments,
        state: applied.state,
        contract: applied.contract,
        trace: applied.trace,
    }))
}

pub fn preview_inputs(
    contract: &Contract,
    state: &SimState,
    interval_start: &BigInt,
    interval_end: &BigInt,
) -> Result<PreviewResult, SimError> {
    let check = type_check(contract, &TypeCheckContext::default());
    if !check.errors.is_empty() || !check.holes.is_empty() || !check.params.is_empty() {
        return Err(SimError::NotReadyToRun);
    }

    let (environment, fixed_state) = fix_interval(interval_start, interval_end, state)?;
    let reduced = reduce_until_quiescent(&environment, &fixed_state, contract, false)?;

    let inputs = match &reduced.contract {
        Contract::When { cases, .. } => cases
            .iter()
            .filter_map(|case| preview_case(case, &reduced.state, &environment))
            .collect(),
        _ => Vec::new(),
    };

    Ok(PreviewResult {
        state: reduced.state,
        contract: reduced.contract,
        inputs,
    })
}

fn fix_interval(
    start: &BigInt,
    end: &BigInt,
    state: &SimState,
) -> Result<(Environment, SimState), SimError> {
    if start > end {
        return Err(SimError::InvalidInterval {
            start: start.clone(),
            end: end.clone(),
        });
    }

    let min_time = state.min_time.clone();
    if end < &min_time {
        return Err(SimError::IntervalInPast {
            min_time,
            start: start.clone(),
            end: end.clone(),
        });
    }

    let bounded_start = if start > &state.min_time {
        start.clone()
    } else {
        state.min_time.clone()
    };

    let environment = Environment {
        start: bounded_start.clone(),
        end: end.clone(),
    };

    let mut new_state = state.clone();
    new_state.min_time = bounded_start;
    Ok((environment, new_state))
}

struct ApplyAllResult {
    reduced: bool,
    warnings: Vec<TransactionWarning>,
    payments: Vec<Payment>,
    state: SimState,
    contract: Contract,
    trace: Vec<TraceStep>,
}

fn apply_all_inputs(
    environment: &Environment,
    state: &SimState,
    contract: &Contract,
    inputs: &[SimInput],
    include_trace: bool,
) -> Result<ApplyAllResult, SimError> {
    let mut current_state = state.clone();
    let mut current_contract = contract.clone();
    let mut warnings = Vec::new();
    let mut payments = Vec::new();
    let mut reduced_or_applied = false;
    let mut trace = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        let reduced = reduce_until_quiescent(
            environment,
            &current_state,
            &current_contract,
            include_trace,
        )?;
        warnings.extend(reduced.warnings);
        payments.extend(reduced.payments);
        trace.extend(reduced.trace);
        current_state = reduced.state;
        current_contract = reduced.contract;

        let applied = apply_input(environment, &current_state, &current_contract, input)
            .ok_or(SimError::NoMatchForInput { input_index: index })?;

        reduced_or_applied = true;
        if let Some(warning) = applied.warning {
            warnings.push(warning.clone());
            if include_trace {
                trace.push(TraceStep::InputApplied {
                    input_index: index,
                    input: input.clone(),
                    warning: Some(warning),
                });
            }
        } else if include_trace {
            trace.push(TraceStep::InputApplied {
                input_index: index,
                input: input.clone(),
                warning: None,
            });
        }
        current_state = applied.state;
        current_contract = applied.contract;
    }

    let reduced = reduce_until_quiescent(
        environment,
        &current_state,
        &current_contract,
        include_trace,
    )?;
    reduced_or_applied |= reduced.reduced;
    warnings.extend(reduced.warnings);
    payments.extend(reduced.payments);
    trace.extend(reduced.trace);

    Ok(ApplyAllResult {
        reduced: reduced_or_applied,
        warnings,
        payments,
        state: reduced.state,
        contract: reduced.contract,
        trace,
    })
}

struct ReduceUntilResult {
    reduced: bool,
    warnings: Vec<TransactionWarning>,
    payments: Vec<Payment>,
    state: SimState,
    contract: Contract,
    trace: Vec<TraceStep>,
}

fn reduce_until_quiescent(
    environment: &Environment,
    state: &SimState,
    contract: &Contract,
    include_trace: bool,
) -> Result<ReduceUntilResult, SimError> {
    let mut current_state = state.clone();
    let mut current_contract = contract.clone();
    let mut warnings = Vec::new();
    let mut payments = Vec::new();
    let mut any_reduced = false;
    let mut trace = Vec::new();

    loop {
        match reduce_step(environment, &current_state, &current_contract)? {
            ReduceStep::NotReduced => {
                return Ok(ReduceUntilResult {
                    reduced: any_reduced,
                    warnings,
                    payments,
                    state: current_state,
                    contract: current_contract,
                    trace,
                });
            }
            ReduceStep::Reduced(step) => {
                any_reduced = true;
                if let Some(warning) = &step.warning {
                    warnings.push(warning.clone());
                }
                if let Some(payment) = &step.payment {
                    payments.push(payment.clone());
                }
                if include_trace {
                    trace.push(TraceStep::Reduced {
                        rule: step.rule.clone(),
                        warning: step.warning.clone(),
                        payment: step.payment.clone(),
                    });
                }
                current_state = step.state;
                current_contract = step.contract;
            }
        }
    }
}

enum ReduceStep {
    NotReduced,
    Reduced(Box<ReducedStepData>),
}

struct ReducedStepData {
    rule: TraceReduceRule,
    warning: Option<TransactionWarning>,
    payment: Option<Payment>,
    state: SimState,
    contract: Contract,
}

fn reduce_step(
    environment: &Environment,
    state: &SimState,
    contract: &Contract,
) -> Result<ReduceStep, SimError> {
    match contract {
        Contract::Hole(_) => Ok(ReduceStep::NotReduced),
        Contract::Close => Ok(refund_one(state)),
        Contract::Pay {
            from,
            to,
            token,
            amount,
            then,
        } => {
            let amount_to_pay = eval_value(state, environment, amount);
            if amount_to_pay <= BigInt::zero() {
                return Ok(ReduceStep::Reduced(Box::new(ReducedStepData {
                    rule: TraceReduceRule::Pay,
                    warning: Some(TransactionWarning::NonPositivePay {
                        from: from.clone(),
                        to: to.clone(),
                        token: token.clone(),
                        amount: amount_to_pay,
                    }),
                    payment: None,
                    state: state.clone(),
                    contract: (**then).clone(),
                })));
            }

            let account = AccountId {
                owner: from.clone(),
                token: token.clone(),
            };
            let balance = money_in_account(&state.accounts, &account);
            let paid = if balance < amount_to_pay {
                balance.clone()
            } else {
                amount_to_pay.clone()
            };

            let mut new_state = state.clone();
            update_money_in_account(&mut new_state.accounts, &account, &(balance - &paid));
            let payment = give_money(&mut new_state.accounts, from, to, token, &paid);

            let warning = if paid < amount_to_pay {
                Some(TransactionWarning::PartialPay {
                    from: from.clone(),
                    to: to.clone(),
                    token: token.clone(),
                    expected: amount_to_pay,
                    paid: paid.clone(),
                })
            } else {
                None
            };

            Ok(ReduceStep::Reduced(Box::new(ReducedStepData {
                rule: TraceReduceRule::Pay,
                warning,
                payment: Some(payment),
                state: new_state,
                contract: (**then).clone(),
            })))
        }
        Contract::If { cond, then, else_ } => {
            let selected = if eval_observation(state, environment, cond) {
                (**then).clone()
            } else {
                (**else_).clone()
            };
            Ok(ReduceStep::Reduced(Box::new(ReducedStepData {
                rule: TraceReduceRule::IfBranch,
                warning: None,
                payment: None,
                state: state.clone(),
                contract: selected,
            })))
        }
        Contract::When {
            cases: _,
            timeout,
            timeout_continuation,
        } => {
            let timeout = eval_timeout(timeout);
            if environment.end < timeout {
                Ok(ReduceStep::NotReduced)
            } else if timeout <= environment.start {
                Ok(ReduceStep::Reduced(Box::new(ReducedStepData {
                    rule: TraceReduceRule::WhenTimeout,
                    warning: None,
                    payment: None,
                    state: state.clone(),
                    contract: (**timeout_continuation).clone(),
                })))
            } else {
                Err(SimError::AmbiguousTimeInterval)
            }
        }
        Contract::Let { name, value, then } => {
            let evaluated = eval_value(state, environment, value);
            let mut new_state = state.clone();
            let warning = new_state
                .bound_values
                .insert(name.clone(), evaluated.clone())
                .map(|old| TransactionWarning::Shadowing {
                    name: name.clone(),
                    old,
                    new: evaluated.clone(),
                });

            Ok(ReduceStep::Reduced(Box::new(ReducedStepData {
                rule: TraceReduceRule::Let,
                warning,
                payment: None,
                state: new_state,
                contract: (**then).clone(),
            })))
        }
        Contract::Assert { cond, then } => {
            let warning = if eval_observation(state, environment, cond) {
                None
            } else {
                Some(TransactionWarning::AssertionFailed)
            };
            Ok(ReduceStep::Reduced(Box::new(ReducedStepData {
                rule: TraceReduceRule::Assert,
                warning,
                payment: None,
                state: state.clone(),
                contract: (**then).clone(),
            })))
        }
    }
}

fn refund_one(state: &SimState) -> ReduceStep {
    let mut target: Option<(AccountId, BigInt)> = None;
    for (account, balance) in &state.accounts {
        if balance > &BigInt::zero() {
            target = Some((account.clone(), balance.clone()));
            break;
        }
    }

    let Some((account, amount)) = target else {
        return ReduceStep::NotReduced;
    };

    let mut new_state = state.clone();
    new_state.accounts.remove(&account);
    let payment = Payment {
        from: account.owner.clone(),
        to: PayeeTarget::ToParty(account.owner.clone()),
        token: account.token.clone(),
        amount,
    };

    ReduceStep::Reduced(Box::new(ReducedStepData {
        rule: TraceReduceRule::CloseRefund,
        warning: None,
        payment: Some(payment),
        state: new_state,
        contract: Contract::Close,
    }))
}

struct ApplyInputResult {
    warning: Option<TransactionWarning>,
    state: SimState,
    contract: Contract,
}

fn apply_input(
    environment: &Environment,
    state: &SimState,
    contract: &Contract,
    input: &SimInput,
) -> Option<ApplyInputResult> {
    let Contract::When { cases, .. } = contract else {
        return None;
    };

    for case in cases {
        let Case::Case { action, then } = case else {
            continue;
        };
        match (action, input) {
            (
                Action::Deposit {
                    into,
                    by,
                    token,
                    amount,
                },
                SimInput::Deposit {
                    into: input_into,
                    by: input_by,
                    token: input_token,
                    amount: input_amount,
                },
            ) => {
                let expected = eval_value(state, environment, amount);
                if into == input_into
                    && by == input_by
                    && token == input_token
                    && expected == *input_amount
                {
                    let mut new_state = state.clone();
                    add_money_to_account(&mut new_state.accounts, into, token, input_amount);
                    let warning = if input_amount <= &BigInt::zero() {
                        Some(TransactionWarning::NonPositiveDeposit {
                            into: into.clone(),
                            by: by.clone(),
                            token: token.clone(),
                            amount: input_amount.clone(),
                        })
                    } else {
                        None
                    };
                    return Some(ApplyInputResult {
                        warning,
                        state: new_state,
                        contract: (**then).clone(),
                    });
                }
            }
            (
                Action::Choice { id, bounds },
                SimInput::Choice {
                    id: input_id,
                    value,
                },
            ) => {
                if id == input_id && in_bounds(value, bounds) {
                    let mut new_state = state.clone();
                    new_state.choices.insert(id.clone(), value.clone());
                    return Some(ApplyInputResult {
                        warning: None,
                        state: new_state,
                        contract: (**then).clone(),
                    });
                }
            }
            (Action::Notify { if_ }, SimInput::Notify) => {
                if eval_observation(state, environment, if_) {
                    return Some(ApplyInputResult {
                        warning: None,
                        state: state.clone(),
                        contract: (**then).clone(),
                    });
                }
            }
            _ => {}
        }
    }

    None
}

fn preview_case(case: &Case, state: &SimState, environment: &Environment) -> Option<PreviewInput> {
    let Case::Case { action, .. } = case else {
        return None;
    };

    match action {
        Action::Deposit {
            into,
            by,
            token,
            amount,
        } => Some(PreviewInput::Deposit {
            into: into.clone(),
            by: by.clone(),
            token: token.clone(),
            amount: eval_value(state, environment, amount),
        }),
        Action::Choice { id, bounds } => Some(PreviewInput::Choice {
            id: id.clone(),
            bounds: bounds
                .iter()
                .filter_map(|bound| match bound {
                    Bound::Bound { from, to } => Some((constant_value(from)?, constant_value(to)?)),
                    Bound::Hole(_) => None,
                })
                .collect(),
        }),
        Action::Notify { if_ } => Some(PreviewInput::Notify {
            can_notify: eval_observation(state, environment, if_),
        }),
        Action::Hole(_) => None,
    }
}

fn in_bounds(value: &BigInt, bounds: &[Bound]) -> bool {
    bounds.iter().any(|bound| match bound {
        Bound::Bound { from, to } => match (constant_value(from), constant_value(to)) {
            (Some(low), Some(high)) => value >= &low && value <= &high,
            _ => false,
        },
        Bound::Hole(_) => false,
    })
}

fn constant_value(value: &Value) -> Option<BigInt> {
    match value {
        Value::Constant(v) => Some(v.clone()),
        _ => None,
    }
}

fn give_money(
    accounts: &mut BTreeMap<AccountId, BigInt>,
    from: &Party,
    to: &PayeeTarget,
    token: &Token,
    amount: &BigInt,
) -> Payment {
    if let PayeeTarget::ToAccount(account_owner) = to {
        add_money_to_account(accounts, account_owner, token, amount);
    }

    Payment {
        from: from.clone(),
        to: to.clone(),
        token: token.clone(),
        amount: amount.clone(),
    }
}

fn money_in_account(accounts: &BTreeMap<AccountId, BigInt>, account: &AccountId) -> BigInt {
    accounts.get(account).cloned().unwrap_or_else(BigInt::zero)
}

fn update_money_in_account(
    accounts: &mut BTreeMap<AccountId, BigInt>,
    account: &AccountId,
    new_balance: &BigInt,
) {
    if new_balance <= &BigInt::zero() {
        accounts.remove(account);
    } else {
        accounts.insert(account.clone(), new_balance.clone());
    }
}

fn add_money_to_account(
    accounts: &mut BTreeMap<AccountId, BigInt>,
    owner: &Party,
    token: &Token,
    amount: &BigInt,
) {
    if amount <= &BigInt::zero() {
        return;
    }

    let account = AccountId {
        owner: owner.clone(),
        token: token.clone(),
    };
    let current = accounts.get(&account).cloned().unwrap_or_else(BigInt::zero);
    accounts.insert(account, current + amount);
}

fn eval_timeout(timeout: &Timeout) -> BigInt {
    match timeout {
        Timeout::PosixTime(value) => value.clone(),
        Timeout::Param(_) | Timeout::Hole(_) => BigInt::zero(),
    }
}

fn eval_value(state: &SimState, environment: &Environment, value: &Value) -> BigInt {
    match value {
        Value::Hole(_) | Value::Param(_) => BigInt::zero(),
        Value::Constant(v) => v.clone(),
        Value::Add(a, b) => eval_value(state, environment, a) + eval_value(state, environment, b),
        Value::Sub(a, b) => eval_value(state, environment, a) - eval_value(state, environment, b),
        Value::Mul(a, b) => eval_value(state, environment, a) * eval_value(state, environment, b),
        Value::Div(a, b) => {
            let lhs = eval_value(state, environment, a);
            let rhs = eval_value(state, environment, b);
            if rhs == BigInt::zero() {
                BigInt::zero()
            } else {
                lhs / rhs
            }
        }
        Value::Neg(v) => -eval_value(state, environment, v),
        Value::Abs(v) => eval_value(state, environment, v).abs(),
        Value::ValueFromChoice(choice_id) => state
            .choices
            .get(choice_id)
            .cloned()
            .unwrap_or_else(BigInt::zero),
        Value::UseValue(name) => state
            .bound_values
            .get(name)
            .cloned()
            .unwrap_or_else(BigInt::zero),
        Value::TimeIntervalStart => environment.start.clone(),
        Value::TimeIntervalEnd => environment.end.clone(),
        Value::AvailableMoney { account, token } => money_in_account(
            &state.accounts,
            &AccountId {
                owner: account.clone(),
                token: token.clone(),
            },
        ),
    }
}

fn eval_observation(
    state: &SimState,
    environment: &Environment,
    observation: &Observation,
) -> bool {
    match observation {
        Observation::Hole(_) => false,
        Observation::True => true,
        Observation::False => false,
        Observation::And(a, b) => {
            eval_observation(state, environment, a) && eval_observation(state, environment, b)
        }
        Observation::Or(a, b) => {
            eval_observation(state, environment, a) || eval_observation(state, environment, b)
        }
        Observation::Not(v) => !eval_observation(state, environment, v),
        Observation::ChoseSomething(choice_id) => state.choices.contains_key(choice_id),
        Observation::ValueGE(a, b) => {
            eval_value(state, environment, a) >= eval_value(state, environment, b)
        }
        Observation::ValueGT(a, b) => {
            eval_value(state, environment, a) > eval_value(state, environment, b)
        }
        Observation::ValueLT(a, b) => {
            eval_value(state, environment, a) < eval_value(state, environment, b)
        }
        Observation::ValueLE(a, b) => {
            eval_value(state, environment, a) <= eval_value(state, environment, b)
        }
        Observation::ValueEQ(a, b) => {
            eval_value(state, environment, a) == eval_value(state, environment, b)
        }
    }
}
