pub const REQUIRED_SKILLS: [&str; 3] = ["macc-performer", "macc-reviewer", "macc-prd-planner"];

pub fn required_skills() -> &'static [&'static str] {
    &REQUIRED_SKILLS
}

pub fn is_required_skill(id: &str) -> bool {
    REQUIRED_SKILLS.contains(&id)
}
