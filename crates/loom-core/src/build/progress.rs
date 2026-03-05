use crate::build::report::{TimingMetrics, UtilizationMetrics};

/// Events emitted during a build for live progress reporting.
#[derive(Debug, Clone)]
pub enum BuildEvent {
    PhaseStarted {
        phase: String,
    },
    PhaseCompleted {
        phase: String,
        elapsed_secs: f64,
        memory_mb: f64,
    },
    UtilizationAvailable(UtilizationMetrics),
    TimingAvailable {
        stage: String, // "post_place" or "post_route"
        timing: TimingMetrics,
    },
    IntermediateTiming {
        wns: f64,
        tns: f64,
        whs: Option<f64>,
        ths: Option<f64>,
    },
    CriticalWarning(String),
    Warning(String),
    DrcResult {
        errors: u32,
    },
    SynthesisSummary {
        errors: u32,
        critical_warnings: u32,
        warnings: u32,
    },
    VerboseLine(String),
}

/// Stateful line-by-line parser for Vivado stdout output.
///
/// Tracks the current build phase and emits [`BuildEvent`]s as lines are processed.
pub struct VivadoOutputParser {
    current_phase: Option<String>,
    in_marker: Option<String>, // e.g., "REPORT_UTIL" or "POST_PLACE_TIMING"
    marker_lines: Vec<String>,
    phases_completed: Vec<String>,
    /// Last seen Time (s): elapsed and peak memory for the current phase.
    /// Used as fallback when the phase has no summary time line (e.g., place_design).
    last_elapsed: Option<f64>,
    last_memory: Option<f64>,
    phase_start: Option<std::time::Instant>,
    /// Pending phase completion data: (phase, elapsed, memory).
    /// Vivado may emit multiple `Time (s):` lines per phase; we buffer the latest
    /// and only emit `PhaseCompleted` on phase transition or "completed successfully".
    pending_completion: Option<(String, f64, f64)>,
}

impl VivadoOutputParser {
    pub fn new() -> Self {
        Self {
            current_phase: None,
            in_marker: None,
            marker_lines: Vec::new(),
            phases_completed: Vec::new(),
            last_elapsed: None,
            last_memory: None,
            phase_start: None,
            pending_completion: None,
        }
    }

    /// Returns the list of phases completed so far.
    pub fn phases_completed(&self) -> &[String] {
        &self.phases_completed
    }

    /// Parse a single line of Vivado stdout and return any events produced.
    pub fn parse_line(&mut self, line: &str) -> Vec<BuildEvent> {
        let mut events = Vec::new();

        // Check for LOOM_MARKER boundaries
        if let Some(marker) = parse_marker_begin(line) {
            self.in_marker = Some(marker);
            self.marker_lines.clear();
            return events;
        }

        if line.contains("LOOM_MARKER:") && line.contains("_END") {
            if let Some(marker_type) = self.in_marker.take() {
                let captured = std::mem::take(&mut self.marker_lines);
                if let Some(event) = self.process_marker(&marker_type, &captured) {
                    events.push(event);
                }
            }
            return events;
        }

        // If inside a marker section, accumulate lines
        if self.in_marker.is_some() {
            self.marker_lines.push(line.to_string());
            return events;
        }

        // Phase start detection
        if let Some(phase) = detect_phase_start(line) {
            if self.current_phase.as_deref() != Some(&phase) {
                // Flush pending completion for previous phase before switching
                self.flush_pending(&mut events);
                self.current_phase = Some(phase.clone());
                self.last_elapsed = None;
                self.last_memory = None;
                self.phase_start = Some(std::time::Instant::now());
                events.push(BuildEvent::PhaseStarted { phase });
            }
        }

        // Track last Time (s): line within current phase for fallback
        if line.contains("Time (s):") {
            if let Some(e) = parse_elapsed_from_line(line) {
                self.last_elapsed = Some(e);
            }
            if let Some(m) = parse_memory_from_line(line) {
                self.last_memory = Some(m);
            }
        }

        // Phase completion with summary time line (e.g., "synth_design: Time (s): ...")
        // Buffer the latest values — Vivado may emit multiple Time lines per phase.
        if let Some((phase, elapsed, memory)) = detect_phase_time_memory(line) {
            self.pending_completion = Some((phase, elapsed, memory));
        }

        // "X completed successfully" — if pending Time data exists for this phase,
        // leave it for flush_pending (which may pick up a later, more accurate Time line).
        // If no pending, emit immediately using fallback values.
        if let Some(phase) = detect_completed_successfully(line) {
            if !self.phases_completed.contains(&phase) {
                let has_pending = self
                    .pending_completion
                    .as_ref()
                    .map(|(p, _, _)| p == &phase)
                    .unwrap_or(false);

                if !has_pending {
                    // No pending Time data — emit using last_elapsed/last_memory fallback
                    self.phases_completed.push(phase.clone());
                    let elapsed = self
                        .last_elapsed
                        .or_else(|| self.phase_start.map(|s| s.elapsed().as_secs_f64()))
                        .unwrap_or(0.0);
                    let memory = self.last_memory.unwrap_or(0.0);
                    events.push(BuildEvent::PhaseCompleted {
                        phase,
                        elapsed_secs: elapsed,
                        memory_mb: memory,
                    });
                }
                // If pending exists for this phase, do nothing — flush_pending
                // will emit with the latest Time values on next phase start or EOF.
            }
        }

        // Intermediate timing (during routing)
        if let Some(timing) = parse_intermediate_timing(line) {
            events.push(timing);
        }

        // Critical warnings
        if line.contains("CRITICAL WARNING:") {
            events.push(BuildEvent::CriticalWarning(line.to_string()));
        } else if line.contains("WARNING:") && !line.contains("CRITICAL") {
            events.push(BuildEvent::Warning(line.to_string()));
        }

        // DRC results
        if let Some(errors) = parse_drc_result(line) {
            events.push(BuildEvent::DrcResult { errors });
        }

        // Synthesis summary
        if let Some(summary) = parse_synthesis_summary(line) {
            events.push(summary);
        }

        events
    }

    /// Flush any pending completion event into the given events vector.
    /// Skips emission if the phase was already completed (e.g., by "completed successfully").
    fn flush_pending(&mut self, events: &mut Vec<BuildEvent>) {
        if let Some((phase, elapsed, memory)) = self.pending_completion.take() {
            if !self.phases_completed.contains(&phase) {
                self.phases_completed.push(phase.clone());
                events.push(BuildEvent::PhaseCompleted {
                    phase,
                    elapsed_secs: elapsed,
                    memory_mb: memory,
                });
            }
        }
    }

    /// Flush any buffered completion event at end-of-stream.
    /// Call this after the last line has been parsed.
    pub fn flush(&mut self) -> Vec<BuildEvent> {
        let mut events = Vec::new();
        self.flush_pending(&mut events);
        events
    }

    fn process_marker(&self, marker_type: &str, lines: &[String]) -> Option<BuildEvent> {
        match marker_type {
            "REPORT_UTIL" => parse_utilization_report(lines).map(BuildEvent::UtilizationAvailable),
            "POST_PLACE_TIMING" => {
                parse_timing_report(lines).map(|timing| BuildEvent::TimingAvailable {
                    stage: "post_place".to_string(),
                    timing,
                })
            }
            "POST_ROUTE_TIMING" => {
                parse_timing_report(lines).map(|timing| BuildEvent::TimingAvailable {
                    stage: "post_route".to_string(),
                    timing,
                })
            }
            _ => None,
        }
    }
}

impl Default for VivadoOutputParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract marker name from a LOOM_MARKER:..._BEGIN line.
fn parse_marker_begin(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("LOOM_MARKER:") {
        let rest = rest.trim();
        if let Some(name) = rest.strip_suffix("_BEGIN") {
            return Some(name.to_string());
        }
    }
    None
}

/// Detect phase start from Vivado output lines.
fn detect_phase_start(line: &str) -> Option<String> {
    // "Starting synth_design", "Starting opt_design", etc.
    if line.contains("Starting synth_design") {
        return Some("synthesis".to_string());
    }
    if line.contains("Starting opt_design") || line.contains("Command: opt_design") {
        return Some("optimize".to_string());
    }
    if line.contains("Starting place_design") || line.contains("Command: place_design") {
        return Some("place".to_string());
    }
    if line.contains("Starting route_design") || line.contains("Command: route_design") {
        return Some("route".to_string());
    }
    if line.contains("Starting write_bitstream") || line.contains("Command: write_bitstream") {
        return Some("bitstream".to_string());
    }
    None
}

/// Detect "X completed successfully" lines for phases that don't emit summary time lines.
fn detect_completed_successfully(line: &str) -> Option<String> {
    if line.contains("completed successfully") {
        if line.contains("synth_design") {
            return Some("synthesis".to_string());
        }
        if line.contains("opt_design") {
            return Some("optimize".to_string());
        }
        if line.contains("place_design") {
            return Some("place".to_string());
        }
        if line.contains("route_design") {
            return Some("route".to_string());
        }
        if line.contains("write_bitstream") {
            return Some("bitstream".to_string());
        }
    }
    None
}

/// Parse the phase completion time/memory line.
///
/// Example: `synth_design: Time (s): cpu = 00:00:20 ; elapsed = 00:00:27 . Memory (MB): peak = 1912.547`
fn detect_phase_time_memory(line: &str) -> Option<(String, f64, f64)> {
    // Match pattern: PHASE: Time (s): ... elapsed = HH:MM:SS ... Memory (MB): peak = NNNN.NNN
    let phase = if line.starts_with("synth_design:") || line.contains("synth_design: Time") {
        "synthesis"
    } else if line.starts_with("opt_design:") || line.contains("opt_design: Time") {
        "optimize"
    } else if line.starts_with("place_design:") || line.contains("place_design: Time") {
        "place"
    } else if line.starts_with("route_design:") || line.contains("route_design: Time") {
        "route"
    } else if line.starts_with("write_bitstream:") || line.contains("write_bitstream: Time") {
        "bitstream"
    } else {
        return None;
    };

    if !line.contains("Time (s):") {
        return None;
    }

    let elapsed = parse_elapsed_from_line(line)?;
    let memory = parse_memory_from_line(line).unwrap_or(0.0);

    Some((phase.to_string(), elapsed, memory))
}

/// Parse elapsed time from "elapsed = HH:MM:SS" pattern.
fn parse_elapsed_from_line(line: &str) -> Option<f64> {
    let elapsed_marker = "elapsed = ";
    let idx = line.find(elapsed_marker)?;
    let rest = &line[idx + elapsed_marker.len()..];
    // Format: HH:MM:SS or could be just seconds
    let time_str = rest.split_whitespace().next()?;
    parse_time_str(time_str)
}

/// Parse a time string like "00:00:27" into seconds.
fn parse_time_str(s: &str) -> Option<f64> {
    let s = s.trim_end_matches(|c: char| !c.is_ascii_digit() && c != ':');
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        3 => {
            let h: f64 = parts[0].parse().ok()?;
            let m: f64 = parts[1].parse().ok()?;
            let sec: f64 = parts[2].parse().ok()?;
            Some(h * 3600.0 + m * 60.0 + sec)
        }
        2 => {
            let m: f64 = parts[0].parse().ok()?;
            let sec: f64 = parts[1].parse().ok()?;
            Some(m * 60.0 + sec)
        }
        1 => parts[0].parse().ok(),
        _ => None,
    }
}

/// Parse peak memory from "peak = NNNN.NNN" pattern.
fn parse_memory_from_line(line: &str) -> Option<f64> {
    let marker = "peak = ";
    let idx = line.find(marker)?;
    let rest = &line[idx + marker.len()..];
    let val_str = rest.split_whitespace().next()?;
    val_str.parse().ok()
}

/// Parse intermediate or estimated timing summary lines.
///
/// Example: `Intermediate Timing Summary | WNS=7.276  | TNS=0.000  | WHS=0.005  | THS=0.000  |`
fn parse_intermediate_timing(line: &str) -> Option<BuildEvent> {
    if !line.contains("Timing Summary") || !line.contains("WNS=") {
        return None;
    }

    let wns = extract_timing_value(line, "WNS=")?;
    let tns = extract_timing_value(line, "TNS=");
    let whs = extract_timing_value(line, "WHS=");
    let ths = extract_timing_value(line, "THS=");

    Some(BuildEvent::IntermediateTiming {
        wns,
        tns: tns.unwrap_or(0.0),
        whs,
        ths,
    })
}

/// Extract a numeric value after a key like "WNS=" from a timing summary line.
fn extract_timing_value(line: &str, key: &str) -> Option<f64> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let val_str: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    val_str.parse().ok()
}

/// Parse DRC result line: "DRC finished with N Errors"
fn parse_drc_result(line: &str) -> Option<u32> {
    if !line.contains("DRC finished with") {
        return None;
    }
    let idx = line.find("DRC finished with")?;
    let rest = &line[idx + "DRC finished with".len()..];
    let rest = rest.trim();
    let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

/// Parse synthesis summary: "Synthesis finished with N errors, N critical warnings and N warnings."
fn parse_synthesis_summary(line: &str) -> Option<BuildEvent> {
    if !line.contains("Synthesis finished with") {
        return None;
    }

    let errors = extract_count_before(line, "error")?;
    let critical_warnings = extract_count_before(line, "critical warning").unwrap_or(0);
    let warnings = extract_last_warning_count(line).unwrap_or(0);

    Some(BuildEvent::SynthesisSummary {
        errors,
        critical_warnings,
        warnings,
    })
}

/// Extract count before a keyword: "N errors" -> N
fn extract_count_before(line: &str, keyword: &str) -> Option<u32> {
    let idx = line.find(keyword)?;
    let before = &line[..idx];
    let num_str: String = before
        .chars()
        .rev()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    num_str.parse().ok()
}

/// Extract the last "N warnings" count (the one after "and").
fn extract_last_warning_count(line: &str) -> Option<u32> {
    // "N critical warnings and N warnings."
    // Find the last occurrence of a number before "warnings" that isn't before "critical"
    let and_idx = line.rfind(" and ")?;
    let rest = &line[and_idx + 5..];
    let num_str: String = rest
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse().ok()
}

/// Parse a Vivado utilization report table captured between LOOM_MARKER delimiters.
fn parse_utilization_report(lines: &[String]) -> Option<UtilizationMetrics> {
    let mut lut_used = 0u64;
    let mut lut_available = 0u64;
    let mut lut_percent = 0.0f64;
    let mut ff_used = 0u64;
    let mut ff_available = 0u64;
    let mut ff_percent = 0.0f64;
    let mut bram_used = 0u64;
    let mut bram_available = 0u64;
    let mut bram_percent = 0.0f64;

    for line in lines {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
            continue;
        }

        let cols: Vec<&str> = trimmed.split('|').map(|s| s.trim()).collect();
        // cols[0] is empty (before first |), cols[1] is the resource name, etc.
        if cols.len() < 6 {
            continue;
        }

        let name = cols[1].to_lowercase();

        if name.contains("slice lut") && !name.contains("logic") && !name.contains("memory") {
            if let Some((used, avail, pct)) = parse_util_row(&cols) {
                lut_used = used;
                lut_available = avail;
                lut_percent = pct;
            }
        } else if name.contains("clb lut") && !name.contains("logic") && !name.contains("memory") {
            // UltraScale uses "CLB LUTs" instead of "Slice LUTs"
            if let Some((used, avail, pct)) = parse_util_row(&cols) {
                lut_used = used;
                lut_available = avail;
                lut_percent = pct;
            }
        } else if (name.contains("register") || name.contains("slice ff"))
            && !name.contains("carry")
        {
            if lut_used > 0 || ff_used == 0 {
                // Only take the first register/FF row
                if let Some((used, avail, pct)) = parse_util_row(&cols) {
                    if ff_used == 0 {
                        ff_used = used;
                        ff_available = avail;
                        ff_percent = pct;
                    }
                }
            }
        } else if name.contains("block ram") || name.contains("bram") {
            if let Some((used, avail, pct)) = parse_util_row(&cols) {
                bram_used = used;
                bram_available = avail;
                bram_percent = pct;
            }
        }
    }

    // Only return if we found at least some data
    if lut_available > 0 || ff_available > 0 {
        Some(UtilizationMetrics {
            lut_used,
            lut_available,
            lut_percent,
            ff_used,
            ff_available,
            ff_percent,
            bram_used,
            bram_available,
            bram_percent,
        })
    } else {
        None
    }
}

/// Parse a utilization table row: | Name | Used | Fixed | Available | Util% |
fn parse_util_row(cols: &[&str]) -> Option<(u64, u64, f64)> {
    // cols: ["", "Name", "Used", "Fixed", "Prohibited", "Available", "Util%", ""]
    // or: ["", "Name", "Used", "Fixed", "Available", "Util%", ""]
    // We need to find the used, available, and percent columns
    // Strategy: parse from the right — last numeric-looking column is %, before that is available, etc.
    let numeric_cols: Vec<(usize, &str)> = cols
        .iter()
        .enumerate()
        .skip(2) // skip empty + name
        .filter(|(_, s)| {
            let s = s.trim();
            !s.is_empty()
                && (s
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    || s.starts_with('<'))
        })
        .map(|(i, s)| (i, *s))
        .collect();

    if numeric_cols.len() < 3 {
        return None;
    }

    let pct_str = numeric_cols.last()?.1;
    let avail_str = numeric_cols[numeric_cols.len() - 2].1;
    let used_str = numeric_cols[0].1;

    let used: u64 = used_str.trim().parse().ok()?;
    let avail: u64 = avail_str.trim().parse().ok()?;
    let pct: f64 = pct_str.trim().parse().unwrap_or_else(|_| {
        if avail > 0 {
            (used as f64 / avail as f64) * 100.0
        } else {
            0.0
        }
    });

    Some((used, avail, pct))
}

/// Parse a Vivado timing summary report captured between LOOM_MARKER delimiters.
fn parse_timing_report(lines: &[String]) -> Option<TimingMetrics> {
    let mut wns: Option<f64> = None;
    let mut tns: Option<f64> = None;
    let mut whs: Option<f64> = None;
    let mut ths: Option<f64> = None;
    let mut failing = 0u32;

    for line in lines {
        let trimmed = line.trim();

        // Look for "WNS(ns)      TNS(ns)  TNS Failing Endpoints  ..."
        // Followed by data row like: "7.085       0.000              0  ..."
        if trimmed.starts_with("WNS") && trimmed.contains("TNS") {
            continue; // header row
        }

        // Try to parse timing data row (numbers separated by whitespace)
        if wns.is_none() && looks_like_timing_data(trimmed) {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                if let (Ok(w), Ok(t)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    wns = Some(w);
                    tns = Some(t);
                    if let Ok(f) = parts[2].parse::<u32>() {
                        failing = f;
                    }
                }
            }
        }

        // WHS/THS is in a second block with the same pattern
        if wns.is_some() && whs.is_none() && looks_like_timing_data(trimmed) {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                if let (Ok(w), Ok(t)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    // If this is a different set of values than WNS/TNS
                    if Some(w) != wns || Some(t) != tns {
                        whs = Some(w);
                        ths = Some(t);
                        if let Ok(f) = parts[2].parse::<u32>() {
                            failing += f;
                        }
                    }
                }
            }
        }
    }

    if let (Some(wns_val), Some(tns_val)) = (wns, tns) {
        Some(TimingMetrics {
            wns: wns_val,
            tns: tns_val,
            whs: whs.unwrap_or(0.0),
            ths: ths.unwrap_or(0.0),
            failing_endpoints: failing,
        })
    } else {
        None
    }
}

/// Check if a line looks like a timing data row (starts with a number or negative number).
fn looks_like_timing_data(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let first_char = trimmed.chars().next().unwrap();
    first_char.is_ascii_digit() || first_char == '-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_start_detection() {
        let mut parser = VivadoOutputParser::new();

        let events = parser.parse_line("Starting synth_design");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::PhaseStarted { phase } if phase == "synthesis"));

        let events = parser.parse_line("Command: opt_design");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::PhaseStarted { phase } if phase == "optimize"));

        let events = parser.parse_line("Command: place_design");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::PhaseStarted { phase } if phase == "place"));

        let events = parser.parse_line("Command: route_design");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::PhaseStarted { phase } if phase == "route"));

        let events = parser.parse_line("Command: write_bitstream");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::PhaseStarted { phase } if phase == "bitstream"));
    }

    #[test]
    fn test_phase_completion_with_time_memory() {
        let mut parser = VivadoOutputParser::new();

        // Time line buffers completion data (not emitted yet)
        let events = parser.parse_line(
            "synth_design: Time (s): cpu = 00:00:20 ; elapsed = 00:00:27 . Memory (MB): peak = 1912.547",
        );
        assert_eq!(events.len(), 0);

        // "completed successfully" defers to flush when pending exists
        let events = parser.parse_line("synth_design completed successfully");
        assert_eq!(events.len(), 0);

        // Flush emits the buffered event
        let events = parser.flush();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BuildEvent::PhaseCompleted {
                phase,
                elapsed_secs,
                memory_mb,
            } => {
                assert_eq!(phase, "synthesis");
                assert!((elapsed_secs - 27.0).abs() < 0.01);
                assert!((memory_mb - 1912.547).abs() < 0.01);
            }
            _ => panic!("Expected PhaseCompleted"),
        }
    }

    #[test]
    fn test_phase_completion_place() {
        let mut parser = VivadoOutputParser::new();

        // Time line buffers completion data
        let events = parser.parse_line(
            "place_design: Time (s): cpu = 00:00:01 ; elapsed = 00:00:01 . Memory (MB): peak = 2049.000",
        );
        assert_eq!(events.len(), 0);

        // Flush emits the buffered event
        let events = parser.flush();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BuildEvent::PhaseCompleted {
                phase,
                elapsed_secs,
                memory_mb,
            } => {
                assert_eq!(phase, "place");
                assert!((elapsed_secs - 1.0).abs() < 0.01);
                assert!((memory_mb - 2049.0).abs() < 0.01);
            }
            _ => panic!("Expected PhaseCompleted"),
        }
    }

    #[test]
    fn test_intermediate_timing() {
        let mut parser = VivadoOutputParser::new();

        let events = parser.parse_line(
            "Intermediate Timing Summary | WNS=7.276  | TNS=0.000  | WHS=0.005  | THS=0.000  |",
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            BuildEvent::IntermediateTiming { wns, tns, whs, ths } => {
                assert!((wns - 7.276).abs() < 0.001);
                assert!((tns - 0.0).abs() < 0.001);
                assert!((whs.unwrap() - 0.005).abs() < 0.001);
                assert!((ths.unwrap() - 0.0).abs() < 0.001);
            }
            _ => panic!("Expected IntermediateTiming"),
        }
    }

    #[test]
    fn test_estimated_timing() {
        let mut parser = VivadoOutputParser::new();

        let events = parser.parse_line(
            "Estimated Timing Summary | WNS=7.085  | TNS=0.000  | WHS=0.308  | THS=0.000  |",
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], BuildEvent::IntermediateTiming { wns, .. } if (*wns - 7.085).abs() < 0.001)
        );
    }

    #[test]
    fn test_critical_warning() {
        let mut parser = VivadoOutputParser::new();

        let events = parser
            .parse_line("CRITICAL WARNING: [Constraints 18-512] set_false_path: something wrong");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::CriticalWarning(msg) if msg.contains("18-512")));
    }

    #[test]
    fn test_warning_not_critical() {
        let mut parser = VivadoOutputParser::new();

        let events = parser.parse_line("WARNING: [DRC RTSTAT-1] Unrouted nets: some message");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::Warning(msg) if msg.contains("RTSTAT")));
    }

    #[test]
    fn test_drc_result() {
        let mut parser = VivadoOutputParser::new();

        let events = parser.parse_line("DRC finished with 0 Errors");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::DrcResult { errors: 0 }));

        let events = parser.parse_line("DRC finished with 3 Errors");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BuildEvent::DrcResult { errors: 3 }));
    }

    #[test]
    fn test_synthesis_summary() {
        let mut parser = VivadoOutputParser::new();

        let events = parser
            .parse_line("Synthesis finished with 0 errors, 1 critical warnings and 5 warnings.");
        assert_eq!(events.len(), 1);
        match &events[0] {
            BuildEvent::SynthesisSummary {
                errors,
                critical_warnings,
                warnings,
            } => {
                assert_eq!(*errors, 0);
                assert_eq!(*critical_warnings, 1);
                assert_eq!(*warnings, 5);
            }
            _ => panic!("Expected SynthesisSummary"),
        }
    }

    #[test]
    fn test_completed_successfully_fallback() {
        let mut parser = VivadoOutputParser::new();

        // Start place phase
        parser.parse_line("Command: place_design");

        // Simulate sub-phase time lines (no "place_design: Time" summary)
        parser.parse_line(
            "Time (s): cpu = 00:00:01 ; elapsed = 00:00:00.300 . Memory (MB): peak = 2027.180 ; gain = 0.000",
        );

        // Completed successfully triggers PhaseCompleted using last_elapsed/last_memory
        let events = parser.parse_line("place_design completed successfully");
        assert!(events
            .iter()
            .any(|e| matches!(e, BuildEvent::PhaseCompleted { phase, .. } if phase == "place")));
        let completed = events
            .iter()
            .find(|e| matches!(e, BuildEvent::PhaseCompleted { .. }))
            .unwrap();
        match completed {
            BuildEvent::PhaseCompleted { memory_mb, .. } => {
                assert!((*memory_mb - 2027.18).abs() < 0.01);
            }
            _ => unreachable!(),
        }
        assert!(parser.phases_completed().contains(&"place".to_string()));
    }

    #[test]
    fn test_utilization_with_prohibited_column() {
        let mut parser = VivadoOutputParser::new();

        parser.parse_line("LOOM_MARKER:REPORT_UTIL_BEGIN");

        // Real Vivado format with Prohibited column and <0.01
        let table_lines = [
            "+-------------------------+------+-------+------------+-----------+-------+",
            "|        Site Type        | Used | Fixed | Prohibited | Available | Util% |",
            "+-------------------------+------+-------+------------+-----------+-------+",
            "| Slice LUTs*             |    2 |     0 |          0 |     20800 | <0.01 |",
            "| Slice Registers         |   31 |     0 |          0 |     41600 |  0.07 |",
            "| Block RAM Tile          |    0 |     0 |          0 |        50 |  0.00 |",
            "+-------------------------+------+-------+------------+-----------+-------+",
        ];

        for line in &table_lines {
            parser.parse_line(line);
        }

        let events = parser.parse_line("LOOM_MARKER:REPORT_UTIL_END");
        assert_eq!(events.len(), 1);
        match &events[0] {
            BuildEvent::UtilizationAvailable(util) => {
                assert_eq!(util.lut_used, 2);
                assert_eq!(util.lut_available, 20800);
                assert_eq!(util.ff_used, 31);
                assert_eq!(util.ff_available, 41600);
                assert_eq!(util.bram_used, 0);
                assert_eq!(util.bram_available, 50);
            }
            _ => panic!("Expected UtilizationAvailable"),
        }
    }

    #[test]
    fn test_marker_utilization_parsing() {
        let mut parser = VivadoOutputParser::new();

        // Start the marker
        let events = parser.parse_line("LOOM_MARKER:REPORT_UTIL_BEGIN");
        assert!(events.is_empty());

        // Feed utilization table lines
        let table_lines = [
            "+----------------------------+------+-------+-----------+-------+",
            "|          Site Type         | Used | Fixed | Available | Util% |",
            "+----------------------------+------+-------+-----------+-------+",
            "| Slice LUTs                 | 1234 |     0 |     20800 |  5.93 |",
            "| Slice Registers            |  450 |     0 |     41600 |  1.08 |",
            "| Block RAM Tile             |    0 |     0 |        50 |  0.00 |",
            "+----------------------------+------+-------+-----------+-------+",
        ];

        for line in &table_lines {
            let events = parser.parse_line(line);
            assert!(events.is_empty());
        }

        // End the marker
        let events = parser.parse_line("LOOM_MARKER:REPORT_UTIL_END");
        assert_eq!(events.len(), 1);

        match &events[0] {
            BuildEvent::UtilizationAvailable(util) => {
                assert_eq!(util.lut_used, 1234);
                assert_eq!(util.lut_available, 20800);
                assert!((util.lut_percent - 5.93).abs() < 0.01);
                assert_eq!(util.ff_used, 450);
                assert_eq!(util.ff_available, 41600);
                assert!((util.ff_percent - 1.08).abs() < 0.01);
                assert_eq!(util.bram_used, 0);
                assert_eq!(util.bram_available, 50);
            }
            _ => panic!("Expected UtilizationAvailable"),
        }
    }

    #[test]
    fn test_marker_timing_parsing() {
        let mut parser = VivadoOutputParser::new();

        // Start the marker
        parser.parse_line("LOOM_MARKER:POST_ROUTE_TIMING_BEGIN");

        let timing_lines = [
            "------------------------------------------------------------------------------------------------",
            "| Design Timing Summary",
            "| ---------------------",
            "------------------------------------------------------------------------------------------------",
            "",
            "    WNS(ns)      TNS(ns)  TNS Failing Endpoints  TNS Total Endpoints",
            "    -------      -------  ---------------------  -------------------",
            "      7.085        0.000                      0                   42",
            "",
            "",
            "    WHS(ns)      THS(ns)  THS Failing Endpoints  THS Total Endpoints",
            "    -------      -------  ---------------------  -------------------",
            "      0.308        0.000                      0                   38",
        ];

        for line in &timing_lines {
            parser.parse_line(line);
        }

        let events = parser.parse_line("LOOM_MARKER:POST_ROUTE_TIMING_END");
        assert_eq!(events.len(), 1);

        match &events[0] {
            BuildEvent::TimingAvailable { stage, timing } => {
                assert_eq!(stage, "post_route");
                assert!((timing.wns - 7.085).abs() < 0.001);
                assert!((timing.tns - 0.0).abs() < 0.001);
                assert!((timing.whs - 0.308).abs() < 0.001);
                assert!((timing.ths - 0.0).abs() < 0.001);
                assert_eq!(timing.failing_endpoints, 0);
            }
            _ => panic!("Expected TimingAvailable"),
        }
    }

    #[test]
    fn test_no_duplicate_phase_completed() {
        let mut parser = VivadoOutputParser::new();

        // Simulate Vivado emitting two Time lines for the same phase
        parser.parse_line("Starting synth_design");

        let events = parser.parse_line(
            "synth_design: Time (s): cpu = 00:00:00 ; elapsed = 00:00:00 . Memory (MB): peak = 1922.000",
        );
        assert!(events.is_empty(), "First Time line should buffer, not emit");

        let events = parser.parse_line(
            "synth_design: Time (s): cpu = 00:00:20 ; elapsed = 00:00:21 . Memory (MB): peak = 1922.000",
        );
        assert!(
            events.is_empty(),
            "Second Time line should overwrite buffer"
        );

        // "completed successfully" defers because pending exists
        let events = parser.parse_line("synth_design completed successfully");
        assert!(
            events.is_empty(),
            "Should defer to flush when pending exists"
        );

        // Next phase start flushes with latest values
        let events = parser.parse_line("Command: opt_design");
        let completed: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, BuildEvent::PhaseCompleted { .. }))
            .collect();
        assert_eq!(completed.len(), 1, "Should emit exactly one PhaseCompleted");
        match completed[0] {
            BuildEvent::PhaseCompleted {
                phase,
                elapsed_secs,
                ..
            } => {
                assert_eq!(phase, "synthesis");
                assert!(
                    (*elapsed_secs - 21.0).abs() < 0.01,
                    "Should use latest Time value, not first"
                );
            }
            _ => panic!("Expected PhaseCompleted"),
        }
    }

    #[test]
    fn test_no_duplicate_phase_start() {
        let mut parser = VivadoOutputParser::new();

        let events1 = parser.parse_line("Starting synth_design");
        assert_eq!(events1.len(), 1);

        // Same phase again — should not emit
        let events2 = parser.parse_line("Starting synth_design");
        assert_eq!(events2.len(), 0);
    }

    #[test]
    fn test_parse_time_str() {
        assert!((parse_time_str("00:00:27").unwrap() - 27.0).abs() < 0.01);
        assert!((parse_time_str("00:01:30").unwrap() - 90.0).abs() < 0.01);
        assert!((parse_time_str("01:00:00").unwrap() - 3600.0).abs() < 0.01);
    }

    #[test]
    fn test_full_build_sequence() {
        let mut parser = VivadoOutputParser::new();

        let lines = [
            "Starting synth_design",
            "INFO: Synthesizing...",
            "Synthesis finished with 0 errors, 0 critical warnings and 2 warnings.",
            "LOOM_MARKER:REPORT_UTIL_BEGIN",
            "| Slice LUTs                 | 100 |     0 |     20800 |  0.48 |",
            "| Slice Registers            |  50 |     0 |     41600 |  0.12 |",
            "| Block RAM Tile             |   0 |     0 |        50 |  0.00 |",
            "LOOM_MARKER:REPORT_UTIL_END",
            "synth_design: Time (s): cpu = 00:00:20 ; elapsed = 00:00:27 . Memory (MB): peak = 1912.547",
            "synth_design completed successfully",
            "Command: opt_design",
            "opt_design: Time (s): cpu = 00:00:05 ; elapsed = 00:00:06 . Memory (MB): peak = 2049.000",
            "opt_design completed successfully",
            "Command: place_design",
            "LOOM_MARKER:POST_PLACE_TIMING_BEGIN",
            "    WNS(ns)      TNS(ns)  TNS Failing Endpoints  TNS Total Endpoints",
            "    -------      -------  ---------------------  -------------------",
            "      7.085        0.000                      0                   42",
            "",
            "    WHS(ns)      THS(ns)  THS Failing Endpoints  THS Total Endpoints",
            "    -------      -------  ---------------------  -------------------",
            "      0.308        0.000                      0                   38",
            "LOOM_MARKER:POST_PLACE_TIMING_END",
            "place_design: Time (s): cpu = 00:00:01 ; elapsed = 00:00:01 . Memory (MB): peak = 2049.000",
            "place_design completed successfully",
            "Command: route_design",
            "Intermediate Timing Summary | WNS=7.276  | TNS=0.000  | WHS=0.005  | THS=0.000  |",
            "route_design: Time (s): cpu = 00:00:10 ; elapsed = 00:00:11 . Memory (MB): peak = 2049.000",
            "route_design completed successfully",
            "Command: write_bitstream",
            "CRITICAL WARNING: [Constraints 18-512] set_false_path: some constraint issue",
            "write_bitstream: Time (s): cpu = 00:00:06 ; elapsed = 00:00:07 . Memory (MB): peak = 2215.000",
        ];

        let mut all_events = Vec::new();
        for line in &lines {
            all_events.extend(parser.parse_line(line));
        }
        // Flush any remaining pending completion (write_bitstream has no "completed successfully")
        all_events.extend(parser.flush());

        // Verify we got the key events
        let phase_starts: Vec<&str> = all_events
            .iter()
            .filter_map(|e| match e {
                BuildEvent::PhaseStarted { phase } => Some(phase.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            phase_starts,
            vec!["synthesis", "optimize", "place", "route", "bitstream"]
        );

        let phase_completes: Vec<&str> = all_events
            .iter()
            .filter_map(|e| match e {
                BuildEvent::PhaseCompleted { phase, .. } => Some(phase.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            phase_completes,
            vec!["synthesis", "optimize", "place", "route", "bitstream"]
        );

        // Each phase should emit exactly ONE PhaseCompleted (no duplicates)
        assert_eq!(
            phase_completes.len(),
            5,
            "Each phase should have exactly one PhaseCompleted event"
        );

        // Check utilization was captured
        assert!(all_events
            .iter()
            .any(|e| matches!(e, BuildEvent::UtilizationAvailable(_))));

        // Check timing was captured
        assert!(all_events.iter().any(
            |e| matches!(e, BuildEvent::TimingAvailable { stage, .. } if stage == "post_place")
        ));

        // Check critical warning
        assert!(all_events
            .iter()
            .any(|e| matches!(e, BuildEvent::CriticalWarning(_))));

        // Check intermediate timing
        assert!(all_events.iter().any(|e| matches!(
            e,
            BuildEvent::IntermediateTiming { wns, .. } if (*wns - 7.276).abs() < 0.001
        )));

        // Check phases_completed
        assert_eq!(parser.phases_completed().len(), 5);
    }
}
