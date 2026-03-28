use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::profile::{parse_sections, ProfileManager};
use macc_core::Result;

pub enum ConfigAction<'a> {
    Save {
        name: &'a str,
        description: Option<&'a str>,
        only: Option<&'a str>,
    },
    Restore {
        name: &'a str,
        only: Option<&'a str>,
    },
    List,
    Delete {
        name: &'a str,
    },
}

pub struct ConfigCommand<'a> {
    app: AppContext,
    action: ConfigAction<'a>,
}

impl<'a> ConfigCommand<'a> {
    pub fn new(app: AppContext, action: ConfigAction<'a>) -> Self {
        Self { app, action }
    }
}

impl<'a> Command for ConfigCommand<'a> {
    fn run(&self) -> Result<()> {
        let mgr = ProfileManager::new()?;

        match &self.action {
            ConfigAction::Save {
                name,
                description,
                only,
            } => {
                let config = self.app.canonical_config()?;
                let sections = match only {
                    Some(csv) => Some(parse_sections(csv)?),
                    None => None,
                };
                mgr.save(name, *description, &config, sections.as_deref())?;
                println!("Profile '{}' saved.", name);
                Ok(())
            }
            ConfigAction::Restore { name, only } => {
                let paths = self.app.project_paths()?;
                let config = self.app.canonical_config()?;
                let sections = match only {
                    Some(csv) => Some(parse_sections(csv)?),
                    None => None,
                };
                let merged = mgr.restore(name, &config, sections.as_deref())?;
                let yaml = merged.to_yaml().map_err(|e| macc_core::MaccError::Config {
                    path: name.to_string(),
                    source: e,
                })?;
                std::fs::write(&paths.config_path, yaml.as_bytes()).map_err(|e| {
                    macc_core::MaccError::Io {
                        path: paths.config_path.to_string_lossy().into(),
                        action: "write restored config".into(),
                        source: e,
                    }
                })?;
                println!("Profile '{}' restored to {}.", name, paths.config_path.display());
                Ok(())
            }
            ConfigAction::List => {
                let profiles = mgr.list()?;
                if profiles.is_empty() {
                    println!("No saved profiles.");
                } else {
                    println!("{:<20} {:<24} {}", "NAME", "CREATED", "DESCRIPTION");
                    println!("{:-<20} {:-<24} {:-<30}", "", "", "");
                    for p in &profiles {
                        let created = p.created_at.as_deref().unwrap_or("-");
                        let desc = p.description.as_deref().unwrap_or("");
                        println!("{:<20} {:<24} {}", p.name, created, desc);
                    }
                }
                Ok(())
            }
            ConfigAction::Delete { name } => {
                mgr.delete(name)?;
                println!("Profile '{}' deleted.", name);
                Ok(())
            }
        }
    }
}
