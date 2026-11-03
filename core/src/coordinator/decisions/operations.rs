//! Durable external-operation state machine. Reservation is never treated as
//! external success, and an ambiguous execution is never blindly repeated.
use super::{store::*, *};
use rusqlite::{OptionalExtension, TransactionBehavior};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn key(plan: &str, id: &str) -> String {
    format!("operation:{}", digest(&serde_json::json!([plan, id])))
}
fn identity(pid: u32) -> Option<String> {
    if !crate::coordinator::helpers::is_pid_running(pid as i64) {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let tail = stat
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        if matches!(tail.first().copied(), Some("Z" | "X")) {
            return None;
        }
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        Some(format!("{pid}:{}:{}", boot.trim(), tail.get(19)?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Some(format!("{pid}"))
    }
}
fn alive(saved: &Option<String>) -> bool {
    saved
        .as_ref()
        .and_then(|s| s.split(':').next()?.parse::<u32>().ok())
        .and_then(identity)
        .as_ref()
        == saved.as_ref()
        && saved.is_some()
}
fn adapter(policy: &Policy, spec: &OperationSpec) -> crate::Result<OperationAdapter> {
    let a = policy
        .operation_adapters
        .get(&spec.adapter)
        .cloned()
        .ok_or_else(|| error("No trusted operation adapter is configured"))?;
    if a.command.is_empty()
        || !Path::new(&a.command[0]).is_absolute()
        || a.timeout_seconds == 0
        || a.timeout_seconds > 3600
        || !(1024..=1048576).contains(&a.max_output_bytes)
    {
        return Err(error("Operation adapters require an absolute executable, 1..3600 second timeout and 1024..1048576 output bytes"));
    }
    if !a.allowed_targets.contains(&spec.target) {
        return Err(error(
            "Operation target is not authorized by its trusted adapter",
        ));
    }
    if a.verified_idempotency.is_some() && (a.idempotency_key_arg.is_none() || a.probe.is_empty()) {
        return Err(error(
            "Verified retries require an idempotency key argument and an outcome probe",
        ));
    }
    Ok(a)
}
impl Store {
    pub fn operation(&self, plan: &str, id: &str) -> crate::Result<Option<OperationExecution>> {
        let payload: Option<String> = sql(self
            .db
            .query_row(
                "SELECT value FROM metadata WHERE key=?1",
                [key(plan, id)],
                |r| r.get(0),
            )
            .optional())?;
        payload.map(|s| decode(&s)).transpose()
    }
    pub fn operations(&self, plan: &str) -> crate::Result<Vec<OperationExecution>> {
        let mut stmt = sql(self
            .db
            .prepare("SELECT value FROM metadata WHERE key LIKE 'operation:%' ORDER BY key"))?;
        let rows = sql(stmt.query_map([], |r| r.get::<_, String>(0)))?;
        let mut values = Vec::new();
        for row in rows {
            let op: OperationExecution = decode(&sql(row)?)?;
            if op.plan_id == plan {
                values.push(op);
            }
        }
        Ok(values)
    }
    fn save_operation(&mut self, op: &OperationExecution, actor: &str) -> crate::Result<()> {
        self.save_operation_authorized(op, actor, None)
    }
    fn save_operation_authorized(
        &mut self,
        op: &OperationExecution,
        actor: &str,
        authority: Option<serde_json::Value>,
    ) -> crate::Result<()> {
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![key(&op.plan_id,&op.operation_id),encode(op)?]))?;
        event(
            &tx,
            &op.plan_id,
            "operation.transition",
            &serde_json::json!({"actor":actor,"operation":op,"authority":authority}),
        )?;
        sql(tx.commit())?;
        Ok(())
    }
    pub fn recover_operations(&mut self) -> crate::Result<usize> {
        let plans = self.plans()?;
        let mut changed = 0;
        for plan in plans {
            for mut op in self.operations(&plan.id)? {
                if op.state == OperationState::Executing
                    && !alive(&op.owner_identity)
                    && !alive(&op.child_identity)
                {
                    let _lock = self.mutation_lock()?;
                    op = self
                        .operation(&plan.id, &op.operation_id)?
                        .ok_or_else(|| error("Missing operation during recovery"))?;
                    if op.state != OperationState::Executing
                        || alive(&op.owner_identity)
                        || alive(&op.child_identity)
                    {
                        continue;
                    }
                    op.version += 1;
                    op.state = OperationState::Unknown;
                    op.evidence="Execution owner and child are gone; external outcome is unknown. Reconciliation required before retry.".into();
                    op.updated_at = now();
                    self.save_operation(&op, "recovery")?;
                    changed += 1;
                }
            }
        }
        Ok(changed)
    }
    fn operation_spec(
        &self,
        d: &Decision,
        p: &Policy,
    ) -> crate::Result<(OperationSpec, OperationAdapter, PathBuf)> {
        let spec=d.request.operation_spec.clone().ok_or_else(||error("This authorization predates executable operation manifests; obtain a fresh authorization with operation_spec"))?;
        let adapter = adapter(p, &spec)?;
        let registry = registry(&self.root)?;
        let root = registry
            .find_task(&d.request.task_id)
            .and_then(|t| t.worktree_path())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.root.clone());
        if spec.source_revision != crate::git::head_commit(&root)? || crate::git::is_dirty(&root)? {
            return Err(error("Operation source differs from the reviewed clean commit; obtain fresh authorization"));
        }
        Ok((spec, adapter, root))
    }
    pub fn execute_operation(&mut self, decision_id: &str) -> crate::Result<OperationExecution> {
        let p = policy(&self.root)?;
        let d = self.decision(decision_id)?;
        let id = d
            .request
            .operation_id
            .clone()
            .ok_or_else(|| error("Missing operation ID"))?;
        let (spec, a, root) = self.operation_spec(&d, &p)?;
        // Existing reservation validates decision status, subject, policy and
        // stable operation ID. The operation metadata CAS adds execution state.
        self.consume_authorization(decision_id, &id)?;
        let mut op = {
            let _lock = self.mutation_lock()?;
            let existing = self.operation(&d.plan_id, &id)?;
            if let Some(old) = existing.as_ref() {
                if old.decision_id != d.id
                    || old.spec_digest != digest(&spec)
                    || old.policy_digest != digest(&p)
                {
                    return Err(error(
                        "Operation reservation does not match this authorization/manifest",
                    ));
                }
                if old.state == OperationState::Succeeded {
                    return Ok(old.clone());
                }
                if old.state != OperationState::Reserved {
                    return Err(error("Operation was already started; reconcile its external outcome before retrying"));
                }
            }
            let mut op = existing.unwrap_or(OperationExecution {
                operation_id: id.clone(),
                decision_id: d.id.clone(),
                plan_id: d.plan_id.clone(),
                version: 0,
                state: OperationState::Reserved,
                spec_digest: digest(&spec),
                policy_digest: digest(&p),
                owner_identity: None,
                child_identity: None,
                evidence: String::new(),
                output_digest: None,
                output_path: None,
                updated_at: now(),
            });
            op.version += 1;
            op.state = OperationState::Executing;
            op.owner_identity = identity(std::process::id());
            op.updated_at = now();
            op.evidence = "Execution intent committed before spawning the external command".into();
            self.save_operation(&op, "operation runner")?;
            op
        };
        let outcome =
            self.run_operation_command(&a.command, &a, &spec, &id, &root, true, |store, child| {
                op.version += 1;
                op.child_identity = identity(child);
                op.updated_at = now();
                store.save_operation(&op, "operation runner")
            })?;
        let _lock = self.mutation_lock()?;
        let mut current = self
            .operation(&d.plan_id, &id)?
            .ok_or_else(|| error("Operation state disappeared"))?;
        current.version += 1;
        current.state = if outcome.0 == Some(0) {
            OperationState::Succeeded
        } else {
            OperationState::Unknown
        };
        current.owner_identity = None;
        current.child_identity = None;
        current.updated_at = now();
        current.output_digest = Some(outcome.1);
        current.output_path = Some(outcome.2);
        current.evidence=if current.state==OperationState::Succeeded{"Adapter reported successful completion"}else{"Command failed, timed out or exceeded output budget; partial external effects are possible. Reconcile before retry."}.into();
        self.save_operation(&current, "operation runner")?;
        Ok(current)
    }
    pub fn reconcile_operation(
        &mut self,
        plan: &str,
        id: &str,
        version: u64,
        state: OperationState,
        evidence: &str,
        actor: &str,
    ) -> crate::Result<OperationExecution> {
        self.reconcile_operation_attested(plan, id, version, state, evidence, actor, None)
    }
    pub fn reconcile_operation_attested(
        &mut self,
        plan: &str,
        id: &str,
        version: u64,
        state: OperationState,
        evidence: &str,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> crate::Result<OperationExecution> {
        self.reconcile_operation_inner(plan, id, version, state, evidence, actor, proof, false)
    }
    fn reconcile_operation_inner(
        &mut self,
        plan: &str,
        id: &str,
        version: u64,
        state: OperationState,
        evidence: &str,
        actor: &str,
        proof: Option<&Attestation>,
        trusted_probe: bool,
    ) -> crate::Result<OperationExecution> {
        let _lock = self.mutation_lock()?;
        let mut op = self
            .operation(plan, id)?
            .ok_or_else(|| error("Unknown operation"))?;
        let p = policy(&self.root)?;
        let actor = if trusted_probe {
            actor.to_owned()
        } else {
            super::secure::mutation_authority(
                &self.root,
                "operation.reconcile",
                &super::secure::operation_challenge(&op, state, evidence, &p),
                actor,
                proof,
                &p,
            )?
        };
        let actor = actor.as_str();
        if op.version != version {
            return Err(error("Operation changed; refresh before reconciliation"));
        }
        if alive(&op.owner_identity) || alive(&op.child_identity) {
            return Err(error("Operation still has a live execution owner/child"));
        }
        if !matches!(
            op.state,
            OperationState::Unknown | OperationState::Failed | OperationState::Executing
        ) {
            return Err(error(
                "Only an ambiguous/interrupted operation can be reconciled",
            ));
        }
        if !matches!(
            state,
            OperationState::Succeeded | OperationState::Failed | OperationState::Reserved
        ) || evidence.trim().is_empty()
        {
            return Err(error(
                "Reconciliation needs a conclusive outcome and concrete evidence",
            ));
        }
        if state == OperationState::Reserved {
            let p = policy(&self.root)?;
            let d = self.decision(&op.decision_id)?;
            let (_, a, _) = self.operation_spec(&d, &p)?;
            if a.verified_idempotency.is_none() {
                return Err(error("A non-idempotent operation cannot be rearmed; record its final outcome or use a new explicitly authorized operation"));
            }
        }
        op.state = state;
        op.version += 1;
        op.owner_identity = None;
        op.child_identity = None;
        op.updated_at = now();
        op.evidence = evidence.into();
        self.save_operation_authorized(&op, actor,Some(serde_json::json!({"origin":if trusted_probe{"trusted_adapter_probe"}else{"operator"},"attestation":proof})))?;
        Ok(op)
    }
    pub fn probe_operation(
        &mut self,
        plan: &str,
        id: &str,
        version: u64,
    ) -> crate::Result<OperationExecution> {
        let op = self
            .operation(plan, id)?
            .ok_or_else(|| error("Unknown operation"))?;
        if op.version != version {
            return Err(error("Stale operation version"));
        }
        if alive(&op.owner_identity) || alive(&op.child_identity) {
            return Err(error("Operation is still executing"));
        }
        let p = policy(&self.root)?;
        let d = self.decision(&op.decision_id)?;
        let (spec, a, root) = self.operation_spec(&d, &p)?;
        if a.probe.is_empty() {
            return Err(error(
                "No trusted outcome probe; operator reconciliation is required",
            ));
        }
        let result =
            self.run_operation_command(&a.probe, &a, &spec, id, &root, false, |_, _| Ok(()))?;
        let state = match result.0 {
            Some(0) => OperationState::Succeeded,
            Some(10) if a.verified_idempotency.is_some() => OperationState::Reserved,
            Some(11) => OperationState::Failed,
            _ => return Err(error(
                "Probe did not establish a conclusive external outcome; operation remains blocked",
            )),
        };
        self.reconcile_operation_inner(
            plan,
            id,
            version,
            state,
            &format!(
                "Trusted adapter probe result {:?}, evidence digest {}",
                result.0, result.1
            ),
            "trusted outcome probe",
            None,
            true,
        )
    }
    fn run_operation_command<F>(
        &mut self,
        template: &[String],
        a: &OperationAdapter,
        spec: &OperationSpec,
        id: &str,
        root: &Path,
        execute: bool,
        mut started: F,
    ) -> crate::Result<(Option<i32>, String, String)>
    where
        F: FnMut(&mut Self, u32) -> crate::Result<()>,
    {
        if template.is_empty() || !Path::new(&template[0]).is_absolute() {
            return Err(error(
                "Trusted operation command must have an absolute executable",
            ));
        }
        let expand = |value: &str| -> crate::Result<String> {
            if value == "{target}" {
                Ok(spec.target.clone())
            } else if value == "{operation_id}" {
                Ok(id.into())
            } else if let Some(key) = value
                .strip_prefix("{argument:")
                .and_then(|s| s.strip_suffix('}'))
            {
                spec.arguments
                    .get(key)
                    .cloned()
                    .ok_or_else(|| error(format!("Missing operation argument {key}")))
            } else if value.contains('{') || value.contains('}') {
                Err(error(
                    "Operation placeholders must occupy an entire argument",
                ))
            } else {
                Ok(value.into())
            }
        };
        let args = template[1..]
            .iter()
            .map(|s| expand(s))
            .collect::<crate::Result<Vec<_>>>()?;
        let dir = self
            .root
            .join(".macc/runtime/operations")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).map_err(|e| error(e.to_string()))?;
        let path = dir.join("output.log");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|e| error(e.to_string()))?;
        let mut command = Command::new(&template[0]);
        command
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("LANG", "C.UTF-8");
        for name in &a.environment_names {
            if name.starts_with("MACC_") {
                return Err(error(
                    "Private MACC authority variables cannot be passed to an operation adapter",
                ));
            }
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        if execute {
            if let Some(flag) = &a.idempotency_key_arg {
                command.arg(flag).arg(id);
            }
        }
        #[cfg(unix)]
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(not(unix))]
        command
            .stdout(Stdio::from(
                file.try_clone().map_err(|e| error(e.to_string()))?,
            ))
            .stderr(Stdio::from(
                file.try_clone().map_err(|e| error(e.to_string()))?,
            ));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return Ok((None, digest(&e.to_string()), path.display().to_string())),
        };
        if let Err(e) = started(self, child.id()) {
            kill_operation(&mut child);
            return Err(e);
        }
        #[cfg(unix)]
        {
            capture_unix(&mut child, file, &path, a)
        }
        #[cfg(not(unix))]
        {
            let began = Instant::now();
            let code = loop {
                if let Some(status) = child.try_wait().map_err(|e| error(e.to_string()))? {
                    break status.code();
                }
                if began.elapsed() >= Duration::from_secs(a.timeout_seconds)
                    || std::fs::metadata(&path).is_ok_and(|m| m.len() > a.max_output_bytes as u64)
                {
                    kill_operation(&mut child);
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .map_err(|e| error(e.to_string()))?
                .take((a.max_output_bytes + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| error(e.to_string()))?;
            Ok((
                if bytes.len() > a.max_output_bytes {
                    None
                } else {
                    code
                },
                digest(&bytes),
                path.display().to_string(),
            ))
        }
    }
}

fn kill_operation(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Drain both streams without blocking, enforcing a combined hard disk budget.
#[cfg(unix)]
pub(super) fn capture_unix(
    child: &mut std::process::Child,
    mut file: std::fs::File,
    path: &Path,
    a: &OperationAdapter,
) -> crate::Result<(Option<i32>, String, String)> {
    use std::os::fd::AsRawFd;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            kill_operation(child);
            return Err(error(
                "Cannot establish bounded nonblocking operation capture",
            ));
        }
    }
    let began = Instant::now();
    let mut remaining = a.max_output_bytes;
    let mut status = None;
    let mut out_closed = false;
    let mut err_closed = false;
    let code = loop {
        let result = (|| {
            if !out_closed {
                out_closed = drain(&mut stdout, &mut file, &mut remaining)?;
            }
            if !err_closed {
                err_closed = drain(&mut stderr, &mut file, &mut remaining)?;
            }
            Ok::<_, std::io::Error>(())
        })();
        if result.is_err() || began.elapsed() >= Duration::from_secs(a.timeout_seconds) {
            kill_operation(child);
            break None;
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => {
                    kill_operation(child);
                    break None;
                }
            }
        }
        if out_closed && err_closed {
            if let Some(status) = status {
                break status.code();
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let bytes = std::fs::read(path).map_err(|e| error(e.to_string()))?;
    Ok((code, digest(&bytes), path.display().to_string()))
}
#[cfg(unix)]
fn drain<R: Read>(
    reader: &mut R,
    file: &mut std::fs::File,
    remaining: &mut usize,
) -> std::io::Result<bool> {
    use std::io::Write;
    let mut bytes = [0u8; 4096];
    for _ in 0..16 {
        match reader.read(&mut bytes) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                let count = n.min(*remaining);
                file.write_all(&bytes[..count])?;
                *remaining -= count;
                if count < n {
                    return Err(std::io::Error::other("Operation output budget exceeded"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}
