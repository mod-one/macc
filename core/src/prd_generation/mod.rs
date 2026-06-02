pub mod audit;
pub mod metadata;
pub mod promotion;
pub mod prompt_builder;
pub mod request;
pub mod target_dir;
pub mod validation;

pub use audit::{PrdAuditRequest, PrdAuditResult};
pub use metadata::{GenerationRunMetadata, PRD_GENERATION_DEFAULT_TARGET_DIR};
pub use promotion::{PromoteOptions, PromoteResult};
pub use request::{ModelRoutingMode, ModelSelection, PrdGenerateRequest};
pub use target_dir::resolve_target_dir;
pub use validation::{validate_prd_file, ValidationResult};

pub const PRD_GENERATION_INTERNAL_ROLE: &str = "prd-planner";
pub const PRD_GENERATION_INTERNAL_SKILL: &str = "macc-prd-planner";
