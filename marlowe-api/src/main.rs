use marlowe_api::http::build_router;

#[tokio::main]
async fn main() {
    let app = build_router();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind 127.0.0.1:3000");

    println!("listening on http://127.0.0.1:3000");

    axum::serve(listener, app).await.expect("server failed");
}
