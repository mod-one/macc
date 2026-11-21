//! Provider-neutral projection of assistant protocol and native runner summaries.
mod provider;
pub use provider::provider_output;

pub fn valid_explanation(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && ![
            "<reason>",
            "<condition>",
            "<evidence>",
            "<summary>",
            "<explanation>",
            "<result>",
            "<kind>",
            "<status>",
        ]
        .iter()
        .any(|p| value.to_ascii_lowercase().contains(p))
}
/// The shell transport records this sentinel when the performer omits its explanation.
pub(crate) fn missing_precondition_explanation(value: Option<&str>) -> bool {
    value.map(str::trim).is_none_or(|s| s.is_empty() || s == "(no explanation provided: the tool reported precondition_unmet without the required MACC_TASK_RESULT_EXP line)")
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Channel {
    #[default]
    Provider,
    Assistant,
    Transcript,
    User,
}
#[derive(Default)]
struct State {
    prompt: bool,
    channel: Channel,
    fence: Option<(char, usize)>,
    epoch: usize,
}
impl State {
    fn line(&mut self, line: &str) -> bool {
        if line.starts_with("MACC_PROMPT_BEGIN:")
            && matches!(self.channel, Channel::Provider | Channel::User)
            && self.fence.is_none()
        {
            self.epoch += 1;
            self.prompt = true;
            self.fence = None;
            return false;
        }
        if self.prompt && line.starts_with("MACC_PROMPT_END:") {
            self.prompt = false;
            self.channel = Channel::Provider;
            self.fence = None;
            return false;
        }
        if self.prompt {
            return false;
        }
        // Channel boundaries must be handled before Markdown. Tool output is
        // excluded without touching the assistant's Markdown state.
        let channel = match line {
            "exec" | "tool" | "system" | "developer" => Some(Channel::Transcript),
            "user" => Some(Channel::User),
            "codex" | "assistant" | "final" => Some(Channel::Assistant), // macc:allow-tool-name: emitted transcript role token, never adapter routing.
            _ => None,
        };
        if let Some(channel) = channel.filter(|_| {
            matches!(self.channel, Channel::Transcript | Channel::User) || self.fence.is_none()
        }) {
            if channel == Channel::Assistant {
                self.epoch += 1;
            }
            self.channel = channel;
            self.fence = None;
            return false;
        }
        if matches!(self.channel, Channel::Transcript | Channel::User) {
            return false;
        }
        let trimmed = line.trim_start();
        if let Some(ch) = trimmed.chars().next().filter(|c| matches!(c, '`' | '~')) {
            let len = trimmed.chars().take_while(|c| *c == ch).count();
            if len >= 3 {
                match self.fence {
                    None => self.fence = Some((ch, len)),
                    Some((kind, size))
                        if kind == ch && len >= size && trimmed[len..].trim().is_empty() =>
                    {
                        self.fence = None
                    }
                    _ => {}
                }
                return false;
            }
        }
        self.fence.is_none() && !line.trim_start().starts_with('>')
    }
}
/// A captured command can print an entire earlier transcript, including role
/// markers and unmatched Markdown fences. Strip its payload when the adapter
/// supplies both a command completion header and the native post-tool footer.
/// Streams without that framing use the ordinary role parser.
fn transcript_lines(raw: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut state = State::default();
    let mut command = false;
    let mut payload = None;
    for line in raw.lines() {
        state.line(line);
        if line == "exec" && !state.prompt && state.channel == Channel::Transcript {
            command = true;
        } else if command && line.starts_with(" succeeded in ") && line.ends_with(':') {
            payload.get_or_insert(lines.len());
            command = false;
        } else if line == "hook: PostToolUse Completed"
            && lines.last() == Some(&"hook: PostToolUse")
        {
            if let Some(start) = payload.take() {
                lines.truncate(start);
                lines.push("exec");
                state = State {
                    channel: Channel::Transcript,
                    ..Default::default()
                };
            }
            command = false;
        }
        lines.push(line);
    }
    lines
}

/// Prompt echoes, tool transcripts and fenced examples cannot provide protocol.
pub fn projection(raw: &str) -> String {
    project(raw, false)
}

/// Decision validation uses the final assistant response in this invocation.
pub(super) fn final_projection(raw: &str) -> String {
    project(raw, true)
}

fn project(raw: &str, final_only: bool) -> String {
    let mut state = State::default();
    let mut out = String::new();
    for line in transcript_lines(raw) {
        let epoch = state.epoch;
        let accepted = state.line(line);
        if final_only && epoch != state.epoch {
            out.clear();
        }
        if !accepted {
            continue;
        }
        let trimmed = line.trim();
        if (trimmed.starts_with("MACC_")
            || trimmed.starts_with("REVIEW_VERDICT:")
            || trimmed.starts_with("- Explanation:"))
            && (!valid_explanation(trimmed) || line.starts_with(char::is_whitespace))
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
/// A terminal result cannot borrow an explanation from an older result block.
pub fn completion_projection(raw: &str) -> String {
    let mut state = State::default();
    let mut fields = Vec::new();
    let mut boundary = false;
    for line in transcript_lines(raw) {
        let epoch = state.epoch;
        let accepted = state.line(line);
        if epoch != state.epoch {
            fields.clear();
            boundary = false;
        }
        if !accepted || line.starts_with(char::is_whitespace) || !valid_explanation(line) {
            continue;
        }
        if line.starts_with("MACC_TASK_")
            || ["- Result kind:", "- Explanation:", "- Unmet precondition:"]
                .iter()
                .any(|p| line.starts_with(p))
        {
            if boundary {
                fields.clear();
                boundary = false;
            }
            fields.push(line);
        } else if line.strip_prefix("MACC_DECISION_ACK:").is_some_and(|ack| {
            ack.trim().strip_prefix("sha256:").is_some_and(|digest| {
                digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
            })
        }) {
            // ACK validation belongs to decision control. A protocol sibling in
            // the same response does not terminate the surrounding task fields.
            continue;
        } else if !fields.is_empty() {
            boundary = true;
        }
    }
    let markers: Vec<_> = fields
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.starts_with("MACC_TASK_RESULT:").then_some(i))
        .collect();
    if markers.len() > 1 {
        fields.drain(..=markers[markers.len() - 2]);
    }
    fields.join("\n")
}
#[cfg(test)]
mod tests;
