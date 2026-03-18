mod adapter;
mod doctor;
mod emit;
pub mod error_normalizer;
mod map;
mod user_mcp_merge;

pub use adapter::ClaudeAdapter;

inventory::submit! {
    macc_core::tool::AdapterRegistration {
        factory: || std::sync::Arc::new(ClaudeAdapter)
    }
}
