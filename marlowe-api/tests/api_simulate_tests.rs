use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
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
    assert_eq!(json["error"]["code"], "NotReadyToRun");
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
    assert_eq!(json["error"]["code"], "AmbiguousTimeInterval");
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
    assert_eq!(json["error"]["code"], "NoMatchForInput");
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
    assert_eq!(json["error"]["code"], "NotReadyToRun");
    assert_eq!(json["error"]["path"], "$.contract_yaml");
    assert!(json["error"]["diagnostics"].is_array());
}
