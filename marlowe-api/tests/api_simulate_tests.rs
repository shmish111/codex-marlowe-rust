use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use marlowe_api::http::build_router;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn json_response(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice::<Value>(&body).expect("valid json")
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let app = build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    let status = response.status();
    let json = json_response(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn openapi_endpoint_returns_spec() {
    let app = build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    let status = response.status();
    let json = json_response(response).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["paths"]["/simulate/step"].is_object());
    assert!(json["paths"]["/simulate/preview"].is_object());
}

#[tokio::test]
async fn cors_header_is_returned_for_origin_request() {
    let app = build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .method("GET")
                .header(header::ORIGIN, "http://localhost:5173")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok()),
        Some("*")
    );
}

#[tokio::test]
async fn cors_preflight_options_is_handled() {
    let app = build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("OPTIONS")
                .header(header::ORIGIN, "http://localhost:5173")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert!(response.status().is_success());
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok()),
        Some("*")
    );
}

#[tokio::test]
async fn simulate_step_success_response() {
    let app = build_router();
    let contract = r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": [
          {
            "deposit": {
              "into": {"Role": "Alice"},
              "by": {"Role": "Alice"},
              "token": {"Token": {"currency_symbol": "", "token_name": ""}},
              "amount": "5"
            }
          }
        ]
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    let status = response.status();
    let json = json_response(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["result"], "success");
    assert_eq!(json["success"]["payments"][0]["amount"], "5");
    assert_eq!(
        json["success"]["state"]["accounts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(json["success"].get("trace").is_none());
}

#[tokio::test]
async fn simulate_step_trace_mode_returns_semantic_events() {
    let app = build_router();
    let contract = r#"
Pay:
  from: { Role: "Alice" }
  to_party: { Role: "Bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 10 }
  then: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "trace": true,
      "state": {
        "accounts": [
          {
            "owner": {"Role": "Alice"},
            "token": {"Token": {"currency_symbol": "", "token_name": ""}},
            "amount": "3"
          }
        ]
      },
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": []
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    let trace = json["success"]["trace"].as_array().expect("trace array");
    assert!(!trace.is_empty());
    assert_eq!(trace[0]["code"], "Reduced");
    assert_eq!(trace[0]["rule"], "Pay");
    assert_eq!(trace[0]["warning"]["code"], "PartialPay");
    assert_eq!(trace[0]["warning"]["expected"], "10");
    assert_eq!(trace[0]["warning"]["paid"], "3");
    assert_eq!(trace[0]["payment"]["amount"], "3");
    assert_eq!(
        trace[0]["delta"]["accounts_removed"][0]["owner"],
        json!({"Role": "Alice"})
    );
    assert_eq!(
        trace[0]["delta"]["accounts_removed"][0]["token"],
        json!({"Token": {"currency_symbol": "", "token_name": ""}})
    );
    assert!(trace[0]["delta"].get("accounts_upserted").is_none());
}

#[tokio::test]
async fn simulate_step_trace_mode_includes_input_state_delta() {
    let app = build_router();
    let contract = r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "trace": true,
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": [
          {
            "deposit": {
              "into": {"Role": "Alice"},
              "by": {"Role": "Alice"},
              "token": {"Token": {"currency_symbol": "", "token_name": ""}},
              "amount": "5"
            }
          }
        ]
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    let trace = json["success"]["trace"].as_array().expect("trace array");
    let input_event = trace
        .iter()
        .find(|event| event["code"] == "InputApplied")
        .expect("input event");
    assert_eq!(input_event["input_index"], 0);
    assert_eq!(input_event["input"]["kind"], "deposit");
    assert_eq!(input_event["delta"]["accounts_upserted"][0]["amount"], "5");
    assert_eq!(
        input_event["delta"]["accounts_upserted"][0]["owner"],
        json!({"Role": "Alice"})
    );
}

#[tokio::test]
async fn simulate_step_rejects_uninstantiated_contract() {
    let app = build_router();
    let request_body = json!({
      "contract_yaml": "When:\n  cases: []\n  timeout: $deadline\n  timeout_continuation: { Close: {} }\n",
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": []
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = json_response(response).await;
    assert_eq!(json["error"]["code"], "ValidationError");
    assert_eq!(json["error"]["subcode"], "NotReadyToRun");
    assert_eq!(json["error"]["path"], "$.contract_yaml");
    assert!(json["error"]["diagnostics"].is_array());
    assert!(json["error"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["subcode"] == "ParamUnresolved" && d["code"] == "Validation"));
}

#[tokio::test]
async fn simulate_step_rejects_ambiguous_interval() {
    let app = build_router();
    let request_body = json!({
      "contract_yaml": "When:\n  cases: []\n  timeout: { Timeout: 10 }\n  timeout_continuation: { Close: {} }\n",
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": []
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = json_response(response).await;
    assert_eq!(json["error"]["code"], "SimulationError");
    assert_eq!(json["error"]["subcode"], "AmbiguousTimeInterval");
    assert_eq!(json["error"]["path"], "$.transaction");
}

#[tokio::test]
async fn simulate_step_no_match_input_has_input_path() {
    let app = build_router();
    let contract = r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": [
          {
            "deposit": {
              "into": {"Role": "Alice"},
              "by": {"Role": "Alice"},
              "token": {"Token": {"currency_symbol": "", "token_name": ""}},
              "amount": "2"
            }
          }
        ]
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = json_response(response).await;
    assert_eq!(json["error"]["code"], "SimulationError");
    assert_eq!(json["error"]["subcode"], "NoMatchForInput");
    assert_eq!(json["error"]["path"], "$.transaction.inputs[0]");
}

#[tokio::test]
async fn simulate_step_clamps_interval_to_state_min_time() {
    let app = build_router();
    let contract = r#"
Pay:
  from: { Role: "Alice" }
  to_party: { Role: "Bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { TimeIntervalStart: {} }
  then: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "state": {
        "min_time": "40",
        "accounts": [
          {
            "owner": {"Role": "Alice"},
            "token": {"Token": {"currency_symbol": "", "token_name": ""}},
            "amount": "100"
          }
        ]
      },
      "transaction": {
        "interval_start": "10",
        "interval_end": "50",
        "inputs": []
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    assert_eq!(json["success"]["payments"][0]["amount"], "40");
    assert_eq!(json["success"]["state"]["min_time"], "40");
}

#[tokio::test]
async fn simulate_step_returns_typed_non_positive_deposit_warning() {
    let app = build_router();
    let contract = r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 0 }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": [
          {
            "deposit": {
              "into": {"Role": "Alice"},
              "by": {"Role": "Alice"},
              "token": {"Token": {"currency_symbol": "", "token_name": ""}},
              "amount": "0"
            }
          }
        ]
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    assert_eq!(json["success"]["warnings"][0]["code"], "NonPositiveDeposit");
    assert_eq!(json["success"]["warnings"][0]["amount"], "0");
    assert_eq!(
        json["success"]["warnings"][0]["into"],
        json!({"Role": "Alice"})
    );
    assert!(json["success"]["warnings"][0].get("details").is_none());
}

#[tokio::test]
async fn simulate_step_returns_typed_partial_pay_warning() {
    let app = build_router();
    let contract = r#"
Pay:
  from: { Role: "Alice" }
  to_party: { Role: "Bob" }
  token: { Token: { currency_symbol: "", token_name: "" } }
  amount: { Constant: 10 }
  then: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "state": {
        "accounts": [
          {
            "owner": {"Role": "Alice"},
            "token": {"Token": {"currency_symbol": "", "token_name": ""}},
            "amount": "3"
          }
        ]
      },
      "transaction": {
        "interval_start": "0",
        "interval_end": "10",
        "inputs": []
      }
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/step")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    assert_eq!(json["success"]["warnings"][0]["code"], "PartialPay");
    assert_eq!(json["success"]["warnings"][0]["expected"], "10");
    assert_eq!(json["success"]["warnings"][0]["paid"], "3");
    assert_eq!(
        json["success"]["warnings"][0]["to"],
        json!({"ToParty": {"Role": "Bob"}})
    );
    assert!(json["success"]["warnings"][0].get("details").is_none());
}

#[tokio::test]
async fn simulate_preview_lists_inputs_for_when_contract() {
    let app = build_router();
    let contract = r#"
When:
  cases:
    - Case:
        action:
          Deposit:
            into: { Role: "Alice" }
            by: { Role: "Alice" }
            token: { Token: { currency_symbol: "", token_name: "" } }
            amount: { Constant: 5 }
        then: { Close: {} }
    - Case:
        action:
          Notify:
            if: { "True": {} }
        then: { Close: {} }
  timeout: { Timeout: 100 }
  timeout_continuation: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "interval_start": "0",
      "interval_end": "10"
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/preview")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    assert_eq!(json["result"], "success");
    assert_eq!(json["success"]["inputs"].as_array().unwrap().len(), 2);
    assert_eq!(json["success"]["inputs"][0]["deposit"]["amount"], "5");
    assert_eq!(json["success"]["inputs"][1]["notify"]["can_notify"], true);
}

#[tokio::test]
async fn simulate_preview_returns_locatable_not_ready_error() {
    let app = build_router();
    let contract = r#"
When:
  cases: []
  timeout: "$deadline"
  timeout_continuation: { Close: {} }
"#;

    let request_body = json!({
      "contract_yaml": contract,
      "interval_start": "0",
      "interval_end": "10"
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/simulate/preview")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = json_response(response).await;
    assert_eq!(json["error"]["code"], "ValidationError");
    assert_eq!(json["error"]["subcode"], "NotReadyToRun");
    assert_eq!(json["error"]["path"], "$.contract_yaml");
    assert!(json["error"]["diagnostics"].is_array());
}
