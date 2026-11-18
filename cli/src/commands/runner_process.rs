//! One isolated process per provider invocation. No project state or saved PID
//! grants authority to signal a process. Linux adoption also covers setsid and
//! double-forked tools which cannot be contained by a process group alone.
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
mod unix;

pub fn run(command: &[OsString], completion_file: &Path, owner_pid: Option<u32>) -> ! {
    match supervise(command, owner_pid).and_then(|code| {
        use std::io::Write;
        // Absence of this receipt means cleanup was NOT confirmed, including
        // supervisor crashes/SIGKILL. Never reuse an older receipt.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(completion_file)?
            .write_all(b"quiescent-v1\n")?;
        Ok(code)
    }) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("MACC_RUNNER_ERROR: E901\nProvider process supervision failed: {error}; work retained");
            std::process::exit(125);
        }
    }
}

fn supervise(command: &[OsString], owner_pid: Option<u32>) -> io::Result<i32> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing provider command"))?;
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(unix)]
    return unix::supervise(command, owner_pid);
    #[cfg(not(unix))]
    {
        let _ = owner_pid;
        Ok(command.status()?.code().unwrap_or(1))
    }
}
