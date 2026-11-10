use std::collections::BTreeMap;
pub fn render_shared_agents_md(
    _enabled_tools: &[String],
    inline: &BTreeMap<String, String>,
    path: Option<&str>,
) -> String {
    let mut resolved = macc_core::resolve::resolve(
        &macc_core::config::CanonicalConfig::default(),
        &Default::default(),
    );
    resolved.standards.inline = inline.clone();
    resolved.standards.path = path.map(str::to_owned);
    macc_core::project_context::render_standards(&resolved)
}
pub fn orchestrate_agents_md(
    resolved: &macc_core::resolve::ResolvedConfig,
    sections: &[macc_core::tool::ProjectContextSection],
) -> String {
    let mut text = macc_core::project_context::render_standards(resolved);
    for section in sections {
        text.push_str(&format!(
            "\n## {}\n{}\n",
            section.heading,
            section.content.trim()
        ));
    }
    text
}
