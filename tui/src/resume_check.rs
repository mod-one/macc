//! Surface the same durable observation in task detail and in the stopped-run summary.
use macc_core::coordinator::resume_check::{CheckRef, Status};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
pub(crate) fn line(check: &CheckRef, good: Color, warn: Color, bad: Color) -> Line<'static> {
    let (status, detail, color) = match &check.last {
        Some(result) => (
            result.status.label(),
            result.detail.as_str(),
            match result.status {
                Status::Ready => good,
                Status::Waiting => warn,
                Status::Error => bad,
            },
        ),
        None => (check.category.label(), check.criterion.as_str(), warn),
    };
    Line::from(vec![
        Span::styled(
            format!("Resume · {status} · {}  ", check.phase),
            Style::default().fg(color),
        ),
        Span::raw(detail.to_owned()),
    ])
}
