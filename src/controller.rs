use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use lapin::options::{BasicPublishOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Connection, ConnectionProperties};
use mongodb::bson::doc;
use mongodb::{Client, Collection};
use uuid::Uuid;

use crate::runner::{
    get_runner, PublishMessagePayload, TaskRequest, TaskResponse, TaskStatus, TaskStatusResponse,
    TestRunRequest, TestRunResponse, TestRunStatus, SUBMISSION_QUEUE,
};

pub async fn execute_task_handler(
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

pub async fn test_run_handler(
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
        .await
        .expect("Failed to initialize runner");

    let compile_task = runner.compile(&source_file, &binary_file).await;
    if compile_task.is_ok() {
        let output = runner.execute(&binary_file, &payload.stdin).await;
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
        .await
        .expect("Failed to cleanup");

    (StatusCode::OK, Json(response))
}

pub async fn get_task_status(
    Path(task_id): Path<String>,
) -> (StatusCode, Json<TaskStatusResponse>) {
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
