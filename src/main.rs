use axum::routing::{get, post};
use axum::Router;
use dotenv::dotenv;
use tower_http::trace::{self, TraceLayer};
use tracing::Level;

use controller::{execute_task_handler, get_task_status, test_run_handler};
use runner::{connect_to_amqp, listen_consumer};

mod runner;

mod controller;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    dotenv().ok();

    let conn = connect_to_amqp()
        .await
        .expect("AMQP server must be running");

    tokio::spawn(async move {
        listen_consumer(conn).await;
    });

    let app = Router::new()
        .route("/api/v1/code-engine/execute", post(execute_task_handler))
        .route("/api/v1/code-engine/test-run", post(test_run_handler))
        .route("/api/v1/code-engine/task/:task_id", get(get_task_status))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await.unwrap();
    println!("Server listening on port 4000");
    axum::serve(listener, app).await.unwrap();
}
