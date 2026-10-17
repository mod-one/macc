use super::*;
#[derive(Clone)]
pub(super) struct Form {
    pub name: String,
    pub labels: Vec<String>,
    pub values: Vec<String>,
    pub input: String,
    pub mutation: bool,
}
impl Panel {
    pub(super) fn start(&mut self, name: &str, mutation: bool, labels: &[&str]) {
        self.form = Some(Form {
            name: name.into(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            values: vec![],
            input: String::new(),
            mutation,
        });
        self.message.clear();
    }
    pub(super) fn form_key(&mut self, root: &Path, key: KeyCode) -> Result<()> {
        let form = self.form.as_mut().unwrap();
        match key {
            KeyCode::Esc => {
                self.form = None;
                return Ok(());
            }
            KeyCode::BackTab => {
                if let Some(previous) = form.values.pop() {
                    form.input = previous;
                }
            }
            KeyCode::Backspace => {
                form.input.pop();
            }
            KeyCode::Char(c) => form.input.push(c),
            KeyCode::Enter => {
                form.values.push(std::mem::take(&mut form.input));
                if form.values.len() < form.labels.len() {
                    return Ok(());
                }
                let form = self.form.take().unwrap();
                let running = self.job.is_some();
                if let Err(e) = self.submit(root, &form.name, form.values.clone()) {
                    let mut retained = form;
                    retained.input = retained.values.pop().unwrap_or_default();
                    self.form = Some(retained);
                    return Err(e);
                }
                if !running && self.job.is_some() {
                    self.retain_job_form(form);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn submit(&mut self, root: &Path, name: &str, v: Vec<String>) -> Result<()> {
        let empty = |s: &str| if s.is_empty() { Value::Null } else { json!(s) };
        let parse = |s: &str| {
            s.parse::<u64>()
                .map_err(|_| MaccError::Validation("Enter a nonnegative whole number".into()))
        };
        let mut pending = false;
        let action = match name {
            "semantic_benchmark" => {pending=true; Action::SemanticBenchmark {suite:macc_core::o2::semantic::Suite {
                access:serde_json::from_value(self.access.clone()).map_err(|e|MaccError::Validation(e.to_string()))?,
                top_k:parse(&v[8])? as usize,
                cases:(0..4).map(|i|macc_core::o2::semantic::Case{query:v[i*2].clone(),relevant_ids:v[i*2+1].split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_string).collect()}).collect(),
            }}},
            "semantic_search" => {self.query=v[0].clone();self.conflicts=false;Action::MemorySearch {search:serde_json::from_value(json!({"query":self.query,"access":self.access,"include_inactive":false})).map_err(|e|MaccError::Validation(e.to_string()))?}},
            "find_field" => {
                let needle = v[0].to_ascii_lowercase();
                if let Some((index, _)) = self
                    .view
                    .as_ref()
                    .unwrap()
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, f)| {
                        format!("{} {}", f.path, f.label)
                            .to_ascii_lowercase()
                            .contains(&needle)
                    })
                {
                    self.selected = index;
                    self.scroll = index.saturating_sub(3) as u16;
                } else {
                    self.message = "No matching policy field".into();
                }
                return Ok(());
            }
            "text_field" => {
                let f = &self.view.as_ref().unwrap().fields[self.selected];
                let pointer = format!("/{}", f.path.replace('.', "/"));
                *self.draft.pointer_mut(&pointer).unwrap() = json!(v[0]);
                self.preview = false;
                self.inherit = false;
                return Ok(());
            }
            "overview_filter" => { self.overview_filter(root,&v[0])?; return Ok(()); },
            "compare" => Action::CodeCompare {
                provider: v[0].clone(),
                query: serde_json::from_value(
                    json!({"text":v[1],"kind":v[2],"worktree":empty(&v[3]),"path":v.get(4).map(|s|empty(s))}),
                )
                .unwrap(),
            },
            "extract" => { pending = true; Action::MemoryExtract { checkpoint_id:v[0].clone() } },
            "operation" => Action::ProviderOperation { id: v[0].clone() },
            "field" => {
                let f = &self.view.as_ref().unwrap().fields[self.selected];
                let pointer = format!("/{}", f.path.replace('.', "/"));
                let value: Value = serde_json::from_str(&v[0])
                    .map_err(|e| MaccError::Validation(e.to_string()))?;
                if !value.is_number() {
                    return Err(MaccError::Validation(
                        "Numeric policy value required".into(),
                    ));
                }
                *self.draft.pointer_mut(&pointer).unwrap() = value;
                self.preview = false;
                self.inherit = false;
                return Ok(());
            }
            "search" => {
                self.conflicts = false;
                self.captured_evidence=None;
                self.query = v[0].clone();
                self.access = json!({"task_id":empty(&v[1]),"tool_id":empty(&v[2]),"worktree":empty(&v[3]),"feature":empty(&v[4]),"user_id":empty(&v[5])});
                let search = serde_json::from_value(
                    json!({"query":v[0],"access":self.access,"include_inactive":true}),
                )
                .unwrap();
                if !self.view.as_ref().is_some_and(|v|v.ready) {return Err(MaccError::Validation("O2 SQLite authority unavailable; input retained".into()));}
                self.rows.clear(); self.selected=0;
                return self.launch(root,Action::MemorySearch {search});
            }
            "capture_symbol" => Action::MemorySymbolEvidence { path:v[0].clone(),symbol:v[1].clone(),provider:if v[2].is_empty(){"native".into()}else{v[2].clone()},worktree:(!v[3].is_empty()).then(||v[3].clone()) },
            "capture_tool" => Action::MemoryToolEvidence {tool_id:v[0].clone()},
            "propose" => {
                let evidence = if v[5].is_empty() { self.captured_evidence.clone().ok_or_else(||MaccError::Validation("Capture evidence with y/Y or enter a source path".into()))? } else { execute(
                    root,
                    Action::MemoryEvidence {
                        path: v[5].clone(),
                        worktree: (v[3] == "worktree").then(|| v[4].clone()),
                    },
                )? };
                Action::MemoryPropose{proposal:serde_json::from_value(json!({"kind":v[0],"title":v[1],"body":v[2],"scope":v[3],"scope_ref":empty(&v[4]),"confidence":v[6].parse::<f64>().map_err(|_|MaccError::Validation("Confidence must be numeric".into()))?,"evidence":[evidence],"tags":[],"source_task":if v[3]=="task"{empty(&v[4])}else{Value::Null},"source_revision":null})).map_err(|e|MaccError::Validation(e.to_string()))?}
            }
            "review" | "review_captured" => {
                let evidence = if name=="review_captured" {Some(vec![self.captured_evidence.clone().ok_or_else(||MaccError::Validation("Capture current evidence first".into()))?])} else if v.get(2).is_some_and(|s| !s.is_empty()) {
                    Some(vec![execute(
                        root,
                        Action::MemoryEvidence {
                            path: v[2].clone(),
                            worktree: if self.selected()?["memory"]["proposal"]["scope"]
                                == "worktree"
                            {
                                self.selected()?["memory"]["proposal"]["scope_ref"]
                                    .as_str()
                                    .map(str::to_string)
                            } else {
                                None
                            },
                        },
                    )?])
                } else {
                    None
                };
                let id = self.selected()?["memory"]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let row = execute(
                    root,
                    Action::MemoryGet {
                        id,
                        access: serde_json::from_value(self.access.clone())
                            .map_err(|e| MaccError::Validation(e.to_string()))?,
                    },
                )?;
                pending = true;
                Action::MemoryReview{review:serde_json::from_value(json!({"id":row["memory"]["id"],"status":v[0],"reason":v[1],"confirmed":true,"expected_checksum":row["checksum"],"evidence":evidence})).map_err(|e|MaccError::Validation(e.to_string()))?}
            }
            "conflict" => {
                pending = true;
                Action::MemoryConflict {
                    left: self.selected()?["memory"]["id"]
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    right: v[0].clone(),
                    reason: v[1].clone(),
                }
            }
            "resolve_conflict" => {
                pending = true;
                Action::MemoryResolveConflict {
                    resolution: serde_json::from_value(json!(v.get(1).filter(|v|!v.is_empty()).map(String::as_str).unwrap_or("both_stale"))).map_err(|e|MaccError::Validation(e.to_string()))?,
                    expected_checksum:self.selected()?["checksum"].as_str().map(str::to_string),
                    id: self.selected()?["id"].as_str().unwrap_or_default().into(),
                    reason: v[0].clone(),
                    confirmed: true,
                }
            }
            "pull" => Action::MemoryPull {
                search: serde_json::from_value(json!({"query":self.query,"access":self.access}))
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
                cursor: parse(&v[0])? as usize,
            },
            "read" => Action::MemoryRead {
                id: self.selected()?["memory"]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                access: serde_json::from_value(self.access.clone())
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
                offset: parse(&v[0])? as usize,
            },
            "provider" => {
                let data = execute(
                    root,
                    Action::ProviderPlan {
                        provider: v[0].clone(),
                        operation: v[1].clone(),
                    },
                )?;
                if data["result"]["outcome"] == "supported" {
                    self.pending = Some(Action::ProviderApply {
                        plan_id: data["result"]["data"]["id"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                        checksum: data["checksum"].as_str().unwrap_or_default().into(),
                    });
                    self.message="Review paths, origin, network and operation. Press a to apply this exact plan.".into();
                }
                self.result(data);
                return Ok(());
            }
            "adopt_native" => {
                let plan = execute(root, Action::NativeSettingPreview { id: v[0].clone() })?;
                self.pending = Some(Action::NativeSettingAdopt {
                    id: v[0].clone(),
                    checksum: plan["checksum"].as_str().unwrap_or_default().into(),
                });
                self.result(plan);
                self.message="Review the exact value, path and policy before a to adopt ownership. No native file is changed by adoption.".into();
                return Ok(());
            }
            "checkpoint" => Action::CheckpointCreate { id: v[0].clone() },
            "rotate" => {
                pending = true;
                Action::Rotate {
                    id: v[0].clone(),
                    confirmed: true,
                }
            }
            "decision" => Action::CompactionPreview {
                uses: parse(&v[0])?,
                budget: parse(&v[1])?,
                context_tokens: if v[2].is_empty() {
                    None
                } else {
                    Some(parse(&v[2])?)
                },
                window: if v[3].is_empty() {
                    None
                } else {
                    Some(parse(&v[3])?)
                },
                in_flight: false,
            },
            "code" => Action::CodeQuery {
                query: serde_json::from_value(
                    json!({"text":v[0],"kind":v[1],"worktree":empty(&v[2]),"path":v.get(3).map(|s|empty(s))}),
                )
                .unwrap(),
            },
            "reduce" => Action::Reduce {
                kind: v[0].clone(),
                raw: v[1].clone(),
            },
            "advanced" => {
                let action: Action = serde_json::from_str(&v[0])
                    .map_err(|e| MaccError::Validation(e.to_string()))?;
                pending = action.is_mutation();
                action
            }
            _ => return Err(MaccError::Validation("Unknown O2 form".into())),
        };
        if pending {
            self.details = format!(
                "Review this exact operation before pressing a:\n{}",
                serde_json::to_string_pretty(&action).unwrap_or_default()
            );
            self.pending = Some(action);
            self.message =
                "Prepared operation; no mutation applied. Press a to confirm after inspection."
                    .into();
        } else {
            if matches!(
                action,
                Action::MemorySearch { .. }
                    | Action::SemanticStatus { .. }
                    | Action::CodeQuery { .. }
                    | Action::CodeCompare { .. }
                    | Action::MemoryToolEvidence { .. }
                    | Action::MemorySymbolEvidence { .. }
                    | Action::MemoryPull { .. }
            ) {
                return self.launch(root, action);
            }
            let value = execute(root, action)?;
            self.result(value);
            self.refresh(root)?;
        }
        Ok(())
    }
}
