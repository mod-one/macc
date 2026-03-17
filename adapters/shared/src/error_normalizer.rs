//! Re-exports of the canonical error model for adapter convenience.
//!
//! Adapters can import from `macc_adapter_shared::error_normalizer` instead of
//! reaching into `macc_core::coordinator::error_normalizer` directly.

pub use macc_core::coordinator::error_normalizer::{
    canonical_to_error_code, is_retryable, is_user_action_required, truncate_raw_message,
    CanonicalClass, ErrorNormalizer, ToolError,
};
