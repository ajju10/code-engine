use mongodb::bson::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runner::cpp_runner::CppRunner;

mod cpp_runner;

#[derive(Serialize, Deserialize)]
pub enum TestCaseStatus {
    Error,
    Passed,
    Failed,
}

#[derive(Serialize, Deserialize)]
pub struct TestCaseResult {
    pub srno: usize,
    pub status: TestCaseStatus,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Serialize, Deserialize)]
pub struct TaskStatus {
    pub task_id: String,
    pub compiler_error_msg: String,
    pub status: u8,
    pub stdout: Vec<(usize, String)>,
    pub stderr: Vec<(usize, String)>,
    pub created_at: DateTime,
}

#[derive(Serialize, Deserialize)]
pub struct TaskStatusResponse {
    pub result: Option<TaskStatus>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestCase {
    pub srno: usize,
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
pub struct PublishMessagePayload {
    pub task_id: Uuid,
    pub task_request: TaskRequest,
}

pub trait Runner {
    fn initialize(&self, filename: &str) -> std::io::Result<()>;
    fn compile(&self, filename: &str, binary_file: &str) -> Result<String, String>;
    fn execute(&self, binary_file: &str, input: &str) -> Result<String, String>;
    fn cleanup(&self, filename: &str, binary_file: &str) -> std::io::Result<()>;
}

pub fn get_runner(lang: &str, source_code: &str) -> Result<Box<dyn Runner + Send + Sync>, String> {
    match lang {
        "cpp" => Ok(Box::new(CppRunner::new(source_code))),
        _ => Err(format!("Unsupported language: {}", lang)),
    }
}
