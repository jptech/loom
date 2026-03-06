use std::path::PathBuf;

use crate::manifest::test::{TestDecl, TestSuiteDecl};
use crate::resolve::resolver::ResolvedProject;

/// A test discovered from a component manifest, tagged with its source component.
#[derive(Debug, Clone)]
pub struct DiscoveredTest {
    /// The test declaration from the component manifest.
    pub test: TestDecl,
    /// Name of the component that declares this test.
    pub component_name: String,
    /// Path to the component's source directory.
    pub component_path: PathBuf,
}

/// Collect all `[[tests]]` from resolved components.
///
/// Walks the resolved dependency tree and extracts test declarations from each
/// component's manifest, tagging each with its source component name and path.
pub fn discover_tests(resolved: &ResolvedProject) -> Vec<DiscoveredTest> {
    let mut tests = Vec::new();

    for comp in &resolved.resolved_components {
        for test in &comp.manifest.tests {
            tests.push(DiscoveredTest {
                test: test.clone(),
                component_name: comp.manifest.component.name.clone(),
                component_path: comp.source_path.clone(),
            });
        }
    }

    tests
}

/// Resolve which tests belong to a named test suite.
///
/// A test matches a suite if:
/// - Its name appears in `suite.tests`, OR
/// - Any of its tags appear in `suite.tags`, OR
/// - Its component name appears in `suite.components`
pub fn resolve_suite<'a>(
    suite: &TestSuiteDecl,
    all_tests: &'a [DiscoveredTest],
) -> Vec<&'a DiscoveredTest> {
    all_tests
        .iter()
        .filter(|dt| {
            // Match by explicit test name
            if suite.tests.contains(&dt.test.name) {
                return true;
            }
            // Match by tag
            if !suite.tags.is_empty() && dt.test.tags.iter().any(|tag| suite.tags.contains(tag)) {
                return true;
            }
            // Match by component
            if suite.components.contains(&dt.component_name) {
                return true;
            }
            false
        })
        .collect()
}

/// Filter tests by a glob-like pattern against test names.
///
/// Supports `*` wildcards (e.g., `axi_*`, `*_loopback`, `*stress*`).
pub fn filter_tests<'a>(tests: &'a [DiscoveredTest], pattern: &str) -> Vec<&'a DiscoveredTest> {
    let regex_pattern = format!(
        "^{}$",
        pattern
            .replace('.', "\\.")
            .replace('*', ".*")
            .replace('?', ".")
    );
    let re = regex::Regex::new(&regex_pattern);

    match re {
        Ok(re) => tests
            .iter()
            .filter(|dt| re.is_match(&dt.test.name))
            .collect(),
        Err(_) => {
            // Fall back to exact prefix matching if regex is invalid
            tests
                .iter()
                .filter(|dt| dt.test.name.starts_with(pattern))
                .collect()
        }
    }
}

/// Filter tests to only those from a specific component.
pub fn filter_by_component<'a>(
    tests: &'a [DiscoveredTest],
    component_name: &str,
) -> Vec<&'a DiscoveredTest> {
    tests
        .iter()
        .filter(|dt| dt.component_name == component_name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::test::TestDecl;

    fn make_test(name: &str, tags: &[&str], component: &str) -> DiscoveredTest {
        DiscoveredTest {
            test: TestDecl {
                name: name.to_string(),
                top: format!("tb_{}", name),
                description: None,
                timeout_seconds: None,
                tags: tags.iter().map(|t| t.to_string()).collect(),
                requires: None,
                sim_options: None,
                dependencies: Default::default(),
                runner: None,
                sources: vec![],
            },
            component_name: component.to_string(),
            component_path: PathBuf::from(format!("/components/{}", component)),
        }
    }

    #[test]
    fn test_filter_wildcard() {
        let tests = vec![
            make_test("axi_read", &["smoke"], "axi"),
            make_test("axi_write", &["smoke"], "axi"),
            make_test("uart_loopback", &["regression"], "uart"),
        ];

        let filtered = filter_tests(&tests, "axi_*");
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|t| t.test.name.starts_with("axi_")));
    }

    #[test]
    fn test_filter_exact() {
        let tests = vec![
            make_test("axi_read", &[], "axi"),
            make_test("axi_write", &[], "axi"),
        ];

        let filtered = filter_tests(&tests, "axi_read");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].test.name, "axi_read");
    }

    #[test]
    fn test_resolve_suite_by_tags() {
        let tests = vec![
            make_test("test_a", &["smoke", "regression"], "comp_a"),
            make_test("test_b", &["regression"], "comp_b"),
            make_test("test_c", &["nightly"], "comp_c"),
        ];

        let suite = TestSuiteDecl {
            description: None,
            tags: vec!["smoke".to_string()],
            components: vec![],
            tests: vec![],
        };

        let matched = resolve_suite(&suite, &tests);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].test.name, "test_a");
    }

    #[test]
    fn test_resolve_suite_by_component() {
        let tests = vec![
            make_test("test_a", &[], "comp_a"),
            make_test("test_b", &[], "comp_b"),
        ];

        let suite = TestSuiteDecl {
            description: None,
            tags: vec![],
            components: vec!["comp_b".to_string()],
            tests: vec![],
        };

        let matched = resolve_suite(&suite, &tests);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].test.name, "test_b");
    }

    #[test]
    fn test_resolve_suite_by_name() {
        let tests = vec![
            make_test("test_a", &[], "comp_a"),
            make_test("test_b", &[], "comp_b"),
            make_test("test_c", &[], "comp_c"),
        ];

        let suite = TestSuiteDecl {
            description: None,
            tags: vec![],
            components: vec![],
            tests: vec!["test_a".to_string(), "test_c".to_string()],
        };

        let matched = resolve_suite(&suite, &tests);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn test_filter_by_component() {
        let tests = vec![
            make_test("test_a", &[], "comp_a"),
            make_test("test_b", &[], "comp_b"),
            make_test("test_c", &[], "comp_a"),
        ];

        let filtered = filter_by_component(&tests, "comp_a");
        assert_eq!(filtered.len(), 2);
    }
}
