use macc_core::tool::{ToolDescriptor, ToolRegistry, ToolSpecLoader};
use std::path::PathBuf;

pub fn default_registry() -> ToolRegistry {
    // ADAPTER REGISTRATION STRATEGY (Self-Registration via inventory):
    //
    // To add a new adapter:
    // 1. Add the adapter crate as a dependency in registry/Cargo.toml
    // 2. Add a reference to the adapter's type here to force the linker to include the crate.
    // 3. The adapter itself uses inventory::submit! to register its factory.
    //
    // This approach minimizes churn because it avoids a central switch/match statement,
    // and keeps the adapter's registration logic inside the adapter crate.

    // Force linking of adapter crates so they can register themselves via inventory
    #[cfg(feature = "claude")]
    let _ = macc_adapter_claude::ClaudeAdapter;
    #[cfg(feature = "codex")]
    let _ = macc_adapter_codex::CodexAdapter;
    #[cfg(feature = "gemini")]
    let _ = macc_adapter_gemini::GeminiAdapter;
    #[cfg(feature = "vibe")]
    let _ = macc_adapter_vibe::VibeAdapter;

    let _ = macc_core::tool::AGENTS_MD_ORCHESTRATOR
        .get_or_init(|| macc_adapter_shared::render::agents_md::orchestrate_agents_md);

    ToolRegistry::from_inventory()
}

pub fn tool_descriptors() -> Vec<ToolDescriptor> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Resolve override paths; built-in ToolSpecs are embedded in the binary.
    let search_paths = if let Ok(paths) = macc_core::find_project_root(&current_dir) {
        ToolSpecLoader::default_search_paths(&paths.root)
    } else {
        ToolSpecLoader::default_search_paths(&current_dir)
    };

    let loader = ToolSpecLoader::new(search_paths);
    let (specs, _diags) = loader.load_all_with_embedded();

    let mut descriptors: Vec<_> = specs.into_iter().map(|s| s.to_descriptor()).collect();

    // Sort by title for deterministic UI/test behavior
    descriptors.sort_by(|a, b| a.title.cmp(&b.title));

    if descriptors.is_empty() {
        eprintln!(
            "Warning: No ToolSpecs resolved (embedded + ~/.config/macc/tools.d + <project>/.macc/tools.d)."
        );
    }

    descriptors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_enumeration() {
        let registry = default_registry();
        let ids = registry.list_ids();

        // Should contain all our enabled adapters
        #[cfg(feature = "claude")]
        assert!(ids.contains(&"claude".to_string()));
        #[cfg(feature = "codex")]
        assert!(ids.contains(&"codex".to_string()));
        #[cfg(feature = "gemini")]
        assert!(ids.contains(&"gemini".to_string()));
        #[cfg(feature = "vibe")]
        assert!(ids.contains(&"vibe".to_string()));
        assert!(ids.contains(&"test".to_string()));

        let mut expected_len = 1; // "test" is built-in core mock
        if cfg!(feature = "claude") {
            expected_len += 1;
        }
        if cfg!(feature = "codex") {
            expected_len += 1;
        }
        if cfg!(feature = "gemini") {
            expected_len += 1;
        }
        if cfg!(feature = "vibe") {
            expected_len += 1;
        }

        // IDs should be sorted (list_ids handles this)
        assert_eq!(ids.len(), expected_len);
    }

    #[test]
    fn test_tool_descriptors_not_empty() {
        let descriptors = tool_descriptors();
        // Embedded ToolSpecs ensure this works even outside the repo tree.
        assert!(
            !descriptors.is_empty(),
            "Should have found embedded tool descriptors"
        );

        // Check for known tools
        assert!(descriptors.iter().any(|d| d.id == "claude"));
        assert!(descriptors.iter().any(|d| d.id == "gemini"));
        assert!(descriptors.iter().any(|d| d.id == "codex"));
        assert!(descriptors.iter().any(|d| d.id == "vibe"));
    }

    #[test]
    fn test_tool_descriptors_sorting() {
        let descriptors = tool_descriptors();
        if descriptors.len() > 1 {
            for i in 0..descriptors.len() - 1 {
                assert!(
                    descriptors[i].title <= descriptors[i + 1].title,
                    "Descriptors should be sorted by title: {} vs {}",
                    descriptors[i].title,
                    descriptors[i + 1].title
                );
            }
        }
    }
}
