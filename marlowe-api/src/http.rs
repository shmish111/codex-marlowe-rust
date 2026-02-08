use std::collections::BTreeMap;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

use crate::{
    ast::{ChoiceId, Party, PayeeTarget, Token},
    contract_to_yaml_string, parse_contract_yaml,
    sim::{
        simulate_transaction, AccountId, Payment, SimError, SimInput, SimState, SimTransaction,
        SimTransactionResult, TransactionWarning,
    },
};

#[derive(Clone, Default)]
pub struct AppState;

pub fn build_router() -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/simulate/step", post(simulate_step_handler))
        .with_state(AppState)
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Debug, Deserialize)]
pub struct SimulateStepRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub state: SimulateStateRequest,
    pub transaction: SimulateTxRequest,
}

#[derive(Debug, Default, Deserialize)]
pub struct SimulateStateRequest {
    #[serde(default = "zero")]
    pub min_time: BigIntValue,
    #[serde(default)]
    pub accounts: Vec<AccountBalanceRequest>,
    #[serde(default)]
    pub choices: Vec<ChoiceValueRequest>,
    #[serde(default)]
    pub bound_values: BTreeMap<String, BigIntValue>,
}

#[derive(Debug, Deserialize)]
pub struct AccountBalanceRequest {
    pub owner: Party,
    pub token: Token,
    pub amount: BigIntValue,
}

#[derive(Debug, Deserialize)]
pub struct ChoiceValueRequest {
    pub id: ChoiceId,
    pub value: BigIntValue,
}

#[derive(Debug, Deserialize)]
pub struct SimulateTxRequest {
    pub interval_start: BigIntValue,
    pub interval_end: BigIntValue,
    #[serde(default)]
    pub inputs: Vec<SimInputRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimInputRequest {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: BigIntValue,
    },
    Choice {
        id: ChoiceId,
        value: BigIntValue,
    },
    Notify,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(untagged)]
pub enum BigIntValue {
    Integer(i64),
    String(String),
    #[default]
    Missing,
}

impl BigIntValue {
    fn into_bigint(self) -> Result<BigInt, String> {
        match self {
            BigIntValue::Integer(value) => Ok(BigInt::from(value)),
            BigIntValue::String(value) => value
                .parse::<BigInt>()
                .map_err(|_| format!("invalid integer '{value}'")),
            BigIntValue::Missing => Ok(BigInt::from(0)),
        }
    }
}

fn zero() -> BigIntValue {
    BigIntValue::Integer(0)
}

#[derive(Debug, Serialize)]
pub struct SimulateStepResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<SimulateSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize)]
pub struct SimulateSuccessResponse {
    pub warnings: Vec<WarningResponse>,
    pub payments: Vec<PaymentResponse>,
    pub state: StateResponse,
    pub contract_yaml: String,
}

#[derive(Debug, Serialize)]
pub struct SimulateErrorResponse {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct WarningResponse {
    pub kind: String,
    pub details: String,
}

#[derive(Debug, Serialize)]
pub struct PaymentResponse {
    pub from: Party,
    pub to: PayeeTarget,
    pub token: Token,
    pub amount: String,
}

#[derive(Debug, Serialize)]
pub struct StateResponse {
    pub min_time: String,
    pub accounts: Vec<AccountBalanceResponse>,
    pub choices: Vec<ChoiceValueResponse>,
    pub bound_values: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct AccountBalanceResponse {
    pub owner: Party,
    pub token: Token,
    pub amount: String,
}

#[derive(Debug, Serialize)]
pub struct ChoiceValueResponse {
    pub id: ChoiceId,
    pub value: String,
}

pub async fn simulate_step_handler(
    State(_): State<AppState>,
    Json(request): Json<SimulateStepRequest>,
) -> (StatusCode, Json<SimulateStepResponse>) {
    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => return bad_request("ParseError", format!("{}: {}", err.path, err.message)),
    };

    let state = match map_state_request(request.state) {
        Ok(state) => state,
        Err(message) => return bad_request("StateError", message),
    };

    let transaction = match map_tx_request(request.transaction) {
        Ok(transaction) => transaction,
        Err(message) => return bad_request("TransactionError", message),
    };

    match simulate_transaction(&contract, &state, &transaction) {
        SimTransactionResult::Success(success) => {
            let contract_yaml = match contract_to_yaml_string(&success.contract) {
                Ok(yaml) => yaml,
                Err(err) => return internal_error(format!("failed to serialize contract: {err}")),
            };

            let response = SimulateStepResponse {
                result: "success",
                success: Some(SimulateSuccessResponse {
                    warnings: success.warnings.iter().map(warning_to_response).collect(),
                    payments: success.payments.iter().map(payment_to_response).collect(),
                    state: state_to_response(&success.state),
                    contract_yaml,
                }),
                error: None,
            };
            (StatusCode::OK, Json(response))
        }
        SimTransactionResult::Error(err) => {
            bad_request(&sim_error_code(&err), sim_error_message(&err))
        }
    }
}

fn map_state_request(request: SimulateStateRequest) -> Result<SimState, String> {
    let mut accounts = BTreeMap::new();
    for account in request.accounts {
        accounts.insert(
            AccountId {
                owner: account.owner,
                token: account.token,
            },
            account.amount.into_bigint()?,
        );
    }

    let mut choices = BTreeMap::new();
    for choice in request.choices {
        choices.insert(choice.id, choice.value.into_bigint()?);
    }

    let mut bound_values = BTreeMap::new();
    for (name, value) in request.bound_values {
        bound_values.insert(name, value.into_bigint()?);
    }

    Ok(SimState {
        accounts,
        choices,
        bound_values,
        min_time: request.min_time.into_bigint()?,
    })
}

fn map_tx_request(request: SimulateTxRequest) -> Result<SimTransaction, String> {
    let mut inputs = Vec::with_capacity(request.inputs.len());
    for input in request.inputs {
        let mapped = match input {
            SimInputRequest::Deposit {
                into,
                by,
                token,
                amount,
            } => SimInput::Deposit {
                into,
                by,
                token,
                amount: amount.into_bigint()?,
            },
            SimInputRequest::Choice { id, value } => SimInput::Choice {
                id,
                value: value.into_bigint()?,
            },
            SimInputRequest::Notify => SimInput::Notify,
        };
        inputs.push(mapped);
    }

    Ok(SimTransaction {
        interval_start: request.interval_start.into_bigint()?,
        interval_end: request.interval_end.into_bigint()?,
        inputs,
    })
}

fn warning_to_response(warning: &TransactionWarning) -> WarningResponse {
    match warning {
        TransactionWarning::NonPositiveDeposit { amount, .. } => WarningResponse {
            kind: "NonPositiveDeposit".to_owned(),
            details: format!("deposit amount {} is not positive", amount),
        },
        TransactionWarning::NonPositivePay { amount, .. } => WarningResponse {
            kind: "NonPositivePay".to_owned(),
            details: format!("pay amount {} is not positive", amount),
        },
        TransactionWarning::PartialPay { expected, paid, .. } => WarningResponse {
            kind: "PartialPay".to_owned(),
            details: format!("expected {}, paid {}", expected, paid),
        },
        TransactionWarning::Shadowing { name, .. } => WarningResponse {
            kind: "Shadowing".to_owned(),
            details: format!("let value '{}' was shadowed", name),
        },
        TransactionWarning::AssertionFailed => WarningResponse {
            kind: "AssertionFailed".to_owned(),
            details: "assertion evaluated to false".to_owned(),
        },
    }
}

fn payment_to_response(payment: &Payment) -> PaymentResponse {
    PaymentResponse {
        from: payment.from.clone(),
        to: payment.to.clone(),
        token: payment.token.clone(),
        amount: payment.amount.to_string(),
    }
}

fn state_to_response(state: &SimState) -> StateResponse {
    StateResponse {
        min_time: state.min_time.to_string(),
        accounts: state
            .accounts
            .iter()
            .map(|(account, amount)| AccountBalanceResponse {
                owner: account.owner.clone(),
                token: account.token.clone(),
                amount: amount.to_string(),
            })
            .collect(),
        choices: state
            .choices
            .iter()
            .map(|(id, value)| ChoiceValueResponse {
                id: id.clone(),
                value: value.to_string(),
            })
            .collect(),
        bound_values: state
            .bound_values
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
    }
}

fn bad_request(code: &str, message: String) -> (StatusCode, Json<SimulateStepResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(SimulateStepResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                message,
            }),
        }),
    )
}

fn internal_error(message: String) -> (StatusCode, Json<SimulateStepResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(SimulateStepResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: "InternalError".to_owned(),
                message,
            }),
        }),
    )
}

fn sim_error_code(error: &SimError) -> String {
    match error {
        SimError::NotReadyToRun => "NotReadyToRun",
        SimError::InvalidInterval { .. } => "InvalidInterval",
        SimError::IntervalInPast { .. } => "IntervalInPast",
        SimError::AmbiguousTimeInterval => "AmbiguousTimeInterval",
        SimError::NoMatchForInput { .. } => "NoMatchForInput",
        SimError::UselessTransaction => "UselessTransaction",
    }
    .to_owned()
}

fn sim_error_message(error: &SimError) -> String {
    match error {
        SimError::NotReadyToRun => {
            "contract must be fully instantiated and type-safe before simulation".to_owned()
        }
        SimError::InvalidInterval { start, end } => {
            format!(
                "invalid interval: start {} is greater than end {}",
                start, end
            )
        }
        SimError::IntervalInPast {
            min_time,
            start,
            end,
        } => format!(
            "interval [{}, {}] is before current minimum time {}",
            start, end, min_time
        ),
        SimError::AmbiguousTimeInterval => {
            "interval straddles timeout boundary and is ambiguous".to_owned()
        }
        SimError::NoMatchForInput { input_index } => {
            format!(
                "input at index {} did not match any contract case",
                input_index
            )
        }
        SimError::UselessTransaction => "transaction made no state or contract changes".to_owned(),
    }
}
