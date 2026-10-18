use super::*;
impl Panel {
    pub(super) fn setting_indices(&self) -> Vec<usize> {
        let Some(view) = &self.view else {
            return vec![];
        };
        let group = view
            .fields
            .get(self.selected)
            .and_then(|f| f.path.split('.').next())
            .unwrap_or("memory");
        let mut indices: Vec<_> = view
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.path.starts_with(&format!("{group}.")))
            .map(|(i, _)| i)
            .collect();
        indices.sort_by_key(|i| !view.fields[*i].path.ends_with(".enabled"));
        indices
    }
    pub(super) fn setting_key(&mut self, root: &Path, key: KeyCode) -> Result<bool> {
        match key {
            KeyCode::Left | KeyCode::Right => {
                let groups = [
                    "memory",
                    "context_engine",
                    "adaptive_compaction",
                    "code_intelligence",
                ];
                if let Some(view) = &self.view {
                    let current = view
                        .fields
                        .get(self.selected)
                        .and_then(|f| {
                            groups
                                .iter()
                                .position(|g| f.path.starts_with(&format!("{g}.")))
                        })
                        .unwrap_or(0);
                    let next = if key == KeyCode::Right {
                        (current + 1) % 4
                    } else {
                        (current + 3) % 4
                    };
                    self.selected = view
                        .fields
                        .iter()
                        .position(|f| f.path == format!("{}.enabled", groups[next]))
                        .or_else(|| {
                            view.fields
                                .iter()
                                .position(|f| f.path.starts_with(&format!("{}.", groups[next])))
                        })
                        .unwrap_or(0);
                    self.details.clear();
                    self.scroll = 0;
                }
            }
            KeyCode::Char('/') => self.start(
                "find_field",
                false,
                &["Find policy by canonical path or label"],
            ),
            KeyCode::Up | KeyCode::Down => {
                let indices = self.setting_indices();
                if let Some(position) = indices.iter().position(|i| *i == self.selected) {
                    let next = if key == KeyCode::Down {
                        (position + 1).min(indices.len().saturating_sub(1))
                    } else {
                        position.saturating_sub(1)
                    };
                    self.selected = indices[next];
                }
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
        if matches!(
            key,
            KeyCode::Enter | KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
        ) {
            self.details.clear();
            self.scroll = 0;
        }
        Ok(true)
    }
}
