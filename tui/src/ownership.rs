use macc_core::process_ownership::{OwnershipRecord, TakeoverRequest};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::time::Instant;

use crate::ui::{centered_rect, truncate_middle};

pub struct TuiOwnershipState {
    pub record: Option<OwnershipRecord>,
    pub is_owner: bool,
    pub pending_incoming_request: Option<TakeoverRequest>,
    pub dismissed_request_id: Option<String>,
    pub last_refresh: Instant,
}

impl TuiOwnershipState {
    pub fn new() -> Self {
        Self {
            record: None,
            is_owner: false,
            pending_incoming_request: None,
            dismissed_request_id: None,
            last_refresh: Instant::now(),
        }
    }

    pub fn is_owner(&self, client_id: &str) -> bool {
        self.record
            .as_ref()
            .and_then(|r| r.owner.as_ref())
            .map(|o| o.client_id == client_id)
            .unwrap_or(false)
    }
}

impl Default for TuiOwnershipState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct OwnershipBannerProps {
    pub owner_label: String,
    pub viewer_count: usize,
    pub is_owner: bool,
    pub has_pending_request: bool,
}

pub fn render_ownership_banner(f: &mut Frame, area: Rect, props: &OwnershipBannerProps) {
    let max_owner_len = area.width.saturating_sub(40) as usize;
    let owner_display = truncate_middle(&props.owner_label, max_owner_len.max(8));

    let status_label = if props.is_owner { "owner" } else { "viewer" };
    let status_color = if props.is_owner {
        Color::Green
    } else {
        Color::Yellow
    };

    let takeover_hint = if !props.is_owner && props.has_pending_request {
        " [pending takeover]"
    } else if !props.is_owner {
        " [T: request ownership]"
    } else {
        ""
    };

    let spans = vec![
        Span::styled(
            format!("[{}]", status_label),
            Style::default().fg(status_color),
        ),
        Span::raw("  owner: "),
        Span::raw(owner_display),
        Span::raw(format!("  viewers: {}", props.viewer_count)),
        Span::styled(takeover_hint.to_string(), Style::default().fg(Color::Cyan)),
    ];

    let banner = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::ALL).title("Ownership"));
    f.render_widget(banner, area);
}

pub fn render_takeover_modal(f: &mut Frame, area: Rect, request: &TakeoverRequest) {
    let modal_area = centered_rect(60, 40, area);
    f.render_widget(Clear, modal_area);

    let text = format!(
        "Takeover requested by: {}\nKind: {:?}\nAt: {}\n\nPress a to accept, r to reject, Esc to dismiss.",
        request.requester.client_id, request.requester.kind, request.requested_at,
    );

    let modal = Paragraph::new(text)
        .block(
            Block::default()
                .title("Ownership Takeover Request")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(modal, modal_area);
}
