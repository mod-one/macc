//! Shared launch decision for all clients; unattended invocations never wait on stdin.
use macc_core::{MaccError, Result};
use std::io::{IsTerminal, Write};
pub fn choose(explicit: bool, disabled: bool) -> Result<bool> {
    if explicit {
        return Ok(true);
    }
    if disabled {
        return Ok(false);
    }
    if std::env::var("MACC_INTERNAL_INVOCATION").as_deref() == Ok("1") {
        return Ok(false);
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        println!("Supervisor enabled for unattended run (use --no-supervisor to disable).");
        return Ok(true);
    }
    print!("Also start the supervisor to diagnose, repair and resume this run? [Y/n] ");
    std::io::stdout().flush().map_err(|source| MaccError::Io {
        path: "stdout".into(),
        action: "prompt for supervisor".into(),
        source,
    })?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|source| MaccError::Io {
            path: "stdin".into(),
            action: "read supervisor choice".into(),
            source,
        })?;
    Ok(accepts(&answer))
}
fn accepts(answer: &str) -> bool {
    !matches!(answer.trim().to_ascii_lowercase().as_str(), "n" | "no")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_yes_and_explicit_no() {
        assert!(accepts("\n"));
        assert!(accepts("yes"));
        assert!(!accepts("n"));
        assert!(!accepts("NO"));
    }
    #[test]
    fn flags_skip_prompt() {
        assert!(choose(true, false).unwrap());
        assert!(!choose(false, true).unwrap());
    }
}
