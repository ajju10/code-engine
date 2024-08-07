use crate::runner::TestCaseStatus;

pub fn get_test_case_run_status(status: TestCaseStatus) -> String {
    match status {
        TestCaseStatus::Error => String::from("Error"),
        TestCaseStatus::Passed => String::from("Passed"),
        TestCaseStatus::Failed => String::from("Failed"),
    }
}
