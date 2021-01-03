use anyhow::Result;
use std::process::{Command, ExitStatus, Output, Stdio};

use crate::env::ProcessEnv;

fn run_with_output<F>(env: ProcessEnv, command: Vec<String>, stdio: F) -> Result<Output>
where
    F: Fn() -> Stdio,
{
    let env = env.into_env();

    let child = Command::new(&command[0])
        .args(command.iter().skip(1))
        .env_clear()
        .envs(&env)
        .stdout(stdio())
        .stdin(stdio())
        .stderr(stdio())
        .spawn()?;

    let output = child.wait_with_output()?;

    Ok(output)
}

pub fn run_in_env(env: ProcessEnv, command: Vec<String>) -> Result<ExitStatus> {
    Ok(run_with_output(env, command, Stdio::inherit)?.status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_process_in_correct_env() {
        let env = ProcessEnv::fresh(
            vec![("ENV".to_string(), "A".to_string())],
            vec![
                ("KV".to_string(), "B".to_string()),
                ("M".to_string(), "C".to_string()),
            ],
            vec!["M".to_string()],
        );

        let output = run_with_output(env, vec!["env".to_string()], Stdio::piped).unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!("ENV=A\nKV=B\n", &stdout);
    }

    #[test]
    fn fails_expectedly() {
        let env = || {
            ProcessEnv::fresh(
                vec![("ENV".to_string(), "A".to_string())],
                vec![("KV".to_string(), "B".to_string())],
                vec![],
            )
        };

        assert!(
            run_with_output(env(), vec!["this-does-not-exist".to_string()], Stdio::piped).is_err()
        );

        let failed_exec = run_with_output(
            env(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "exit 10".to_string(),
            ],
            Stdio::piped,
        )
        .unwrap();
        assert!(!failed_exec.status.success());
        assert_eq!(10, failed_exec.status.code().unwrap());
    }
}
