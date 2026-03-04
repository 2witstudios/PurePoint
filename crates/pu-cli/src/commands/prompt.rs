use crate::error::CliError;
use pu_core::template;

pub async fn run_list(json: bool) -> Result<(), CliError> {
    let cwd = std::env::current_dir()?;
    let templates = template::list_templates(&cwd);

    if json {
        println!("{}", serde_json::to_string_pretty(&templates).unwrap());
        return Ok(());
    }

    if templates.is_empty() {
        println!("No templates found");
        println!("  Local:  .pu/templates/*.md");
        println!("  Global: ~/.pu/templates/*.md");
        return Ok(());
    }

    for tpl in &templates {
        let desc = if tpl.description.is_empty() {
            String::new()
        } else {
            format!(" — {}", tpl.description)
        };
        let vars = template::extract_variables(&tpl.body);
        let var_str = if vars.is_empty() {
            String::new()
        } else {
            format!(" [{}]", vars.join(", "))
        };
        println!("  {} ({}){}{}", tpl.name, tpl.source, desc, var_str);
    }

    Ok(())
}
