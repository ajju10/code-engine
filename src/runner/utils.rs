use futures_util::StreamExt;
use lapin::options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use lapin::{Connection, ConnectionProperties};
use mongodb::bson::{doc, DateTime};
use mongodb::{Client, Collection};
use std::error::Error;
use std::time::Duration;
use tokio::time::sleep;

use crate::runner::{
    get_runner, PublishMessagePayload, TaskStatus, TestCaseResult, TestCaseStatus,
};

pub const SUBMISSION_QUEUE: &str = "SUBMISSION_QUEUE";
const RETRY_INTERVAL: u64 = 5; // seconds
const MAX_RETRIES: u32 = 12; // retry for 1 minute

fn get_test_case_run_status(status: TestCaseStatus) -> String {
    match status {
        TestCaseStatus::Error => String::from("Error"),
        TestCaseStatus::Passed => String::from("Passed"),
        TestCaseStatus::Failed => String::from("Failed"),
    }
}

pub async fn listen_consumer(conn: Connection) {
    let channel = conn.create_channel().await.expect("create channel error");

    channel
        .queue_declare(
            SUBMISSION_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare queue declare error");

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
                Err(err) => println!("Error occurred during task {:?}", err.to_string()),
            }
            delivery
                .ack(BasicAckOptions::default())
                .await
                .expect("Failed to acknowledge the message");
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
        .await
        .expect("Failed to initialize runner");

    let compile_task = runner.compile(&source_file, &binary_file).await;
    if compile_task.is_ok() {
        for test_case in task_data.task_request.test_cases.iter() {
            let output = runner.execute(&binary_file, &test_case.input).await;
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
                "status": 2, // Completed
                "test_case_result": bson_test_case_result,
            }
        };
        coll.update_one(filter, update_doc)
            .await
            .expect("The status must be updated");
    } else {
        let update_doc = doc! {
            "$set": doc! {
                "compiler_error_msg": compile_task.err().unwrap(), // This is safe as we have already checked for ok
                "status": 2, // Completed
            }
        };
        coll.update_one(filter, update_doc)
            .await
            .expect("The status must be updated");
    }

    runner
        .cleanup(&source_file, &binary_file)
        .await
        .expect("Failed to cleanup");
}

pub async fn connect_to_amqp() -> Result<Connection, Box<dyn Error>> {
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
