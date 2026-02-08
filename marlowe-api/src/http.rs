use std::collections::BTreeMap;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

use crate::{
    ast::{ChoiceId, Party, PayeeTarget, Token},
    contract_to_yaml_string, parse_contract_yaml,
    sim::{
        preview_inputs, simulate_transaction, AccountId, Payment, PreviewInput, SimError, SimInput,
        SimState, SimTransaction, SimTransactionResult, TransactionWarning,
    },
    type_check, TypeCheckContext,
};

#[derive(Clone, Default)]
pub struct AppState;

pub fn build_router() -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/openapi.json", get(openapi_handler))
        .route("/simulate/step", post(simulate_step_handler))
        .route("/simulate/preview", post(simulate_preview_handler))
        .with_state(AppState)
}

#[derive(Serialize, ToSchema)]
struct HealthResponse {
    status: &'static str,
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses((status = 200, description = "Service health", body = HealthResponse))
)]
async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    tag = "health",
    responses((status = 200, description = "OpenAPI document", body = serde_json::Value))
)]
async fn openapi_handler() -> Json<serde_json::Value> {
    let doc = ApiDoc::openapi();
    let value = serde_json::to_value(&doc).unwrap_or_else(|_| serde_json::json!({}));
    Json(value)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SimulateStepRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub state: SimulateStateRequest,
    pub transaction: SimulateTxRequest,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SimulateStateRequest {
    #[serde(default = "zero")]
    #[schema(value_type = String, example = "0")]
    pub min_time: BigIntValue,
    #[serde(default)]
    pub accounts: Vec<AccountBalanceRequest>,
    #[serde(default)]
    pub choices: Vec<ChoiceValueRequest>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub bound_values: BTreeMap<String, BigIntValue>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountBalanceRequest {
    pub owner: Party,
    pub token: Token,
    #[schema(value_type = String, example = "100")]
    pub amount: BigIntValue,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChoiceValueRequest {
    pub id: ChoiceId,
    #[schema(value_type = String, example = "1")]
    pub value: BigIntValue,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SimulateTxRequest {
    #[schema(value_type = String, example = "0")]
    pub interval_start: BigIntValue,
    #[schema(value_type = String, example = "100")]
    pub interval_end: BigIntValue,
    #[serde(default)]
    pub inputs: Vec<SimInputRequest>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SimulatePreviewRequest {
    pub contract_yaml: String,
    #[serde(default)]
    pub state: SimulateStateRequest,
    #[schema(value_type = String, example = "0")]
    pub interval_start: BigIntValue,
    #[schema(value_type = String, example = "100")]
    pub interval_end: BigIntValue,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SimInputRequest {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        #[schema(value_type = String, example = "5")]
        amount: BigIntValue,
    },
    Choice {
        id: ChoiceId,
        #[schema(value_type = String, example = "1")]
        value: BigIntValue,
    },
    Notify,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulateStepResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<SimulateSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulateSuccessResponse {
    pub warnings: Vec<WarningResponse>,
    pub payments: Vec<PaymentResponse>,
    pub state: StateResponse,
    pub contract_yaml: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulateErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<ErrorDiagnosticResponse>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDiagnosticResponse {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WarningResponse {
    pub kind: String,
    pub details: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PaymentResponse {
    pub from: Party,
    pub to: PayeeTarget,
    pub token: Token,
    pub amount: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StateResponse {
    pub min_time: String,
    pub accounts: Vec<AccountBalanceResponse>,
    pub choices: Vec<ChoiceValueResponse>,
    pub bound_values: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountBalanceResponse {
    pub owner: Party,
    pub token: Token,
    pub amount: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChoiceValueResponse {
    pub id: ChoiceId,
    pub value: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SimulatePreviewResponse {
    pub result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<PreviewSuccessResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulateErrorResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PreviewSuccessResponse {
    pub state: StateResponse,
    pub contract_yaml: String,
    pub inputs: Vec<PreviewInputResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreviewInputResponse {
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: String,
    },
    Choice {
        id: ChoiceId,
        bounds: Vec<PreviewBoundResponse>,
    },
    Notify {
        can_notify: bool,
    },
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PreviewBoundResponse {
    pub from: String,
    pub to: String,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health_handler,
        openapi_handler,
        simulate_step_handler,
        simulate_preview_handler
    ),
    components(
        schemas(
            HealthResponse,
            SimulateStepRequest,
            SimulateStateRequest,
            AccountBalanceRequest,
            ChoiceValueRequest,
            SimulateTxRequest,
            SimInputRequest,
            SimulatePreviewRequest,
            BigIntValue,
            SimulateStepResponse,
            SimulateSuccessResponse,
            SimulateErrorResponse,
            ErrorDiagnosticResponse,
            WarningResponse,
            PaymentResponse,
            StateResponse,
            AccountBalanceResponse,
            ChoiceValueResponse,
            SimulatePreviewResponse,
            PreviewSuccessResponse,
            PreviewInputResponse,
            PreviewBoundResponse,
            Party,
            Token,
            ChoiceId,
            PayeeTarget
        )
    ),
    tags(
        (name = "health", description = "Service health"),
        (name = "simulation", description = "Marlowe simulation APIs")
    )
)]
pub struct ApiDoc;

pub fn openapi_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&ApiDoc::openapi())
}

#[utoipa::path(
    post,
    path = "/simulate/step",
    tag = "simulation",
    request_body = SimulateStepRequest,
    responses(
        (status = 200, description = "Simulation step succeeded", body = SimulateStepResponse),
        (status = 400, description = "Simulation step rejected", body = SimulateStepResponse),
        (status = 500, description = "Internal server error", body = SimulateStepResponse)
    )
)]
pub async fn simulate_step_handler(
    _state: State<AppState>,
    Json(request): Json<SimulateStepRequest>,
) -> (StatusCode, Json<SimulateStepResponse>) {
    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => {
            return bad_request(
                "ParseError",
                format!("{}: {}", err.path, err.message),
                Some(err.path),
                None,
            )
        }
    };

    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty()
        || !validation.holes.is_empty()
        || !validation.params.is_empty()
    {
        let mut diagnostics = Vec::new();
        for error in &validation.errors {
            diagnostics.push(ErrorDiagnosticResponse {
                code: "TypeError".to_owned(),
                path: error.path.clone(),
                message: error.message.clone(),
            });
        }
        for hole in &validation.holes {
            diagnostics.push(ErrorDiagnosticResponse {
                code: "Hole".to_owned(),
                path: hole.path.clone(),
                message: format!(
                    "hole '{}' has inferred type {}",
                    hole.name,
                    hole.ty.as_str()
                ),
            });
        }
        for param in &validation.params {
            diagnostics.push(ErrorDiagnosticResponse {
                code: "Param".to_owned(),
                path: param.path.clone(),
                message: format!(
                    "parameter '{}' has inferred type {} and must be instantiated",
                    param.name,
                    param.ty.as_str()
                ),
            });
        }
        return bad_request(
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before simulation".to_owned(),
            Some("$.contract_yaml".to_owned()),
            Some(diagnostics),
        );
    };

    let state = match map_state_request(request.state) {
        Ok(state) => state,
        Err(message) => {
            return bad_request("StateError", message, Some("$.state".to_owned()), None)
        }
    };

    let transaction = match map_tx_request(request.transaction) {
        Ok(transaction) => transaction,
        Err(message) => {
            return bad_request(
                "TransactionError",
                message,
                Some("$.transaction".to_owned()),
                None,
            )
        }
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
        SimTransactionResult::Error(err) => bad_request(
            &sim_error_code(&err),
            sim_error_message(&err),
            sim_error_path(&err),
            None,
        ),
    }
}

#[utoipa::path(
    post,
    path = "/simulate/preview",
    tag = "simulation",
    request_body = SimulatePreviewRequest,
    responses(
        (status = 200, description = "Preview generated", body = SimulatePreviewResponse),
        (status = 400, description = "Preview rejected", body = SimulatePreviewResponse),
        (status = 500, description = "Internal server error", body = SimulatePreviewResponse)
    )
)]
pub async fn simulate_preview_handler(
    _state: State<AppState>,
    Json(request): Json<SimulatePreviewRequest>,
) -> (StatusCode, Json<SimulatePreviewResponse>) {
    let contract = match parse_contract_yaml(&request.contract_yaml) {
        Ok(contract) => contract,
        Err(err) => {
            return preview_bad_request(
                "ParseError",
                format!("{}: {}", err.path, err.message),
                Some(err.path),
                None,
            )
        }
    };

    let validation = type_check(&contract, &TypeCheckContext::default());
    if !validation.errors.is_empty()
        || !validation.holes.is_empty()
        || !validation.params.is_empty()
    {
        let mut diagnostics = Vec::new();
        for error in &validation.errors {
            diagnostics.push(ErrorDiagnosticResponse {
                code: "TypeError".to_owned(),
                path: error.path.clone(),
                message: error.message.clone(),
            });
        }
        for hole in &validation.holes {
            diagnostics.push(ErrorDiagnosticResponse {
                code: "Hole".to_owned(),
                path: hole.path.clone(),
                message: format!(
                    "hole '{}' has inferred type {}",
                    hole.name,
                    hole.ty.as_str()
                ),
            });
        }
        for param in &validation.params {
            diagnostics.push(ErrorDiagnosticResponse {
                code: "Param".to_owned(),
                path: param.path.clone(),
                message: format!(
                    "parameter '{}' has inferred type {} and must be instantiated",
                    param.name,
                    param.ty.as_str()
                ),
            });
        }
        return preview_bad_request(
            "NotReadyToRun",
            "contract must be fully instantiated and type-safe before simulation".to_owned(),
            Some("$.contract_yaml".to_owned()),
            Some(diagnostics),
        );
    };

    let state = match map_state_request(request.state) {
        Ok(state) => state,
        Err(message) => {
            return preview_bad_request("StateError", message, Some("$.state".to_owned()), None)
        }
    };
    let interval_start = match request.interval_start.into_bigint() {
        Ok(value) => value,
        Err(message) => {
            return preview_bad_request(
                "TransactionError",
                message,
                Some("$.interval_start".to_owned()),
                None,
            )
        }
    };
    let interval_end = match request.interval_end.into_bigint() {
        Ok(value) => value,
        Err(message) => {
            return preview_bad_request(
                "TransactionError",
                message,
                Some("$.interval_end".to_owned()),
                None,
            )
        }
    };

    match preview_inputs(&contract, &state, &interval_start, &interval_end) {
        Ok(preview) => {
            let contract_yaml = match contract_to_yaml_string(&preview.contract) {
                Ok(yaml) => yaml,
                Err(err) => {
                    return preview_internal_error(format!("failed to serialize contract: {err}"))
                }
            };

            (
                StatusCode::OK,
                Json(SimulatePreviewResponse {
                    result: "success",
                    success: Some(PreviewSuccessResponse {
                        state: state_to_response(&preview.state),
                        contract_yaml,
                        inputs: preview
                            .inputs
                            .iter()
                            .map(preview_input_to_response)
                            .collect(),
                    }),
                    error: None,
                }),
            )
        }
        Err(err) => preview_bad_request(
            &sim_error_code(&err),
            sim_error_message(&err),
            sim_error_path(&err),
            None,
        ),
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

fn preview_input_to_response(input: &PreviewInput) -> PreviewInputResponse {
    match input {
        PreviewInput::Deposit {
            into,
            by,
            token,
            amount,
        } => PreviewInputResponse::Deposit {
            into: into.clone(),
            by: by.clone(),
            token: token.clone(),
            amount: amount.to_string(),
        },
        PreviewInput::Choice { id, bounds } => PreviewInputResponse::Choice {
            id: id.clone(),
            bounds: bounds
                .iter()
                .map(|(from, to)| PreviewBoundResponse {
                    from: from.to_string(),
                    to: to.to_string(),
                })
                .collect(),
        },
        PreviewInput::Notify { can_notify } => PreviewInputResponse::Notify {
            can_notify: *can_notify,
        },
    }
}

fn bad_request(
    code: &str,
    message: String,
    path: Option<String>,
    diagnostics: Option<Vec<ErrorDiagnosticResponse>>,
) -> (StatusCode, Json<SimulateStepResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(SimulateStepResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                message,
                path,
                diagnostics,
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
                path: None,
                diagnostics: None,
            }),
        }),
    )
}

fn preview_bad_request(
    code: &str,
    message: String,
    path: Option<String>,
    diagnostics: Option<Vec<ErrorDiagnosticResponse>>,
) -> (StatusCode, Json<SimulatePreviewResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(SimulatePreviewResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: code.to_owned(),
                message,
                path,
                diagnostics,
            }),
        }),
    )
}

fn preview_internal_error(message: String) -> (StatusCode, Json<SimulatePreviewResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(SimulatePreviewResponse {
            result: "error",
            success: None,
            error: Some(SimulateErrorResponse {
                code: "InternalError".to_owned(),
                message,
                path: None,
                diagnostics: None,
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

fn sim_error_path(error: &SimError) -> Option<String> {
    match error {
        SimError::NotReadyToRun => Some("$.contract_yaml".to_owned()),
        SimError::InvalidInterval { .. } => Some("$.transaction".to_owned()),
        SimError::IntervalInPast { .. } => Some("$.transaction".to_owned()),
        SimError::AmbiguousTimeInterval => Some("$.transaction".to_owned()),
        SimError::NoMatchForInput { input_index } => {
            Some(format!("$.transaction.inputs[{input_index}]"))
        }
        SimError::UselessTransaction => Some("$.transaction".to_owned()),
    }
}
