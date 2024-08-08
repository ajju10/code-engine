use async_trait::async_trait;
use mongodb::bson::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runner::cpp_runner::CppRunner;

mod cpp_runner;
mod utils;

pub use utils::{connect_to_amqp, listen_consumer, SUBMISSION_QUEUE};

#[derive(Serialize, Deserialize, Debug)]
pub enum TestCaseStatus {
    Error,
    Passed,
    Failed,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TestRunStatus {
    Initial,
    CompilerError,
    RuntimeError,
    Executed,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TestCaseResult {
    pub srno: i32,
    pub status: TestCaseStatus,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StdoutEntry {
    pub srno: i32,
    pub stdout: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StderrEntry {
    pub srno: i32,
    pub stderr: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TaskStatus {
    pub task_id: String,
    pub compiler_error_msg: String,
    pub status: u8,
    pub test_case_result: Vec<TestCaseResult>,
    pub created_at: DateTime,
}

#[derive(Serialize, Deserialize)]
pub struct TaskStatusResponse {
    pub result: Option<TaskStatus>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestCase {
    pub srno: i32,
    pub input: String,
    pub expected_output: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskRequest {
    pub lang: String,
    pub source_code: String,
    pub test_cases: Vec<TestCase>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskResponse {
    pub task_id: Uuid,
    pub status: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TestRunRequest {
    pub lang: String,
    pub source_code: String,
    pub stdin: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TestRunResponse {
    pub lang: String,
    pub compiler_err: String,
    pub stdout: String,
    pub stderr: String,
    pub status: TestRunStatus,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PublishMessagePayload {
    pub task_id: Uuid,
    pub task_request: TaskRequest,
}

#[async_trait]
pub trait Runner {
    async fn initialize(&self, filename: &str) -> std::io::Result<()>;
    async fn compile(&self, filename: &str, binary_file: &str) -> Result<String, String>;
    async fn execute(&self, binary_file: &str, input: &str) -> Result<String, String>;
    async fn cleanup(&self, filename: &str, binary_file: &str) -> std::io::Result<()>;
}

pub fn get_runner(lang: &str, source_code: &str) -> Result<Box<dyn Runner + Send + Sync>, String> {
    match lang {
        "cpp" => Ok(Box::new(CppRunner::new(source_code))),
        _ => Err(format!("Unsupported language: {}", lang)),
    }
}
