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

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    assert_eq!(json["status"], "ok");
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

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
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
}
