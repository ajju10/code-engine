use std::fs;
use std::io::Write;
use std::os::unix::prelude::ExitStatusExt;
use std::path;
use std::process::{Command, Stdio};

use crate::runner::Runner;

pub struct CppRunner {
    source_code: String,
}

impl CppRunner {
    pub fn new(source_code: &str) -> CppRunner {
        CppRunner {
            source_code: source_code.to_string(),
        }
    }
}

impl Runner for CppRunner {
    fn initialize(&self, filename: &str) -> std::io::Result<()> {
        println!("Step 1 => Starting initialization phase");
        let mut file = fs::File::create(filename)?;
        file.write_all(self.source_code.as_bytes())?;
        Ok(())
    }

    fn compile(&self, filename: &str, binary_file: &str) -> Result<String, String> {
        println!("Step 2 => Starting compilation phase");
        let output = Command::new("g++")
            .arg(filename)
            .arg("-o")
            .arg(binary_file)
            .output()
            .map_err(|e| format!("Failed to execute g++: {:?}", e))?;

        if output.status.success() {
            println!(
                "Compilation successful with code: {}",
                output.status.code().unwrap()
            );
            Ok("Compilation successful".into())
        } else {
            let err = String::from_utf8(output.stderr).unwrap();
            eprintln!("Compilation failed with error: {err}");
            Err(err)
        }
    }

    fn execute(&self, binary_file: &str, input: &str) -> Result<String, String> {
        println!("Step 3 => Starting execution phase");
        let mut process = Command::new(format!("./{}", binary_file))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to execute binary: {}", e))?;

        if let Some(mut stdin) = process.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .map_err(|e| format!("Failed to write to stdin: {}", e))?;
        }

        let output = process
            .wait_with_output()
            .map_err(|e| format!("Failed to wait on child process: {}", e))?;

        if output.status.success() {
            let stdout = String::from_utf8(output.stdout)
                .map_err(|e| format!("Cannot get stdout data: {}", e))?;
            println!("Program successfully executed: {stdout}");
            Ok(stdout)
        } else if !output.stderr.is_empty() {
            let stderr = String::from_utf8(output.stderr)
                .map_err(|e| format!("Cannot get stderr data: {}", e))?;
            eprintln!("Error in program execution: {stderr}");
            Err(stderr)
        } else {
            if let Some(signal) = output.status.signal() {
                let signal_name = match signal {
                    libc::SIGFPE => "SIGFPE",
                    libc::SIGSEGV => "SIGSEGV",
                    libc::SIGILL => "SIGILL",
                    libc::SIGABRT => "SIGABRT",
                    libc::SIGBUS => "SIGBUS",
                    _ => "Unknown Signal",
                };
                eprintln!("Program terminated with signal: {signal_name}");
                Err(format!("Program terminated with signal: {signal_name}"))
            } else {
                eprintln!("Program terminated with code: {}", output.status);
                Err(format!("Program terminated with code: {}", output.status))
            }
        }
    }

    fn cleanup(&self, filename: &str, binary_file: &str) -> std::io::Result<()> {
        println!("Step 4 => Starting cleanup phase");
        if path::Path::new(filename).exists() {
            fs::remove_file(filename)?;
        }
        if path::Path::new(binary_file).exists() {
            fs::remove_file(binary_file)?;
        }
        Ok(())
    }
}
