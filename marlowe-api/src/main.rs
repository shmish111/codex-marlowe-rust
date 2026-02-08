use axum::{routing::get, Json, Router};
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(health_handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind 127.0.0.1:3000");

    println!("listening on http://127.0.0.1:3000");

    axum::serve(listener, app).await.expect("server failed");
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
