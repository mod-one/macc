//! Rendering of ToolSpec performer argument templates for direct tool calls.
use crate::{MaccError, Result};

/// Replace `{model}` and `{effort}` in performer arguments. An argument whose
/// value is empty is dropped together with the flag before it, so the tool
/// falls back to its own default. Any other placeholder is an error: a
/// literal `{...}` must never reach the tool.
pub fn render_performer_args(args: &[String], model: &str, effort: &str) -> Result<Vec<String>> {
    let mut rendered = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if i + 1 < args.len()
            && ((args[i + 1].contains("{model}") && model.is_empty())
                || (args[i + 1].contains("{effort}") && effort.is_empty()))
        {
            i += 2;
            continue;
        }
        let value = a.replace("{model}", model).replace("{effort}", effort);
        if value.contains('{') && value.contains('}') {
            return Err(MaccError::Validation(format!(
                "Unresolved tool argument: {value}"
            )));
        }
        rendered.push(value);
        i += 1;
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn model_is_substituted_or_dropped_with_its_flag() {
        let template = args(&["--model", "{model}", "--output-format", "text"]);
        assert_eq!(
            render_performer_args(&template, "sonnet", "").unwrap(),
            args(&["--model", "sonnet", "--output-format", "text"])
        );
        assert_eq!(
            render_performer_args(&template, "", "").unwrap(),
            args(&["--output-format", "text"])
        );
    }

    #[test]
    fn unknown_placeholder_is_refused() {
        let template = args(&["-r", "{session_id}"]);
        assert!(render_performer_args(&template, "sonnet", "high").is_err());
    }
}
