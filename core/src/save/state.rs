//! Saved-state comparison includes logical SQLite content, independent of WAL layout.
use super::*;

pub fn is_macc_state_unsaved(paths: &ProjectPaths) -> Result<bool> {
    if !paths.config_path.exists() {
        return Ok(crate::session_store::is_sqlite(&paths.root)?
            || crate::coordinator::decisions::current_state_digest(&paths.root)?.is_some());
    }
    let saves = detect_matching_saves(paths)?;
    let saves: Vec<_> = saves
        .into_iter()
        .filter(|(_, strength)| *strength >= MatchStrength::Medium)
        .collect();
    if saves.is_empty() {
        return Ok(true);
    }

    let decision_digest = crate::coordinator::decisions::current_state_digest(&paths.root)?;
    let config_sha = compute_file_sha256(&paths.config_path).ok();
    let sessions_path = paths.macc_dir.join("state").join("tool-sessions.json");
    let sqlite = crate::session_store::is_sqlite(&paths.root)?;
    let sessions_sha = if sqlite {
        Some(crate::session_store::fingerprint::current(&paths.root)?)
    } else if sessions_path.exists() {
        Some(
            compute_file_sha256(&sessions_path)
                .map_err(|e| MaccError::Validation(e.to_string()))?,
        )
    } else {
        None
    };

    for (m, _) in &saves {
        let config_matches = match (&config_sha, &m.hashes.config) {
            (Some(c), Some(m_c)) => c == m_c,
            (None, None) => true,
            _ => false,
        };
        let saved = if sqlite {
            m.session_store.as_ref().map(|s| &s.state_sha256)
        } else if m.session_store.is_none() {
            m.hashes.coordinator_sessions.as_ref()
        } else {
            None
        };
        let sessions_matches = match (sessions_sha.as_ref(), saved) {
            (Some(s), Some(m_s)) => s == m_s,
            (None, None) => true,
            _ => false,
        };
        let decisions_match = decision_digest.as_deref()
            == m.decision_store.as_ref().map(|s| s.state_digest.as_str());
        if config_matches && sessions_matches && decisions_match {
            return Ok(false); // Found a match, so it's not unsaved
        }
    }

    Ok(true)
}

pub fn get_unsaved_state_report(paths: &ProjectPaths) -> Result<UnsavedStateReport> {
    let mut report = UnsavedStateReport {
        config_changed: false,
        sessions_changed: false,
    };
    if !paths.config_path.exists() {
        report.sessions_changed = crate::session_store::is_sqlite(&paths.root)?
            || crate::coordinator::decisions::current_state_digest(&paths.root)?.is_some();
        return Ok(report);
    }

    let decision_digest = crate::coordinator::decisions::current_state_digest(&paths.root)?;
    let config_sha = compute_file_sha256(&paths.config_path).ok();
    let sessions_path = paths.macc_dir.join("state").join("tool-sessions.json");
    let sqlite = crate::session_store::is_sqlite(&paths.root)?;
    let sessions_sha = if sqlite {
        Some(crate::session_store::fingerprint::current(&paths.root)?)
    } else if sessions_path.exists() {
        Some(
            compute_file_sha256(&sessions_path)
                .map_err(|e| MaccError::Validation(e.to_string()))?,
        )
    } else {
        None
    };

    let matches = detect_matching_saves(paths)?;
    let matches: Vec<_> = matches
        .into_iter()
        .filter(|(_, strength)| *strength >= MatchStrength::Medium)
        .collect();
    if matches.is_empty() {
        report.config_changed = true;
        if sessions_path.exists() || decision_digest.is_some() {
            report.sessions_changed = true;
        }
        return Ok(report);
    }

    let best_match = &matches[0].0;
    let config_matches = match (&config_sha, &best_match.hashes.config) {
        (Some(c), Some(m_c)) => c == m_c,
        (None, None) => true,
        _ => false,
    };
    let saved = if sqlite {
        best_match.session_store.as_ref().map(|s| &s.state_sha256)
    } else if best_match.session_store.is_none() {
        best_match.hashes.coordinator_sessions.as_ref()
    } else {
        None
    };
    let sessions_matches = match (sessions_sha.as_ref(), saved) {
        (Some(s), Some(m_s)) => s == m_s,
        (None, None) => true,
        _ => false,
    };

    report.config_changed = !config_matches;
    report.sessions_changed = !sessions_matches
        || decision_digest.as_deref()
            != best_match
                .decision_store
                .as_ref()
                .map(|s| s.state_digest.as_str());

    Ok(report)
}
