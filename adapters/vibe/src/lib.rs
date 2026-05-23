mod adapter;
mod doctor;
pub mod emit;
pub mod error_normalizer;
mod map;

pub use adapter::VibeAdapter;

inventory::submit! {
    macc_core::tool::AdapterRegistration {
        factory: || std::sync::Arc::new(VibeAdapter)
    }
}

inventory::submit! {
    macc_core::coordinator::error_normalizer::NormalizerRegistration {
        tool_id: "vibe",
        factory: || Box::new(crate::error_normalizer::VibeErrorNormalizer),
    }
}
