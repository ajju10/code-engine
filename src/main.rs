use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use dotenv::dotenv;
use futures_util::stream::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Connection, ConnectionProperties};
use mongodb::bson::{doc, DateTime};
use mongodb::{Client, Collection};
use std::error::Error;
use std::time::Duration;
use tokio::time::sleep;
use tower_http::trace::{self, TraceLayer};
use tracing::Level;
use uuid::Uuid;

use runner::{
    get_runner, get_test_case_run_status, PublishMessagePayload, TaskRequest, TaskResponse,
    TaskStatus, TaskStatusResponse, TestCaseResult, TestCaseStatus, TestRunRequest,
    TestRunResponse, TestRunStatus,
};

mod runner;

const SUBMISSION_QUEUE: &str = "SUBMISSION_QUEUE";
const RETRY_INTERVAL: u64 = 5; // seconds
const MAX_RETRIES: u32 = 12; // retry for 1 minute

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

    let channel = conn.create_channel().await.expect("create channel error");

    channel
        .queue_declare(
            SUBMISSION_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare queue declare error");

    tokio::spawn(async move {
        let mut consumer = channel
            .basic_consume(
                SUBMISSION_QUEUE,
                "submission_consumer",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .expect("Failed to initialize consumer");

        while let Some(delivery) = consumer.next().await {
            if let Ok(delivery) = delivery {
                let task_data: PublishMessagePayload =
                    serde_json::from_slice(&delivery.data).expect("Failed to deserialize message");

                let task_handle = tokio::spawn(async move {
                    execute_task(task_data).await;
                });

                match task_handle.await {
                    Ok(_) => println!("Task completed successfully"),
                    Err(err) => println!("Error occured during task {:?}", err.to_string()),
                }
                delivery
                    .ack(BasicAckOptions::default())
                    .await
                    .expect("Failed to acknowledge the message");
            }
        }
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

async fn execute_task_handler(
    Json(payload): Json<TaskRequest>,
) -> (StatusCode, Json<TaskResponse>) {
    let addr = std::env::var("AMQP_DEV_URL").unwrap_or_else(|_| "amqp://rabbitmq:5672/%2f".into());
    let conn = Connection::connect(&addr, ConnectionProperties::default())
        .await
        .expect("connection error");
    let channel = conn.create_channel().await.expect("create channel error");

    channel
        .queue_declare(
            SUBMISSION_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare queue");

    let task_id = Uuid::new_v4();
    let publish_payload = PublishMessagePayload {
        task_id,
        task_request: payload,
    };

    let payload_bytes = serde_json::to_vec(&publish_payload).expect("Failed to serialize payload");

    channel
        .basic_publish(
            "",
            SUBMISSION_QUEUE,
            BasicPublishOptions::default(),
            &payload_bytes,
            BasicProperties::default(),
        )
        .await
        .expect("Failed to publish payload");

    let res = TaskResponse {
        task_id,
        status: "Queued".into(),
        message: "Request queued for processing".into(),
    };

    (StatusCode::ACCEPTED, Json(res))
}

async fn test_run_handler(
    Json(payload): Json<TestRunRequest>,
) -> (StatusCode, Json<TestRunResponse>) {
    println!("Test run request received");
    let mut response = TestRunResponse {
        lang: payload.lang.clone(),
        compiler_err: "".to_string(),
        stdout: "".to_string(),
        stderr: "".to_string(),
        status: TestRunStatus::Initial,
    };

    let task_id = Uuid::new_v4();
    let runner = get_runner(&payload.lang, &payload.source_code).unwrap();
    let source_file = format!("code{}.{}", task_id, payload.lang);
    let binary_file = format!("binary{}", task_id);

    runner
        .initialize(&source_file)
        .expect("Failed to initialize runner");

    let compile_task = runner.compile(&source_file, &binary_file);
    if compile_task.is_ok() {
        let output = runner.execute(&binary_file, &payload.stdin);
        match output {
            Ok(output) => {
                response.stdout = output;
                response.status = TestRunStatus::Executed;
            }
            Err(error) => {
                response.stderr = error;
                response.status = TestRunStatus::RuntimeError;
            }
        }
    } else {
        response.compiler_err = compile_task.err().unwrap();
        response.status = TestRunStatus::CompilerError;
    }

    runner
        .cleanup(&source_file, &binary_file)
        .expect("Failed to cleanup");

    (StatusCode::OK, Json(response))
}

async fn get_task_status(Path(task_id): Path<String>) -> (StatusCode, Json<TaskStatusResponse>) {
    println!("Fetching task status for task id: {task_id:?}");
    let uri = std::env::var("MONGO_URL").expect("MONGO_URL should be specified");
    let mongo_client = Client::with_uri_str(uri)
        .await
        .expect("Should connect to MongoDB");

    let coll: Collection<TaskStatus> = mongo_client.database("oj-data").collection("task_status");
    let filter = doc! { "task_id": task_id.clone() };
    let db_res = coll.find_one(filter).await;
    let mut task_response = TaskStatusResponse { result: None };
    match db_res {
        Ok(Some(result)) => {
            task_response.result = Some(TaskStatus {
                task_id,
                compiler_error_msg: result.compiler_error_msg,
                status: result.status,
                test_case_result: result.test_case_result,
                created_at: result.created_at,
            });
            (StatusCode::OK, Json(task_response))
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(task_response)),
        Err(err) => {
            eprintln!("Error occurred while fetching data: {err:?}");
            (StatusCode::NOT_FOUND, Json(task_response))
        }
    }
}

async fn execute_task(task_data: PublishMessagePayload) {
    let source_file = format!("code{}.{}", task_data.task_id, task_data.task_request.lang);
    let binary_file = format!("binary{}", task_data.task_id);

    let uri = std::env::var("MONGO_URL").expect("MONGO_URL should be specified");
    let mongo_client = Client::with_uri_str(uri)
        .await
        .expect("Failed to connect to MongoDB");

    let runner = get_runner(
        &task_data.task_request.lang,
        &task_data.task_request.source_code,
    )
    .unwrap();

    let mut results = Vec::new();
    let coll: Collection<TaskStatus> = mongo_client.database("oj-data").collection("task_status");
    let status = TaskStatus {
        task_id: task_data.task_id.to_string(),
        compiler_error_msg: String::new(),
        status: 1, //Pending
        test_case_result: Vec::new(),
        created_at: DateTime::now(),
    };
    let insert_result = coll.insert_one(status).await.unwrap();
    let filter = doc! {"_id": insert_result.inserted_id};

    runner
        .initialize(&source_file)
        .expect("Failed to initialize runner");

    let compile_task = runner.compile(&source_file, &binary_file);
    if compile_task.is_ok() {
        for test_case in task_data.task_request.test_cases.iter() {
            let output = runner.execute(&binary_file, &test_case.input);
            match output {
                Ok(output) => {
                    if output.trim() == test_case.expected_output.trim() {
                        results.push(TestCaseResult {
                            srno: test_case.srno,
                            status: TestCaseStatus::Passed,
                            stdout: output,
                            stderr: "".to_string(),
                        });
                    } else {
                        results.push(TestCaseResult {
                            srno: test_case.srno,
                            status: TestCaseStatus::Failed,
                            stdout: output,
                            stderr: "".to_string(),
                        });
                    }
                }
                Err(error) => {
                    results.push(TestCaseResult {
                        srno: test_case.srno,
                        status: TestCaseStatus::Error,
                        stdout: "".to_string(),
                        stderr: error.to_string(),
                    });
                }
            }
        }

        let bson_test_case_result = results
            .into_iter()
            .map(|result| {
                doc! {
                    "srno": result.srno,
                    "status": get_test_case_run_status(result.status),
                    "stdout": result.stdout,
                    "stderr": result.stderr,
                }
            })
            .collect::<Vec<_>>();
        println!("BSON Document: {bson_test_case_result:?}");

        let update_doc = doc! {
            "$set": doc! {
                "status": 2, //Completed
                "test_case_result": bson_test_case_result,
            }
        };
        coll.update_one(filter, update_doc)
            .await
            .expect("The status must be updated");
    } else {
        let update_doc = doc! {
            "$set": doc! {
                "compiler_error_msg": compile_task.err().unwrap(), //This is safe as we have already checked for ok
                "status": 2, //Completed
            }
        };
        coll.update_one(filter, update_doc)
            .await
            .expect("The status must be updated");
    }

    runner
        .cleanup(&source_file, &binary_file)
        .expect("Failed to cleanup");
}

async fn connect_to_amqp() -> Result<Connection, Box<dyn Error>> {
    let addr = std::env::var("AMQP_DEV_URL").unwrap_or_else(|_| "amqp://rabbitmq:5672/%2f".into());
    let mut retries = 0;
    loop {
        match Connection::connect(&addr, ConnectionProperties::default()).await {
            Ok(conn) => {
                println!("Connected to RabbitMQ!");
                return Ok(conn);
            }
            Err(err) => {
                if retries >= MAX_RETRIES {
                    panic!(
                        "Failed to connect to RabbitMQ after {} retries: {:?}",
                        retries, err
                    );
                }
                println!(
                    "Failed to connect to RabbitMQ, retrying in {} seconds...",
                    RETRY_INTERVAL
                );
                sleep(Duration::from_secs(RETRY_INTERVAL)).await;
                retries += 1;
            }
        }
    }
}
