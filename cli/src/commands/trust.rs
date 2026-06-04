use crate::commands::{AppContext, Command};
use macc_core::ops_motif::calculate_trust_summary;
use macc_core::Result;

pub struct TrustCommand {
    app: AppContext,
}

impl TrustCommand {
    pub fn new(app: AppContext) -> Self {
        Self { app }
    }
}

impl Command for TrustCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let config = self.app.canonical_config()?;
        let trust = calculate_trust_summary(&paths, &config);

        println!("====================================================");
        println!("                MACC TRUST CENTER                   ");
        println!("====================================================");
        println!("Trust State     : {:?}", trust.state);
        println!("Server Exposure : {}", trust.server_exposure);
        println!(
            "Local Only      : {}",
            if trust.local_only {
                "YES (offline mode)"
            } else {
                "NO (remote requests allowed)"
            }
        );
        println!(
            "Terminal Access : {}",
            if trust.terminal_enabled {
                "ENABLED (Caution)"
            } else {
                "DISABLED (Safe)"
            }
        );
        println!(
            "User-Level Files: {} modified outside project root",
            trust.user_level_writes
        );
        println!(
            "Backups Status  : {}",
            if trust.backups_ready {
                "READY (Restorables found)"
            } else {
                "MISSING (.macc/backups/ does not exist)"
            }
        );
        println!(
            "Catalog Pinned  : {}",
            if trust.catalog_pinned {
                "YES (Deterministic catalogs)"
            } else {
                "NO (Caution: Dynamic catalogs in use)"
            }
        );
        println!(
            "Secrets Redacted: {}",
            if trust.secrets_redacted {
                "YES (Redaction scanner active)"
            } else {
                "NO"
            }
        );
        println!("Audit Log File  : {}", trust.audit_log);
        println!("----------------------------------------------------");
        println!("Allowed Roots:");
        for root in &trust.allowed_roots {
            println!("  - {}", root);
        }
        println!("====================================================");

        Ok(())
    }
}
