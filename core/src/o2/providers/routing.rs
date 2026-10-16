//! A healthy binary is insufficient: external cached code must match its receipt and source.
use super::*;
pub(super) fn cached_compatible(
    root: &Path,
    target: &Path,
    descriptor: &Descriptor,
    receipt: &Receipt,
    policy: &super::super::policy::CodePolicy,
) -> Result<bool> {
    if receipt.version != descriptor.version {
        return Ok(false);
    }
    let (Some(path), Some(hash), Some(snapshot), Some(revision)) = (
        &receipt.index,
        &receipt.index_sha256,
        &receipt.snapshot_digest,
        &receipt.source_revision,
    ) else {
        return Ok(false);
    };
    let valid = lifecycle::owned_path(root, &descriptor.id, path)
        .and_then(|p| store::bytes(&p))
        .is_ok_and(|raw| digest(&raw) == *hash);
    if !valid {
        return Ok(false);
    }
    let source = native::snapshot(target, policy)?;
    Ok(source.digest == *snapshot && source.revision == *revision)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_requires_current_version_artifact_and_dirty_source_fingerprint() {
        let temp = crate::o2::tests::root();
        let root = temp.path();
        let policy = super::super::super::policy::load(root).unwrap();
        for op in ["deploy", "index"] {
            let Capability::Supported(plan) = plan(root, "native", op).unwrap() else {
                panic!()
            };
            apply(
                root,
                &plan.id,
                &digest(serde_json::to_string(&plan).unwrap().as_bytes()),
            )
            .unwrap();
        }
        let mut receipt = inspect(root).unwrap().receipts.remove(0);
        let descriptor = native::Native.descriptor();
        assert!(
            cached_compatible(root, root, &descriptor, &receipt, &policy.code_intelligence)
                .unwrap()
        );
        receipt.version = "wrong".into();
        assert!(
            !cached_compatible(root, root, &descriptor, &receipt, &policy.code_intelligence)
                .unwrap()
        );
        receipt.version = descriptor.version.clone();
        std::fs::write(root.join("contract.rs"), "dirty source").unwrap();
        assert!(
            !cached_compatible(root, root, &descriptor, &receipt, &policy.code_intelligence)
                .unwrap()
        );
        std::fs::write(root.join("contract.rs"), "pub fn preserve_contract() {}\n").unwrap();
        assert!(
            cached_compatible(root, root, &descriptor, &receipt, &policy.code_intelligence)
                .unwrap()
        );
        std::fs::write(
            root.join(receipt.index.as_ref().unwrap()),
            "corrupt artifact",
        )
        .unwrap();
        assert!(
            !cached_compatible(root, root, &descriptor, &receipt, &policy.code_intelligence)
                .unwrap()
        );
        // An absent snapshot receipt from older schemas is never assumed current.
        receipt.snapshot_digest = None;
        assert!(
            !cached_compatible(root, root, &descriptor, &receipt, &policy.code_intelligence)
                .unwrap()
        );
    }
}
