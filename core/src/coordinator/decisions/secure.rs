//! Verification of human attestations issued outside the performer principal.
//! MACC never stores a signing key or provides a signing endpoint.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttestationBody {
    pub principal: String,
    pub role: String,
    pub action: String,
    pub action_digest: String,
    pub nonce: String,
    pub issued_at: i64,
    pub expires_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attestation {
    pub body: AttestationBody,
    pub signature: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustRegistry {
    pub allowed_signers: PathBuf,
    pub roles: BTreeMap<String, Vec<String>>,
    pub required_roles: BTreeMap<String, Vec<String>>,
}
fn protected(path: &Path) -> crate::Result<()> {
    if !path.is_absolute() {
        return Err(error("Secure registry paths must be absolute"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut p = Some(path);
        while let Some(path) = p {
            let m = std::fs::symlink_metadata(path).map_err(|e| error(e.to_string()))?;
            if m.file_type().is_symlink()
                || m.uid() == unsafe { libc::geteuid() }
                || m.mode() & 0o022 != 0
            {
                return Err(error("Secure authority must be owned by an isolated principal, with no writable or symbolic-link ancestor accessible to the performer"));
            }
            p = path.parent();
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err(error("This platform requires an isolated approval broker; Unix protected-registry verification is unavailable"))
    }
}
fn registry(policy: &Policy) -> crate::Result<TrustRegistry> {
    let path = policy
        .trust_registry
        .as_ref()
        .ok_or_else(|| error("secure_approval requires a protected external trust_registry"))?;
    protected(path)?;
    let bytes = std::fs::read(path).map_err(|e| error(e.to_string()))?;
    if bytes.len() > 65536 {
        return Err(error("Trust registry exceeds budget"));
    }
    let registry: TrustRegistry =
        serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
    protected(&registry.allowed_signers)?;
    Ok(registry)
}
pub fn validate_secure_policy(policy: &Policy) -> crate::Result<()> {
    registry(policy).map(|_| ())
}
pub fn verify_attestation(
    root: &Path,
    p: &Policy,
    proof: &Attestation,
    action: &str,
    action_digest: &str,
    at: i64,
) -> crate::Result<String> {
    let registry = registry(p)?;
    verify_bound_attestation(root, &registry, proof, action, action_digest, at)
}
fn verify_bound_attestation(
    root: &Path,
    registry: &TrustRegistry,
    proof: &Attestation,
    action: &str,
    action_digest: &str,
    at: i64,
) -> crate::Result<String> {
    if proof.body.action != action
        || proof.body.action_digest != action_digest
        || proof.body.nonce.len() < 16
        || proof.body.issued_at > at + 30
        || proof.body.expires_at < at
        || proof
            .body
            .expires_at
            .checked_sub(proof.body.issued_at)
            .is_none_or(|ttl| !(0..=3600).contains(&ttl))
        || proof.body.expires_at < proof.body.issued_at
    {
        return Err(error(
            "Approval attestation is stale or bound to a different action/subject/policy",
        ));
    }
    if !registry
        .roles
        .get(&proof.body.principal)
        .is_some_and(|r| r.contains(&proof.body.role))
        || !registry
            .required_roles
            .get(action)
            .is_some_and(|r| r.contains(&proof.body.role))
    {
        return Err(error(
            "Isolated approver does not hold a required role for this action",
        ));
    }
    verify_signature(
        root,
        &registry.allowed_signers,
        &proof.body.principal,
        "macc-approval",
        &canonical_bytes(&proof.body)?,
        &proof.signature,
    )?;
    Ok(proof.body.principal.clone())
}
pub(super) fn verify_signature(
    root: &Path,
    signers: &Path,
    principal: &str,
    namespace: &str,
    payload: &[u8],
    signature: &str,
) -> crate::Result<()> {
    if payload.len() > 4 * 1024 * 1024
        || signature.len() > 16384
        || principal.is_empty()
        || principal.starts_with('-')
    {
        return Err(error("Signature input exceeds verification bounds"));
    }
    let dir = Store::location(root)?
        .parent()
        .unwrap()
        .join("verification");
    std::fs::create_dir_all(&dir).map_err(|e| error(e.to_string()))?;
    let file = dir.join(uuid::Uuid::new_v4().to_string());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(&file).map_err(|e| error(e.to_string()))?;
    out.write_all(signature.as_bytes())
        .map_err(|e| error(e.to_string()))?;
    let result = (|| {
        let mut child = Command::new("/usr/bin/ssh-keygen")
            .args(["-Y", "verify", "-f"])
            .arg(signers)
            .args(["-I", principal, "-n", namespace, "-s"])
            .arg(&file)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| error(e.to_string()))?;
        // Payload is bounded; the verifier consumes standard input before verification.
        let mut stdin = child.stdin.take().unwrap();
        let data = payload.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&data));
        let began = Instant::now();
        let valid = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s.success(),
                Ok(None) if began.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
            }
        };
        let _ = writer.join();
        if !valid {
            return Err(error("Untrusted or invalid isolated approver signature"));
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(file);
    result
}
pub fn answer_challenge(d: &Decision, input: &AnswerInput, p: &Policy) -> serde_json::Value {
    let mut input = input.clone();
    input.attestation = None;
    serde_json::json!({"action":"decision.answer","project_scope":d.plan_id,"decision_id":d.id,"version":d.version,"subject_digest":d.subject_digest,"policy_digest":digest(p),"answer":input})
}
impl Store {
    pub(super) fn verify_human_binding(&self, d: &Decision, p: &Policy) -> crate::Result<()> {
        let mut root = d.clone();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(reference) = root.reused_from.clone() {
            if !seen.insert(root.id.clone()) {
                return Err(error("Decision provenance contains a cycle"));
            }
            let parent = self.decision(&reference.id)?;
            if parent.status != "resolved"
                || parent.version != reference.version
                || parent.resolution.as_ref().is_none_or(|r| {
                    root.resolution
                        .as_ref()
                        .is_none_or(|own| own.text != r.text || own.advisory != r.advisory)
                })
            {
                return Err(error("DM_STALE: reused decision authority changed or was revoked; reconcile before execution"));
            }
            root = parent;
        }
        if p.trust_mode != "secure_approval" {
            return Ok(());
        }
        let d = &root;
        let Some(r) = &d.resolution else {
            return Ok(());
        };
        if !r.origin.starts_with("human") {
            return Ok(());
        }
        let proof=r.attestation.as_ref().ok_or_else(||error("Historic local-trust guidance requires independently verified renewal before secure execution"))?;
        let mut previous = d.clone();
        previous.version = previous.version.saturating_sub(1);
        let input = AnswerInput {
            text: r.text.clone(),
            option: r.selected_option_id.clone(),
            advisory: r.advisory,
            apply_patch: r.patch_applied,
            fields: r.fields.clone(),
            outcome: serde_json::from_value(serde_json::json!(if r.outcome.is_empty() {
                "answer"
            } else {
                &r.outcome
            }))
            .map_err(|e| error(e.to_string()))?,
            attestation: None,
        };
        let at = chrono::DateTime::parse_from_rfc3339(&r.recorded_at)
            .map_err(|e| error(e.to_string()))?
            .timestamp();
        verify_attestation(
            &self.root,
            p,
            proof,
            "decision.answer",
            &digest(&answer_challenge(&previous, &input, p)),
            at,
        )?;
        Ok(())
    }
}

pub fn graph_challenge(
    plan: &Plan,
    patch: &Patch,
    source_digest: Option<&str>,
    policy: &Policy,
) -> serde_json::Value {
    serde_json::json!({"action":"graph.mutate","plan_id":plan.id,"base_graph_version":plan.graph_version,"patch":patch,"source_digest":source_digest,"policy_digest":digest(policy)})
}
impl Store {
    pub(super) fn graph_authority(
        &self,
        plan: &Plan,
        patch: &Patch,
        source: Option<&str>,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> crate::Result<String> {
        let p = policy(&self.root)?;
        if p.trust_mode != "secure_approval" {
            return Ok(actor.into());
        }
        verify_attestation(&self.root,&p,proof.ok_or_else(||error("Secure graph changes require an isolated attestation; preview an exact graph challenge first"))?,"graph.mutate",&digest(&graph_challenge(plan,patch,source,&p)),chrono::Utc::now().timestamp())
    }
}
pub(super) fn transport_authority(
    p: &Policy,
    requested: &Path,
    principal: &str,
) -> crate::Result<()> {
    if p.trust_mode != "secure_approval" {
        return Ok(());
    }
    let r = registry(p)?;
    if requested != r.allowed_signers
        || !r.roles.get(principal).is_some_and(|roles| {
            r.required_roles
                .get("transport.import")
                .is_some_and(|required| roles.iter().any(|role| required.contains(role)))
        })
    {
        return Err(error(
            "Secure transport requires a publisher entitled by the protected isolated registry",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_proof_checks_entitlement_expiry_subject_and_signature() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .status()
            .unwrap();
        let key = root.join("key");
        assert!(Command::new("/usr/bin/ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .status()
            .unwrap()
            .success());
        let signers = root.join("signers");
        std::fs::write(
            &signers,
            format!(
                "approver {}",
                std::fs::read_to_string(key.with_extension("pub")).unwrap()
            ),
        )
        .unwrap();
        let registry = TrustRegistry {
            allowed_signers: signers,
            roles: BTreeMap::from([("approver".into(), vec!["OWNER".into()])]),
            required_roles: BTreeMap::from([("decision.answer".into(), vec!["OWNER".into()])]),
        };
        let body = AttestationBody {
            principal: "approver".into(),
            role: "OWNER".into(),
            action: "decision.answer".into(),
            action_digest: "subject-and-answer".into(),
            nonce: uuid::Uuid::new_v4().to_string(),
            issued_at: 1000,
            expires_at: 1060,
        };
        let file = root.join("body.json");
        std::fs::write(&file, canonical_bytes(&body).unwrap()).unwrap();
        assert!(Command::new("/usr/bin/ssh-keygen")
            .args(["-Y", "sign", "-n", "macc-approval", "-f"])
            .arg(&key)
            .arg(&file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        let proof = Attestation {
            body,
            signature: std::fs::read_to_string(file.with_extension("json.sig")).unwrap(),
        };
        assert_eq!(
            verify_bound_attestation(
                root,
                &registry,
                &proof,
                "decision.answer",
                "subject-and-answer",
                1030
            )
            .unwrap(),
            "approver"
        );
        assert!(verify_bound_attestation(
            root,
            &registry,
            &proof,
            "decision.answer",
            "other-subject",
            1030
        )
        .is_err());
        assert!(verify_bound_attestation(
            root,
            &registry,
            &proof,
            "decision.answer",
            "subject-and-answer",
            1061
        )
        .is_err());
        let mut extreme = proof.clone();
        extreme.body.issued_at = i64::MIN;
        extreme.body.expires_at = i64::MAX;
        assert!(verify_bound_attestation(
            root,
            &registry,
            &extreme,
            "decision.answer",
            "subject-and-answer",
            1030
        )
        .is_err());
        let mut forged = proof.clone();
        forged.body.role = "ADMIN".into();
        assert!(verify_bound_attestation(
            root,
            &registry,
            &forged,
            "decision.answer",
            "subject-and-answer",
            1030
        )
        .is_err());
        forged = proof.clone();
        forged.body.principal = "attacker".into();
        assert!(verify_bound_attestation(
            root,
            &registry,
            &forged,
            "decision.answer",
            "subject-and-answer",
            1030
        )
        .is_err());
    }
}

/// Compact UTF-8 JSON with recursively sorted object keys, shared with publishers.
pub fn canonical_bytes(value: &impl Serialize) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(&serde_json::to_value(value).map_err(|e| error(e.to_string()))?)
        .map_err(|e| error(e.to_string()))
}
