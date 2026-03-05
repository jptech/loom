use std::collections::HashMap;
use std::path::Path;

use regex::Regex;

use crate::error::LoomError;

/// Context for constraint template preprocessing.
#[derive(Debug, Clone)]
pub struct TemplateContext {
    /// Project parameters (from project.toml).
    pub project: HashMap<String, toml::Value>,
    /// Component metadata.
    pub component: HashMap<String, toml::Value>,
}

impl TemplateContext {
    pub fn new() -> Self {
        Self {
            project: HashMap::new(),
            component: HashMap::new(),
        }
    }

    /// Look up a dotted path like "project.name" or "component.version".
    pub fn resolve(&self, path: &str) -> Option<String> {
        let parts: Vec<&str> = path.splitn(2, '.').collect();
        if parts.len() != 2 {
            return None;
        }

        let (scope, key) = (parts[0], parts[1]);
        let map = match scope {
            "project" => &self.project,
            "component" => &self.component,
            _ => return None,
        };

        map.get(key).map(value_to_string)
    }
}

impl Default for TemplateContext {
    fn default() -> Self {
        Self::new()
    }
}

fn value_to_string(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(n) => n.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// Check if a path has a template extension (.xdc.tpl, .sdc.tpl, etc.).
pub fn is_template_file(path: &Path) -> bool {
    let name = path.to_string_lossy();
    name.ends_with(".xdc.tpl")
        || name.ends_with(".sdc.tpl")
        || name.ends_with(".lpf.tpl")
        || name.ends_with(".pcf.tpl")
}

/// Get the output name for a template (remove .tpl extension).
pub fn template_output_name(path: &Path) -> String {
    let name = path.to_string_lossy();
    name.trim_end_matches(".tpl").to_string()
}

/// Preprocess a constraint template file, replacing {{...}} variables.
pub fn preprocess_constraint_template(
    template_path: &Path,
    context: &TemplateContext,
    output_path: &Path,
) -> Result<(), LoomError> {
    let content = std::fs::read_to_string(template_path).map_err(|e| LoomError::Io {
        path: template_path.to_owned(),
        source: e,
    })?;

    let processed = replace_template_vars(&content, context)?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LoomError::Io {
            path: parent.to_owned(),
            source: e,
        })?;
    }

    std::fs::write(output_path, processed).map_err(|e| LoomError::Io {
        path: output_path.to_owned(),
        source: e,
    })
}

/// Replace all {{...}} patterns in the input string with values from context.
fn replace_template_vars(input: &str, ctx: &TemplateContext) -> Result<String, LoomError> {
    let re = Regex::new(r"\{\{([^}]+)\}\}").unwrap();
    let mut result = String::with_capacity(input.len());
    let mut last_end = 0;

    for cap in re.captures_iter(input) {
        let full_match = cap.get(0).unwrap();
        let var_name = cap[1].trim();

        result.push_str(&input[last_end..full_match.start()]);

        match ctx.resolve(var_name) {
            Some(value) => result.push_str(&value),
            None => {
                return Err(LoomError::Internal(format!(
                    "Template variable '{{{{{}}}}}' not found in context",
                    var_name
                )));
            }
        }

        last_end = full_match.end();
    }

    result.push_str(&input[last_end..]);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context() -> TemplateContext {
        let mut ctx = TemplateContext::new();
        ctx.project.insert(
            "name".to_string(),
            toml::Value::String("radar_top".to_string()),
        );
        ctx.project.insert(
            "clock_freq".to_string(),
            toml::Value::String("100.0".to_string()),
        );
        ctx.component.insert(
            "version".to_string(),
            toml::Value::String("1.0.0".to_string()),
        );
        ctx
    }

    #[test]
    fn test_simple_substitution() {
        let ctx = make_context();
        let input = "create_clock -period {{project.clock_freq}} [get_ports clk]";
        let result = replace_template_vars(input, &ctx).unwrap();
        assert_eq!(result, "create_clock -period 100.0 [get_ports clk]");
    }

    #[test]
    fn test_multiple_substitutions() {
        let ctx = make_context();
        let input = "# {{project.name}} v{{component.version}}";
        let result = replace_template_vars(input, &ctx).unwrap();
        assert_eq!(result, "# radar_top v1.0.0");
    }

    #[test]
    fn test_missing_variable_error() {
        let ctx = make_context();
        let input = "value = {{project.nonexistent}}";
        let result = replace_template_vars(input, &ctx);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not found in context"));
    }

    #[test]
    fn test_no_template_vars() {
        let ctx = make_context();
        let input = "create_clock -period 10 [get_ports clk]";
        let result = replace_template_vars(input, &ctx).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_whitespace_in_var_name() {
        let ctx = make_context();
        let input = "{{ project.name }}";
        let result = replace_template_vars(input, &ctx).unwrap();
        assert_eq!(result, "radar_top");
    }

    #[test]
    fn test_is_template_file() {
        assert!(is_template_file(std::path::Path::new("timing.xdc.tpl")));
        assert!(is_template_file(std::path::Path::new("io.sdc.tpl")));
        assert!(is_template_file(std::path::Path::new("pins.lpf.tpl")));
        assert!(is_template_file(std::path::Path::new("pins.pcf.tpl")));
        assert!(!is_template_file(std::path::Path::new("timing.xdc")));
        assert!(!is_template_file(std::path::Path::new("file.tpl")));
    }

    #[test]
    fn test_template_output_name() {
        let name = template_output_name(std::path::Path::new("timing.xdc.tpl"));
        assert_eq!(name, "timing.xdc");
    }

    #[test]
    fn test_preprocess_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let template_path = tmp.path().join("timing.xdc.tpl");
        let output_path = tmp.path().join("timing.xdc");

        std::fs::write(
            &template_path,
            "create_clock -period {{project.clock_freq}} [get_ports clk]\n",
        )
        .unwrap();

        let ctx = make_context();
        preprocess_constraint_template(&template_path, &ctx, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content, "create_clock -period 100.0 [get_ports clk]\n");
    }
}
