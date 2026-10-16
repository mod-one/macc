use super::*;
impl Panel {
    pub(super) fn setting_key(&mut self, root: &Path, key: KeyCode) -> Result<bool> {
        match key {
            KeyCode::Char('/') => self.start(
                "find_field",
                false,
                &["Find policy by canonical path or label"],
            ),
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(
                    self.view
                        .as_ref()
                        .map(|v| v.fields.len())
                        .unwrap_or_default()
                        .saturating_sub(1),
                )
            }
            KeyCode::Enter => {
                let f = self
                    .view
                    .as_ref()
                    .and_then(|v| v.fields.get(self.selected))
                    .ok_or_else(|| MaccError::Validation("No policy field".into()))?;
                let pointer = format!("/{}", f.path.replace('.', "/"));
                let value = self
                    .draft
                    .pointer(&pointer)
                    .cloned()
                    .unwrap_or(f.default.clone());
                if f.kind == "bool" {
                    *self.draft.pointer_mut(&pointer).unwrap() =
                        json!(!value.as_bool().unwrap_or(false));
                    self.preview = false;
                    self.inherit = false;
                } else if f.kind == "enum" {
                    let pos = f
                        .choices
                        .iter()
                        .position(|s| Some(s.as_str()) == value.as_str())
                        .unwrap_or(0);
                    *self.draft.pointer_mut(&pointer).unwrap() =
                        json!(f.choices[(pos + 1) % f.choices.len()]);
                    self.preview = false;
                    self.inherit = false;
                } else if f.kind == "string" {
                    self.start(
                        "text_field",
                        false,
                        &["Executable (absolute path or PATH basename)"],
                    );
                } else {
                    self.start(
                        "field",
                        false,
                        &["Numeric value (exact entry; preview validates all fields)"],
                    );
                }
            }
            KeyCode::Char('p') => {
                let p = if self.inherit {
                    None
                } else {
                    Some(
                        serde_json::from_value::<policy::Policy>(self.draft.clone())
                            .map_err(|e| MaccError::Validation(e.to_string()))?,
                    )
                };
                let result = execute(
                    root,
                    Action::PolicyPreview {
                        revision: self.revision.clone(),
                        policy: p,
                    },
                )?;
                self.preview = true;
                self.result(result);
            }
            KeyCode::Char('s') => {
                if !self.preview {
                    return Err(MaccError::Validation(
                        "Preview the current draft with p before applying".into(),
                    ));
                }
                let p = if self.inherit {
                    None
                } else {
                    Some(
                        serde_json::from_value::<policy::Policy>(self.draft.clone())
                            .map_err(|e| MaccError::Validation(e.to_string()))?,
                    )
                };
                execute(
                    root,
                    Action::PolicyApply {
                        revision: self.revision.clone(),
                        policy: p,
                    },
                )?;
                let view = policy::inspect(root)?;
                self.revision = view.revision.clone();
                self.draft = serde_json::to_value(&view.policy).unwrap();
                self.view = Some(view);
                self.inherit = false;
                self.preview = false;
                self.message =
                    "Policy saved; next invocation/query. Deployment remains separate.".into();
            }
            KeyCode::Char('x') => {
                let view = policy::inspect(root)?;
                self.draft = serde_json::to_value(&view.policy).unwrap();
                self.revision = view.revision.clone();
                self.view = Some(view);
                self.inherit = false;
                self.preview = false;
                self.message = "Draft discarded.".into();
            }
            KeyCode::Char('b') => {
                self.draft = serde_json::to_value(policy::Policy::default()).unwrap();
                self.preview = false;
                self.inherit = false;
            }
            KeyCode::Char('i') => {
                self.inherit = true;
                self.preview = false;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub(super) fn draw_settings(&self, lines: &mut Vec<String>) {
        lines.push("Enter toggle/edit | p validate/preview | s apply | b default draft | i remove overrides | x discard. Mandatory PRD/evidence/isolation rules cannot be disabled.".into());
        if let Some(view) = &self.view {
            for (i, f) in view.fields.iter().enumerate() {
                let pointer = format!("/{}", f.path.replace('.', "/"));
                lines.push(format!(
                    "{} {} = {} | {} | default {}",
                    if i == self.selected { ">" } else { " " },
                    f.path,
                    self.draft.pointer(&pointer).unwrap_or(&f.default),
                    f.label,
                    f.default
                ));
            }
            lines.push(format!("Source: {} | Activation: next invocation/query. Native auto overrides require a matching version/help probe and explicit opt-in.",view.source));
        }
    }
}
