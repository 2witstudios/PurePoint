use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_agent")]
    pub agent: String,
    pub body: String,
    /// Where this template was loaded from ("local" or "global")
    #[serde(skip_deserializing)]
    pub source: String,
}

fn default_agent() -> String {
    String::new()
}

/// Parse a template from file content. Expects optional YAML frontmatter delimited by `---`.
pub fn parse_template(content: &str, file_name: &str) -> Template {
    let stem = file_name.strip_suffix(".md").unwrap_or(file_name);

    if let Some(rest) = content.strip_prefix("---\n") {
        // Find closing --- (with or without trailing newline for EOF case)
        let (yaml, body) = if let Some(end) = rest.find("\n---\n") {
            (&rest[..end], rest[end + 5..].trim_start().to_string())
        } else if let Some(stripped) = rest.strip_suffix("\n---") {
            (stripped, String::new())
        } else {
            ("", String::new()) // triggers fallthrough below
        };
        if !yaml.is_empty() {
            #[derive(Deserialize)]
            struct FrontMatter {
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                description: Option<String>,
                #[serde(default)]
                agent: Option<String>,
            }

            if let Ok(fm) = serde_yml::from_str::<FrontMatter>(yaml) {
                return Template {
                    name: fm.name.unwrap_or_else(|| stem.to_string()),
                    description: fm.description.unwrap_or_default(),
                    agent: fm.agent.unwrap_or_else(default_agent),
                    body,
                    source: String::new(),
                };
            }
        }
    }

    // No frontmatter — whole content is the body
    Template {
        name: stem.to_string(),
        description: String::new(),
        agent: default_agent(),
        body: content.to_string(),
        source: String::new(),
    }
}

/// Scan both local and global template directories. Local templates take priority.
pub fn list_templates(project_root: &Path) -> Vec<Template> {
    let mut seen = HashMap::new();
    let mut result = Vec::new();

    // Local first
    let local_dir = paths::templates_dir(project_root);
    if local_dir.is_dir() {
        for tpl in scan_dir(&local_dir, "local") {
            seen.insert(tpl.name.clone(), result.len());
            result.push(tpl);
        }
    }

    // Global second (skip duplicates)
    if let Ok(global_dir) = paths::global_templates_dir() {
        if global_dir.is_dir() {
            for tpl in scan_dir(&global_dir, "global") {
                if !seen.contains_key(&tpl.name) {
                    result.push(tpl);
                }
            }
        }
    }

    result
}

/// Find a template by name. Checks local first, then global.
pub fn find_template(project_root: &Path, name: &str) -> Option<Template> {
    let local_dir = paths::templates_dir(project_root);
    if local_dir.is_dir() {
        if let Some(tpl) = find_in_dir(&local_dir, name, "local") {
            return Some(tpl);
        }
    }
    if let Ok(global_dir) = paths::global_templates_dir() {
        if global_dir.is_dir() {
            if let Some(tpl) = find_in_dir(&global_dir, name, "global") {
                return Some(tpl);
            }
        }
    }
    None
}

/// Substitute `{{VAR}}` placeholders in the template body.
pub fn render(template: &Template, vars: &HashMap<String, String>) -> String {
    let mut result = template.body.clone();
    for (key, value) in vars {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
    }
    if result.contains("{{") {
        let remaining = extract_variables(&result);
        eprintln!(
            "warning: unsubstituted template variables: {}",
            remaining.join(", ")
        );
    }
    result
}

/// Extract all `{{VAR}}` names from a template body.
pub fn extract_variables(body: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let var = after[..end].trim().to_string();
            if !var.is_empty() && !vars.contains(&var) {
                vars.push(var);
            }
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    vars
}

/// Save a template as a markdown file with YAML frontmatter.
/// Creates the directory if it doesn't exist.
pub fn save_template(
    dir: &Path,
    name: &str,
    description: &str,
    agent: &str,
    body: &str,
) -> Result<(), std::io::Error> {
    crate::validation::validate_name(name)?;
    std::fs::create_dir_all(dir)?;
    #[derive(Serialize)]
    struct TemplateFrontmatter<'a> {
        name: &'a str,
        description: &'a str,
        agent: &'a str,
    }
    let fm = serde_yml::to_string(&TemplateFrontmatter {
        name,
        description,
        agent,
    })
    .map_err(std::io::Error::other)?;
    let content = format!("---\n{fm}---\n{body}");
    std::fs::write(dir.join(format!("{name}.md")), content)
}

/// Delete a template file. Returns true if the file existed.
pub fn delete_template(dir: &Path, name: &str) -> Result<bool, std::io::Error> {
    crate::validation::validate_name(name)?;
    let path = dir.join(format!("{name}.md"));
    if path.is_file() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn scan_dir(dir: &Path, source: &str) -> Vec<Template> {
    let mut templates = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return templates,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                let mut tpl = parse_template(&content, &file_name);
                tpl.source = source.to_string();
                templates.push(tpl);
            }
        }
    }
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    templates
}

fn find_in_dir(dir: &Path, name: &str, source: &str) -> Option<Template> {
    crate::validation::validate_name(name).ok()?;
    // Try exact file name first
    let path = dir.join(format!("{name}.md"));
    if path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let file_name = path.file_name().unwrap().to_string_lossy().to_string();
            let mut tpl = parse_template(&content, &file_name);
            tpl.source = source.to_string();
            return Some(tpl);
        }
    }
    // Scan all files and match by frontmatter name
    scan_dir(dir, source)
        .into_iter()
        .find(|tpl| tpl.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn given_template_with_frontmatter_should_parse() {
        let content = "---\nname: code-review\ndescription: Review code\nagent: codex\n---\nReview the code on branch {{BRANCH}}.\n";
        let tpl = parse_template(content, "code-review.md");
        assert_eq!(tpl.name, "code-review");
        assert_eq!(tpl.description, "Review code");
        assert_eq!(tpl.agent, "codex");
        assert_eq!(tpl.body, "Review the code on branch {{BRANCH}}.\n");
    }

    #[test]
    fn given_template_without_frontmatter_should_use_filename() {
        let content = "Just a prompt body.\n";
        let tpl = parse_template(content, "my-prompt.md");
        assert_eq!(tpl.name, "my-prompt");
        assert_eq!(tpl.agent, "");
        assert_eq!(tpl.body, "Just a prompt body.\n");
    }

    #[test]
    fn given_template_with_partial_frontmatter_should_default_missing() {
        let content = "---\ndescription: A test\n---\nBody here.\n";
        let tpl = parse_template(content, "test.md");
        assert_eq!(tpl.name, "test");
        assert_eq!(tpl.description, "A test");
        assert_eq!(tpl.agent, "");
    }

    #[test]
    fn given_vars_should_substitute_in_body() {
        let tpl = Template {
            name: "test".into(),
            description: String::new(),
            agent: "claude".into(),
            body: "Review {{BRANCH}} for {{SCOPE}}.".into(),
            source: String::new(),
        };
        let mut vars = HashMap::new();
        vars.insert("BRANCH".into(), "main".into());
        vars.insert("SCOPE".into(), "security".into());
        let rendered = render(&tpl, &vars);
        assert_eq!(rendered, "Review main for security.");
    }

    #[test]
    fn given_body_with_vars_should_extract_names() {
        let body = "Review {{BRANCH}} for {{SCOPE}}. Also {{BRANCH}}.";
        let vars = extract_variables(body);
        assert_eq!(vars, vec!["BRANCH", "SCOPE"]);
    }

    #[test]
    fn given_local_and_global_templates_should_list_local_first() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::templates_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("review.md"),
            "---\nname: review\n---\nLocal review.\n",
        )
        .unwrap();
        std::fs::write(local_dir.join("deploy.md"), "Deploy prompt.\n").unwrap();

        let templates = list_templates(root);
        assert_eq!(templates.len(), 2);
        // Sorted alphabetically
        assert_eq!(templates[0].name, "deploy");
        assert_eq!(templates[1].name, "review");
        assert_eq!(templates[0].source, "local");
    }

    #[test]
    fn given_template_name_should_find_by_name() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let local_dir = paths::templates_dir(root);
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join("review.md"),
            "---\nname: code-review\n---\nReview.\n",
        )
        .unwrap();

        let tpl = find_template(root, "code-review");
        assert!(tpl.is_some());
        assert_eq!(tpl.unwrap().name, "code-review");
    }

    #[test]
    fn given_no_templates_should_return_empty_list() {
        let tmp = TempDir::new().unwrap();
        let templates = list_templates(tmp.path());
        assert!(templates.is_empty());
    }

    #[test]
    fn given_frontmatter_without_trailing_newline_should_parse() {
        let content = "---\nname: minimal\nagent: codex\n---";
        let tpl = parse_template(content, "minimal.md");
        assert_eq!(tpl.name, "minimal");
        assert_eq!(tpl.agent, "codex");
        assert_eq!(tpl.body, "");
    }

    #[test]
    fn given_save_template_should_write_markdown_with_frontmatter() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("templates");

        // when
        save_template(
            &dir,
            "review",
            "Code review",
            "claude",
            "Review the code.\n",
        )
        .unwrap();

        // then
        let content = std::fs::read_to_string(dir.join("review.md")).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("name: review"));
        assert!(content.contains("description: Code review"));
        assert!(content.contains("agent: claude"));
        assert!(content.contains("---\nReview the code.\n"));
    }

    #[test]
    fn given_save_template_should_create_dir_if_missing() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("deep").join("nested").join("templates");
        assert!(!dir.exists());

        // when
        save_template(&dir, "test", "A test", "codex", "Body.\n").unwrap();

        // then
        assert!(dir.join("test.md").is_file());
    }

    #[test]
    fn given_saved_template_should_round_trip_through_parse() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("templates");

        // when
        save_template(
            &dir,
            "deploy",
            "Deploy to prod",
            "claude",
            "Deploy {{ENV}}.\n",
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.join("deploy.md")).unwrap();
        let tpl = parse_template(&content, "deploy.md");

        // then
        assert_eq!(tpl.name, "deploy");
        assert_eq!(tpl.description, "Deploy to prod");
        assert_eq!(tpl.agent, "claude");
        assert_eq!(tpl.body, "Deploy {{ENV}}.\n");
    }

    #[test]
    fn given_existing_template_should_delete_and_return_true() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("templates");
        save_template(&dir, "old", "Old template", "claude", "Old body.\n").unwrap();
        assert!(dir.join("old.md").is_file());

        // when
        let result = delete_template(&dir, "old").unwrap();

        // then
        assert!(result);
        assert!(!dir.join("old.md").exists());
    }

    #[test]
    fn given_nonexistent_template_should_return_false() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("templates");
        std::fs::create_dir_all(&dir).unwrap();

        // when
        let result = delete_template(&dir, "nope").unwrap();

        // then
        assert!(!result);
    }
}
