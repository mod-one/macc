//! Keep provider argument transport bounded without dropping diagnostic evidence.
use std::path::Path;

pub fn argument(prompt: &str, complete_instructions: &Path) -> String {
    if prompt.len() <= 32 * 1024 {
        return prompt.into();
    }
    format!(
        "Read the complete supervisor instructions from this UTF-8 file and carry out that phase exactly: {:?}. The file contains the full task, constraints and incident evidence. Read it before taking any action; do not skip or truncate its requirements. Write the response to the location specified in that file.",
        complete_instructions
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_instructions_use_a_file_reference_and_small_prompts_stay_compatible() {
        let path = Path::new("/tmp/project with spaces/diagnosis-prompt.txt");
        let large = "mandatory evidence\n".repeat(16000);
        let arg = argument(&large, path);
        assert!(large.len() > 131072);
        assert!(arg.len() < 1024);
        assert!(arg.contains("project with spaces/diagnosis-prompt.txt"));
        assert_eq!(argument("small task", path), "small task");
    }
}
