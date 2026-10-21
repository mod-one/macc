//! Metadata only: never clone, install or execute a remote skill.
use super::*;
use std::{
    path::{Component, Path},
    process::Command,
};
trait Metadata: Sync {
    fn get(&self, url: &str) -> Option<(u16, serde_json::Value)>;
}
#[derive(Default)]
pub(super) struct Curl;
#[derive(Default)]
pub(super) struct Github<T = Curl> {
    refs: Mutex<BTreeMap<String, bool>>,
    transport: T,
}
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
impl Metadata for Curl {
    fn get(&self, url: &str) -> Option<(u16, serde_json::Value)> {
        let output = Command::new("curl")
            .args([
                "--disable",
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--connect-timeout",
                "3",
                "--max-time",
                "8",
                "--max-filesize",
                "262144",
                "-H",
                "Accept: application/vnd.github+json",
                "-H",
                "User-Agent: MACC-catalog-check",
                "--write-out",
                "\n%{http_code}",
                url,
            ])
            .output()
            .ok()?;
        if !output.status.success() || output.stdout.len() > 262200 {
            return None;
        }
        let text = String::from_utf8(output.stdout).ok()?;
        let (body, status) = text.rsplit_once('\n')?;
        Some((status.parse().ok()?, serde_json::from_str(body).ok()?))
    }
}
impl<T: Metadata> Checker for Github<T> {
    fn check(&self, root: &Path, e: &SkillEntry, now: i64) -> Availability {
        if e.source.kind == SourceKind::Local {
            return Local.check(root, e, now);
        }
        let unknown = || {
            Availability::new(State::Unknown,"Source could not be verified (network, access policy, rate limit or unsupported transport)",now)
        };
        let Some(repo) = e
            .source
            .url
            .strip_prefix("https://github.com/")
            .map(|s| s.trim_end_matches('/'))
            .map(|s| s.strip_suffix(".git").unwrap_or(s))
        else {
            return unknown();
        };
        if e.source.kind != SourceKind::Git
            || repo.split('/').count() != 2
            || repo.split('/').any(|s| {
                s.is_empty()
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
        {
            return unknown();
        }
        if Path::new(&e.selector.subpath)
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Availability::new(State::Unavailable, "Invalid catalog subpath", now);
        }
        let subpath = e
            .selector
            .subpath
            .split('/')
            .map(encode)
            .collect::<Vec<_>>()
            .join("/");
        let reference = encode(&e.source.reference);
        let base = format!("https://api.github.com/repos/{repo}");
        let Some((status, body)) = self
            .transport
            .get(&format!("{base}/contents/{subpath}?ref={reference}"))
        else {
            return unknown();
        };
        if status == 200 {
            let Some(files) = body.as_array() else {
                return Availability::new(State::Unknown, "Invalid GitHub directory metadata", now);
            };
            if files.iter().any(|file| file["name"].as_str().is_none()) {
                return unknown();
            }
            let marker = files.iter().any(|file| {
                file["name"].as_str().is_some_and(|name| {
                    crate::packages::SKILL_MARKERS.contains(&name) || name == "macc.package.json"
                })
            });
            if !marker && files.len() >= 1000 {
                return Availability::new(
                    State::Unknown,
                    "Directory listing may be truncated; entry point cannot be ruled out",
                    now,
                );
            }
            return Availability::new(
                if marker {
                    State::Available
                } else {
                    State::Unavailable
                },
                if marker {
                    "Source and skill entry point verified"
                } else {
                    "Directory has no supported skill entry point"
                },
                now,
            );
        }
        if status == 404 {
            let ref_key = format!("{base}@{reference}");
            let known = self.refs.lock().unwrap().get(&ref_key).copied();
            let exists = known.unwrap_or_else(|| {
                let exists = self
                    .transport
                    .get(&format!("{base}/commits/{reference}"))
                    .is_some_and(|(status, body)| status == 200 && body["sha"].as_str().is_some());
                self.refs.lock().unwrap().insert(ref_key, exists);
                exists
            });
            if exists {
                return Availability::new(
                    State::Unavailable,
                    "Declared path is absent in an accessible source reference",
                    now,
                );
            }
        }
        let reason = match status {
            401 => "GitHub authentication required (HTTP 401)",
            403 => "GitHub access denied or API limit reached (HTTP 403)",
            429 => "GitHub API rate limit reached (HTTP 429)",
            404 => "Repository/reference is not publicly readable; existence cannot be confirmed (HTTP 404)",
            _ => "GitHub metadata request failed; source availability is unverified",
        };
        Availability::new(State::Unknown, reason, now)
    }
}
pub(super) struct Local;
impl Checker for Local {
    fn check(&self, root: &Path, e: &SkillEntry, now: i64) -> Availability {
        let source = Path::new(&e.source.url);
        let source = if source.is_absolute() {
            source.to_path_buf()
        } else {
            root.join(source)
        };
        if Path::new(&e.selector.subpath)
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Availability::new(State::Unavailable, "Invalid local subpath", now);
        }
        let path = source.join(&e.selector.subpath);
        match std::fs::metadata(&path) {
            Ok(m) if m.is_dir() => {
                let mut unreadable = false;
                for marker in crate::packages::SKILL_MARKERS
                    .iter()
                    .copied()
                    .chain(std::iter::once("macc.package.json"))
                {
                    match std::fs::metadata(path.join(marker)) {
                        Ok(metadata) if metadata.is_file() => {
                            return Availability::new(
                                State::Available,
                                "Local skill entry point verified",
                                now,
                            )
                        }
                        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                            unreadable = true
                        }
                        _ => {}
                    }
                }
                if unreadable {
                    Availability::new(
                        State::Unknown,
                        "Local skill entry point cannot be inspected",
                        now,
                    )
                } else {
                    Availability::new(
                        State::Unavailable,
                        "Local directory has no supported skill entry point",
                        now,
                    )
                }
            }
            Ok(_) => Availability::new(
                State::Unknown,
                "Local source requires a supported skill directory",
                now,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Availability::new(State::Unavailable, "Local skill directory is absent", now)
            }
            Err(_) => Availability::new(State::Unknown, "Local source cannot be inspected", now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Stub {
        path: Option<(u16, serde_json::Value)>,
        reference: u16,
    }
    impl Metadata for Stub {
        fn get(&self, url: &str) -> Option<(u16, serde_json::Value)> {
            if url.contains("/contents/") {
                self.path.clone()
            } else {
                Some((self.reference, serde_json::json!({"sha":"fixture"})))
            }
        }
    }
    fn check(path: Option<(u16, serde_json::Value)>, reference: u16) -> State {
        let entry:SkillEntry=serde_json::from_value(serde_json::json!({"id":"fixture","name":"Fixture","description":"","tags":[],"selector":{"subpath":"skills/test"},"source":{"kind":"git","url":"https://github.com/example/repo.git","ref":"main"}})).unwrap();
        Github {
            refs: Mutex::default(),
            transport: Stub { path, reference },
        }
        .check(Path::new("."), &entry, 1)
        .state
    }
    #[test]
    fn absence_requires_accessible_reference_and_transient_failures_are_unknown() {
        assert_eq!(
            check(Some((404, serde_json::json!({}))), 200),
            State::Unavailable
        );
        for reference in [404, 403, 429, 503] {
            assert_eq!(
                check(Some((404, serde_json::json!({}))), reference),
                State::Unknown
            );
        }
        for status in [403, 429, 503] {
            assert_eq!(
                check(Some((status, serde_json::json!({}))), 200),
                State::Unknown
            );
        }
        assert_eq!(check(None, 200), State::Unknown);
        assert_eq!(
            check(Some((200, serde_json::Value::Null)), 200),
            State::Unknown
        );
    }
    #[test]
    fn valid_skill_or_bundle_entrypoint_is_required() {
        for name in crate::packages::SKILL_MARKERS
            .iter()
            .copied()
            .chain(std::iter::once("macc.package.json"))
        {
            assert_eq!(
                check(Some((200, serde_json::json!([{"name":name}]))), 200),
                State::Available
            );
        }
        assert_eq!(
            check(
                Some((200, serde_json::json!([{"name":"example.txt"}]))),
                200
            ),
            State::Unavailable
        );
    }
}
