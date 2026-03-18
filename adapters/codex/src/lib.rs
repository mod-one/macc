mod adapter;
mod doctor;
mod emit;
pub mod error_normalizer;
mod map;

pub use adapter::CodexAdapter;

inventory::submit! {
    macc_core::tool::AdapterRegistration {
        factory: || std::sync::Arc::new(CodexAdapter)
    }
}
