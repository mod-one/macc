pub fn parse_size_to_bytes(s: &str) -> u64 {
    let s = s.trim().to_ascii_uppercase();
    if s.is_empty() {
        return 50 * 1024 * 1024; // Default fallback
    }
    
    let (num_part, multiplier) = if s.ends_with("GB") {
        (&s[..s.len() - 2], 1024 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1024 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1024)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };
    
    match num_part.trim().parse::<u64>() {
        Ok(val) => val * multiplier,
        Err(_) => 50 * 1024 * 1024, // Fallback
    }
}

pub fn parse_duration_to_seconds(s: &str) -> u64 {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return 7 * 24 * 3600; // Default fallback (7 days)
    }
    
    let (num_part, multiplier) = if s.ends_with('d') {
        (&s[..s.len() - 1], 24 * 3600)
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 3600)
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 60)
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };
    
    match num_part.trim().parse::<u64>() {
        Ok(val) => val * multiplier,
        Err(_) => 7 * 24 * 3600, // Fallback
    }
}
