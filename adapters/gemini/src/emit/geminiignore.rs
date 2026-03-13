use macc_adapter_shared::render::format::ensure_trailing_newline;

pub fn render_geminiignore() -> String {
    let content = r###"# dependencies / builds
node_modules/
dist/
build/
.next/
.out/

# VCS
.git/
.worktrees/

# secrets
.env
.env.*
*.pem
*.key

# MACC internal backups
.macc/

# Ignore noisy/generated Gemini files but keep agent context readable.
.gemini/tmp/
.gemini/history/
.gemini/checkpoints/
.gemini/telemetry.log

# Force allow Gemini context files, even if other ignore layers are broad.
!.gemini/
!GEMINI.md
!**/.gemini/
!**/GEMINI.md
!**/.gemini/skills/
!**/.gemini/skills/**
!**/.gemini/commands/
!**/.gemini/commands/**
!**/.gemini/settings.json
!**/.gemini/styleguide.md
"###;

    ensure_trailing_newline(content.to_string())
}
