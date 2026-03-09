use crate::error::LoomError;
use crate::manifest::project::{ProfileOverlay, ProjectManifest};

use super::resolver::ResolvedProject;

/// Parsed profile selection — either simple or dimensional.
#[derive(Debug, Clone)]
pub enum ProfileSelection {
    /// A single named profile (e.g., "fast_baud").
    Simple(String),
    /// Dimensional selections (e.g., [("board", "icebreaker"), ("tier", "lite")]).
    Dimensional(Vec<(String, String)>),
}

/// Parse a `--profile` string into a `ProfileSelection`.
///
/// - `"fast_baud"` → `Simple("fast_baud")`
/// - `"board=icebreaker,tier=lite"` → `Dimensional([("board","icebreaker"), ("tier","lite")])`
pub fn parse_profile_spec(spec: &str) -> Result<ProfileSelection, LoomError> {
    if spec.contains('=') {
        let mut selections = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            let (dim, choice) = part.split_once('=').ok_or_else(|| {
                LoomError::Internal(format!(
                    "Invalid profile dimension syntax '{}': expected 'dimension=choice'",
                    part
                ))
            })?;
            selections.push((dim.trim().to_string(), choice.trim().to_string()));
        }
        if selections.is_empty() {
            return Err(LoomError::Internal(
                "Empty profile specification".to_string(),
            ));
        }
        Ok(ProfileSelection::Dimensional(selections))
    } else {
        Ok(ProfileSelection::Simple(spec.to_string()))
    }
}

/// Resolve profile overlays from a parsed selection.
/// Returns the overlays in order, plus a display label for the active profile.
fn resolve_profile_overlays<'a>(
    manifest: &'a ProjectManifest,
    selection: &ProfileSelection,
) -> Result<(Vec<&'a ProfileOverlay>, String), LoomError> {
    match selection {
        ProfileSelection::Simple(name) => {
            let overlay = manifest.profiles.get(name).ok_or_else(|| {
                let available: Vec<_> = manifest.profiles.keys().collect();
                LoomError::Internal(format!(
                    "Profile '{}' not found. Available profiles: {:?}",
                    name, available
                ))
            })?;
            Ok((vec![overlay], name.clone()))
        }
        ProfileSelection::Dimensional(selections) => {
            let mut overlays = Vec::new();
            let mut label_parts = Vec::new();

            for (dim_name, choice_name) in selections {
                let dimension = manifest.profile_dimensions.get(dim_name).ok_or_else(|| {
                    let available: Vec<_> = manifest.profile_dimensions.keys().collect();
                    LoomError::Internal(format!(
                        "Profile dimension '{}' not found. Available dimensions: {:?}",
                        dim_name, available
                    ))
                })?;

                let overlay = dimension.choices.get(choice_name).ok_or_else(|| {
                    let available: Vec<_> = dimension.choices.keys().collect();
                    LoomError::Internal(format!(
                        "Choice '{}' not found in dimension '{}'. Available choices: {:?}",
                        choice_name, dim_name, available
                    ))
                })?;

                overlays.push(overlay);
                label_parts.push(format!("{}={}", dim_name, choice_name));
            }

            Ok((overlays, label_parts.join(",")))
        }
    }
}

/// Apply a single profile overlay to the project manifest and resolved project.
fn apply_overlay(resolved: &mut ResolvedProject, overlay: &ProfileOverlay) {
    // Override platform if specified
    if let Some(ref platform) = overlay.platform {
        resolved.project.project.platform = Some(platform.clone());
    }

    // Merge params into profile_params on the resolved project
    for (key, value) in &overlay.params {
        resolved.profile_params.insert(key.clone(), value.clone());
    }

    // Override build config if specified
    if let Some(ref build_overlay) = overlay.build {
        if let Some(ref mut build) = resolved.project.build {
            if let Some(ref reports) = build_overlay.reports {
                build.reports = Some(reports.clone());
            }
            if let Some(ref timing) = build_overlay.timing {
                build.timing = Some(timing.clone());
            }
            if let Some(ref strategy) = build_overlay.default_strategy {
                build.default_strategy = Some(strategy.clone());
            }
        } else {
            resolved.project.build = Some(build_overlay.clone());
        }
    }
}

/// Full profile application: parse spec, resolve overlays, apply them, return label.
/// Must be called BEFORE platform resolution so that platform overrides take effect.
pub fn apply_profile(
    resolved: &mut ResolvedProject,
    profile_spec: &str,
) -> Result<String, LoomError> {
    let selection = parse_profile_spec(profile_spec)?;
    let (overlays, label) = resolve_profile_overlays(&resolved.project, &selection)?;

    // Clone overlays before mutating
    let owned_overlays: Vec<ProfileOverlay> = overlays.into_iter().cloned().collect();
    for overlay in &owned_overlays {
        apply_overlay(resolved, overlay);
    }

    resolved.active_profile = Some(label.clone());
    Ok(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_profile() {
        let sel = parse_profile_spec("fast_baud").unwrap();
        match sel {
            ProfileSelection::Simple(name) => assert_eq!(name, "fast_baud"),
            _ => panic!("Expected Simple"),
        }
    }

    #[test]
    fn test_parse_dimensional_profile() {
        let sel = parse_profile_spec("board=icebreaker,tier=lite").unwrap();
        match sel {
            ProfileSelection::Dimensional(dims) => {
                assert_eq!(dims.len(), 2);
                assert_eq!(dims[0], ("board".to_string(), "icebreaker".to_string()));
                assert_eq!(dims[1], ("tier".to_string(), "lite".to_string()));
            }
            _ => panic!("Expected Dimensional"),
        }
    }
}
