//! Guided operator transfer without raw action JSON or implicit file overwrites.
use super::*;
use macc_core::o2::memory::{
    self,
    transfer::{ExportRequest, Format, ImportRequest},
};
use std::{
    io::{Read, Write},
    path::PathBuf,
};
#[derive(Default)]
pub(super) struct Transfer {
    access: Value,
    output: Option<PathBuf>,
}
impl Panel {
    pub(super) fn transfer_key(&mut self, root: &Path, key: KeyCode) -> Result<bool> {
        if self.tab != 0 {
            return Ok(false);
        }
        match key {
            KeyCode::Char('E') => {
                self.start(
                    "transfer_export",
                    false,
                    &[
                        "Memory IDs, comma separated (blank uses selected memory)",
                        "Format: jsonl or markdown",
                        "New output file path (never overwrites)",
                    ],
                );
            }
            KeyCode::Char('I') => self.start(
                "transfer_import",
                false,
                &["Input file path (up to 2 MiB)", "Format: jsonl or markdown"],
            ),
            KeyCode::Char('D')
                if matches!(
                    self.pending,
                    Some(Action::MemoryExport { .. } | Action::MemoryImport { .. })
                ) =>
            {
                self.pending = None;
                self.transfer = Transfer::default();
                self.message = "Transfer cancelled.".into();
            }
            KeyCode::Char('a')
                if matches!(
                    self.pending,
                    Some(Action::MemoryExport { .. } | Action::MemoryImport { .. })
                ) =>
            {
                if self.transfer.access != self.access {
                    self.pending = None;
                    return Err(MaccError::Validation(
                        "Transfer scope changed; preview again".into(),
                    ));
                }
                let action = self.pending.as_ref().unwrap().clone();
                let value = execute(root, action)?;
                // Applying/replaying an export never overwrites a local file.
                if let Some(path) = &self.transfer.output {
                    let mut options = std::fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(0o600);
                    }
                    let mut file = options.open(path).map_err(|e| {
                        MaccError::Validation(format!(
                            "Cannot create export file: {e}; choose a new path and preview again"
                        ))
                    })?;
                    file.write_all(value["document"].as_str().unwrap_or_default().as_bytes())
                        .map_err(|e| MaccError::Validation(e.to_string()))?;
                }
                self.pending = None;
                self.result(value);
                self.message="Reviewed transfer applied. Imports remain proposals; existing reviews are preserved.".into();
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub(super) fn transfer_form(
        &mut self,
        root: &Path,
        name: &str,
        values: &[String],
    ) -> Result<bool> {
        if !name.starts_with("transfer_") {
            return Ok(false);
        }
        let access: memory::Access = serde_json::from_value(self.access.clone())
            .map_err(|e| MaccError::Validation(e.to_string()))?;
        let format = |value: &str| match value {
            "jsonl" => Ok(Format::Jsonl),
            "markdown" => Ok(Format::Markdown),
            _ => Err(MaccError::Validation("Choose jsonl or markdown".into())),
        };
        let (preview, pending, output) = if name == "transfer_export" {
            let ids = if values[0].trim().is_empty() {
                vec![self.selected()?["memory"]["id"]
                    .as_str()
                    .ok_or_else(|| MaccError::Validation("Select a memory first".into()))?
                    .into()]
            } else {
                values[0].split(',').map(|s| s.trim().to_string()).collect()
            };
            let output = PathBuf::from(&values[2]);
            let output = if output.is_absolute() {
                output
            } else {
                root.join(output)
            };
            if values[2].trim().is_empty() || output.symlink_metadata().is_ok() {
                return Err(MaccError::Validation(
                    "Choose a new, non-existing export file".into(),
                ));
            }
            let request = ExportRequest {
                ids,
                access,
                format: format(&values[1])?,
            };
            let plan = execute(
                root,
                Action::MemoryExportPreview {
                    request: request.clone(),
                },
            )?;
            let action = Action::MemoryExport {
                request,
                checksum: plan["checksum"].as_str().unwrap_or_default().into(),
                confirmed: true,
            };
            (plan, action, Some(output))
        } else {
            let path = PathBuf::from(&values[0]);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            let mut raw = String::new();
            std::fs::File::open(path)
                .map_err(|e| MaccError::Validation(e.to_string()))?
                .take(2 * 1024 * 1024 + 1)
                .read_to_string(&mut raw)
                .map_err(|e| MaccError::Validation(e.to_string()))?;
            let request = ImportRequest {
                document: raw,
                access,
                format: format(&values[1])?,
            };
            let plan = execute(
                root,
                Action::MemoryImportPreview {
                    request: request.clone(),
                },
            )?;
            let action = Action::MemoryImport {
                request,
                checksum: plan["checksum"].as_str().unwrap_or_default().into(),
                confirmed: true,
            };
            (plan, action, None)
        };
        self.transfer = Transfer {
            access: self.access.clone(),
            output,
        };
        self.pending = Some(pending);
        self.result(preview);
        self.message="Inspect every record and scope. Press a to confirm this exact transfer or D to discard it.".into();
        Ok(true)
    }
}
