use std::collections::HashMap;

use loom_core::build::report::{ClockTiming, TimingMetrics, UtilizationMetrics};

/// Parsed yosys cell statistics from `stat` command output.
#[derive(Debug, Clone)]
pub struct YosysStats {
    pub cells: HashMap<String, u64>,
    pub wires: u64,
    pub wire_bits: u64,
    pub warnings_unique: u32,
    pub warnings_total: u32,
}

/// Parsed nextpnr device utilization entry.
#[derive(Debug, Clone)]
pub struct UtilizationEntry {
    pub used: u64,
    pub available: u64,
    pub percent: u64,
}

/// Parsed nextpnr utilization block.
#[derive(Debug, Clone)]
pub struct NextpnrUtilization {
    pub entries: HashMap<String, UtilizationEntry>,
}

/// Parsed nextpnr clock frequency result.
#[derive(Debug, Clone)]
pub struct ClockFrequency {
    pub name: String,
    pub achieved_mhz: f64,
    pub constraint_mhz: f64,
    pub passed: bool,
}

/// Parse yosys `stat` command output for cell counts.
pub fn parse_yosys_stats(log: &str) -> Option<YosysStats> {
    let mut cells = HashMap::new();
    let mut wires: u64 = 0;
    let mut wire_bits: u64 = 0;
    let mut in_cells = false;

    for line in log.lines() {
        let trimmed = line.trim();

        // Detect start of cells section
        if trimmed.starts_with("Number of cells:") {
            in_cells = true;
            continue;
        }

        // Parse cell lines when in cells section
        if in_cells {
            // Cell lines: "     SB_CARRY                       97"
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() == 2 {
                // Try both orderings: "count name" and "name count"
                if let Ok(count) = parts[1].parse::<u64>() {
                    cells.insert(parts[0].to_string(), count);
                    continue;
                } else if let Ok(count) = parts[0].parse::<u64>() {
                    cells.insert(parts[1].to_string(), count);
                    continue;
                }
            }
            // Anything else ends the cells section
            in_cells = false;
        }

        // Parse wire counts
        if trimmed.starts_with("Number of wires:") {
            if let Some(n) = extract_trailing_number(trimmed) {
                wires = n;
            }
        }
        if trimmed.starts_with("Number of wire bits:") {
            if let Some(n) = extract_trailing_number(trimmed) {
                wire_bits = n;
            }
        }
    }

    // Parse warning summary: "Warnings: N unique messages, N total"
    let (warnings_unique, warnings_total) = parse_warning_summary(log);

    if cells.is_empty() {
        return None;
    }

    Some(YosysStats {
        cells,
        wires,
        wire_bits,
        warnings_unique,
        warnings_total,
    })
}

fn extract_trailing_number(line: &str) -> Option<u64> {
    line.split_whitespace().last()?.parse().ok()
}

fn parse_warning_summary(log: &str) -> (u32, u32) {
    for line in log.lines() {
        let trimmed = line.trim();
        // "Warnings: 3 unique messages, 5 total"
        if let Some(rest) = trimmed.strip_prefix("Warnings:") {
            let rest = rest.trim();
            let parts: Vec<&str> = rest.split_whitespace().collect();
            // "3 unique messages, 5 total"
            if parts.len() >= 4 {
                let unique = parts[0].parse().unwrap_or(0);
                let total = parts[3].trim_end_matches(',').parse().unwrap_or(unique);
                return (unique, total);
            }
        }
    }
    (0, 0)
}

/// Collect lines matching `Warning:` from yosys output.
pub fn parse_yosys_warnings(log: &str) -> Vec<String> {
    log.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("Warning:")
                || trimmed.contains("] Warning:")
                || trimmed.starts_with("Warning ")
        })
        .map(|line| line.trim().to_string())
        .collect()
}

/// Parse nextpnr `Device utilisation:` block.
pub fn parse_nextpnr_utilization(log: &str) -> Option<NextpnrUtilization> {
    let mut entries = HashMap::new();
    let mut in_util = false;

    for line in log.lines() {
        let trimmed = line.trim();

        if trimmed.contains("Device utilisation:") {
            in_util = true;
            continue;
        }

        if in_util {
            // Pattern: "Info:   ICESTORM_LC:   795/ 5280    15%"
            // or:      "Info:            SB_LUT4:   795"
            if let Some(rest) = trimmed.strip_prefix("Info:") {
                let rest = rest.trim();
                if rest.is_empty() {
                    in_util = false;
                    continue;
                }
                // Try to parse "CELL: used/ avail  pct%"
                if let Some(entry) = parse_util_line(rest) {
                    entries.insert(entry.0, entry.1);
                } else if !rest.contains(':') {
                    // End of util block (next info line without colon)
                    in_util = false;
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with("Info") {
                in_util = false;
            }
        }
    }

    if entries.is_empty() {
        return None;
    }

    Some(NextpnrUtilization { entries })
}

/// Parse a single utilization line like "ICESTORM_LC:   795/ 5280    15%"
fn parse_util_line(line: &str) -> Option<(String, UtilizationEntry)> {
    let colon_pos = line.find(':')?;
    let cell_name = line[..colon_pos].trim().to_string();
    let rest = line[colon_pos + 1..].trim();

    // Try "used/ avail  pct%"
    if rest.contains('/') {
        let slash_pos = rest.find('/')?;
        let used: u64 = rest[..slash_pos].trim().parse().ok()?;
        let after_slash = rest[slash_pos + 1..].trim();
        let parts: Vec<&str> = after_slash.split_whitespace().collect();
        let available: u64 = parts.first()?.parse().ok()?;
        let percent = if parts.len() > 1 {
            parts[1].trim_end_matches('%').parse().unwrap_or(0)
        } else if available > 0 {
            (used * 100) / available
        } else {
            0
        };
        Some((
            cell_name,
            UtilizationEntry {
                used,
                available,
                percent,
            },
        ))
    } else {
        // Just a count, no available
        let used: u64 = rest.split_whitespace().next()?.parse().ok()?;
        Some((
            cell_name,
            UtilizationEntry {
                used,
                available: 0,
                percent: 0,
            },
        ))
    }
}

/// Parse nextpnr max frequency lines.
///
/// nextpnr may report Fmax twice per clock (estimate during placement, final after routing).
/// We keep only the last occurrence for each clock name (the final result).
pub fn parse_nextpnr_timing(log: &str) -> Vec<ClockFrequency> {
    let mut by_name: HashMap<String, ClockFrequency> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for line in log.lines() {
        // "Info: Max frequency for clock 'clk': 60.12 MHz (PASS at 48.00 MHz)"
        if let Some(rest) = line.trim().strip_prefix("Info: Max frequency for clock '") {
            if let Some(clock) = parse_fmax_line(rest) {
                if !by_name.contains_key(&clock.name) {
                    order.push(clock.name.clone());
                }
                by_name.insert(clock.name.clone(), clock);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|n| by_name.remove(&n))
        .collect()
}

fn parse_fmax_line(rest: &str) -> Option<ClockFrequency> {
    // rest = "clk': 60.12 MHz (PASS at 48.00 MHz)"
    let quote_pos = rest.find('\'')?;
    let name = rest[..quote_pos].to_string();
    let after_name = rest[quote_pos + 2..].trim(); // skip "': "

    let parts: Vec<&str> = after_name.split_whitespace().collect();
    // "60.12 MHz (PASS at 48.00 MHz)"
    if parts.len() < 4 {
        return None;
    }
    let achieved_mhz: f64 = parts[0].parse().ok()?;
    // parts[1] = "MHz", parts[2] = "(PASS" or "(FAIL"
    let passed = parts[2].contains("PASS");
    // parts[4] = constraint MHz
    let constraint_mhz: f64 = if parts.len() >= 5 {
        parts[4].parse().unwrap_or(achieved_mhz)
    } else {
        achieved_mhz
    };

    Some(ClockFrequency {
        name,
        achieved_mhz,
        constraint_mhz,
        passed,
    })
}

/// Map nextpnr utilization to generic `UtilizationMetrics`.
pub fn to_utilization_metrics(util: &NextpnrUtilization) -> UtilizationMetrics {
    // LUT: SB_LUT4 (ice40), TRELLIS_SLICE (ecp5), or ICESTORM_LC
    let (lut_used, lut_available, lut_percent) =
        find_entry(util, &["SB_LUT4", "TRELLIS_SLICE", "ICESTORM_LC", "LUT4"]);

    // FF: SB_DFF* variants (ice40), or TRELLIS_FF (ecp5)
    let (ff_used, ff_available, ff_percent) = find_ff_entry(util);

    // BRAM: SB_RAM40_4K (ice40), DP16KD (ecp5), ICESTORM_RAM
    let (bram_used, bram_available, bram_percent) =
        find_entry(util, &["SB_RAM40_4K", "DP16KD", "ICESTORM_RAM", "EBR"]);

    UtilizationMetrics {
        lut_used,
        lut_available,
        lut_percent: lut_percent as f64,
        ff_used,
        ff_available,
        ff_percent: ff_percent as f64,
        bram_used,
        bram_available,
        bram_percent: bram_percent as f64,
    }
}

fn find_entry(util: &NextpnrUtilization, names: &[&str]) -> (u64, u64, u64) {
    for name in names {
        if let Some(e) = util.entries.get(*name) {
            return (e.used, e.available, e.percent);
        }
    }
    (0, 0, 0)
}

fn find_ff_entry(util: &NextpnrUtilization) -> (u64, u64, u64) {
    // Try specific names first
    for name in &["TRELLIS_FF", "SB_DFF", "SB_DFFE"] {
        if let Some(e) = util.entries.get(*name) {
            return (e.used, e.available, e.percent);
        }
    }
    // Sum all SB_DFF* variants
    let mut total_used = 0u64;
    let mut total_available = 0u64;
    for (name, entry) in &util.entries {
        if name.starts_with("SB_DFF") {
            total_used += entry.used;
            if entry.available > total_available {
                total_available = entry.available;
            }
        }
    }
    if total_used > 0 {
        let pct = if total_available > 0 {
            (total_used * 100) / total_available
        } else {
            0
        };
        return (total_used, total_available, pct);
    }
    (0, 0, 0)
}

/// Convert nextpnr clock frequencies to generic `TimingMetrics`.
pub fn to_timing_metrics(clocks: &[ClockFrequency]) -> TimingMetrics {
    let mut clock_timings = Vec::new();
    let mut worst_wns = f64::MAX;

    for clk in clocks {
        let period_ns = 1000.0 / clk.constraint_mhz;
        let achieved_period = 1000.0 / clk.achieved_mhz;
        let wns = period_ns - achieved_period; // positive = met

        if wns < worst_wns {
            worst_wns = wns;
        }

        clock_timings.push(ClockTiming {
            name: clk.name.clone(),
            period_ns: Some(period_ns),
            frequency_mhz: Some(clk.constraint_mhz),
            wns,
            tns: if wns < 0.0 { wns } else { 0.0 },
            whs: 0.0,
            ths: 0.0,
            failing_endpoints: if clk.passed { 0 } else { 1 },
            total_endpoints: 1,
            achieved_mhz: Some(clk.achieved_mhz),
            is_generated: false,
        });
    }

    if worst_wns == f64::MAX {
        worst_wns = 0.0;
    }

    TimingMetrics {
        wns: worst_wns,
        tns: clock_timings.iter().map(|c| c.tns).sum(),
        whs: 0.0,
        ths: 0.0,
        failing_endpoints: clock_timings.iter().map(|c| c.failing_endpoints).sum(),
        clocks: clock_timings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YOSYS_LOG: &str = r#"
-- Running command `stat' --

=== top ===

   Number of wires:                427
   Number of wire bits:           1684
   Number of public wires:          68
   Number of public wire bits:     653
   Number of memories:               0
   Number of memory bits:            0
   Number of processes:              0
   Number of cells:                795
     SB_CARRY                       97
     SB_DFF                          8
     SB_DFFE                        42
     SB_DFFESR                      16
     SB_DFFSR                        2
     SB_LUT4                       630

Warnings: 3 unique messages, 5 total
"#;

    #[test]
    fn test_parse_yosys_stats() {
        let stats = parse_yosys_stats(YOSYS_LOG).unwrap();
        assert_eq!(stats.cells.get("SB_LUT4"), Some(&630));
        assert_eq!(stats.cells.get("SB_CARRY"), Some(&97));
        assert_eq!(stats.cells.get("SB_DFF"), Some(&8));
        assert_eq!(stats.wires, 427);
        assert_eq!(stats.wire_bits, 1684);
        assert_eq!(stats.warnings_unique, 3);
        assert_eq!(stats.warnings_total, 5);
    }

    #[test]
    fn test_parse_yosys_warnings() {
        let log = "Info: some info\nWarning: unused port\nWarning: width mismatch\nDone.\n";
        let warnings = parse_yosys_warnings(log);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("unused port"));
    }

    const NEXTPNR_LOG: &str = r#"
Info: Device utilisation:
Info: 	         ICESTORM_LC:   795/ 5280    15%
Info: 	        ICESTORM_RAM:     0/   30     0%
Info: 	               SB_IO:     4/   96     4%
Info: 	               SB_GB:     1/    8    12%
Info: 	        ICESTORM_PLL:     0/    1     0%
Info: 	         SB_WARMBOOT:     0/    1     0%

Info: Max frequency for clock 'clk': 60.12 MHz (PASS at 48.00 MHz)
"#;

    #[test]
    fn test_parse_nextpnr_utilization() {
        let util = parse_nextpnr_utilization(NEXTPNR_LOG).unwrap();
        let lc = util.entries.get("ICESTORM_LC").unwrap();
        assert_eq!(lc.used, 795);
        assert_eq!(lc.available, 5280);
        assert_eq!(lc.percent, 15);

        let io = util.entries.get("SB_IO").unwrap();
        assert_eq!(io.used, 4);
        assert_eq!(io.available, 96);
    }

    #[test]
    fn test_parse_nextpnr_timing() {
        let clocks = parse_nextpnr_timing(NEXTPNR_LOG);
        assert_eq!(clocks.len(), 1);
        assert_eq!(clocks[0].name, "clk");
        assert!((clocks[0].achieved_mhz - 60.12).abs() < 0.01);
        assert!((clocks[0].constraint_mhz - 48.00).abs() < 0.01);
        assert!(clocks[0].passed);
    }

    #[test]
    fn test_parse_nextpnr_timing_fail() {
        let log = "Info: Max frequency for clock 'sys_clk': 30.50 MHz (FAIL at 50.00 MHz)\n";
        let clocks = parse_nextpnr_timing(log);
        assert_eq!(clocks.len(), 1);
        assert_eq!(clocks[0].name, "sys_clk");
        assert!(!clocks[0].passed);
    }

    #[test]
    fn test_to_utilization_metrics() {
        let util = parse_nextpnr_utilization(NEXTPNR_LOG).unwrap();
        let metrics = to_utilization_metrics(&util);
        assert_eq!(metrics.lut_used, 795);
        assert_eq!(metrics.lut_available, 5280);
    }

    #[test]
    fn test_to_timing_metrics() {
        let clocks = parse_nextpnr_timing(NEXTPNR_LOG);
        let timing = to_timing_metrics(&clocks);
        assert!(timing.wns > 0.0); // PASS means positive slack
        assert_eq!(timing.clocks.len(), 1);
        assert_eq!(timing.clocks[0].name, "clk");
        assert!(timing.clocks[0].achieved_mhz.unwrap() > 60.0);
    }

    #[test]
    fn test_to_timing_metrics_fail() {
        let clocks = vec![ClockFrequency {
            name: "clk".to_string(),
            achieved_mhz: 30.0,
            constraint_mhz: 50.0,
            passed: false,
        }];
        let timing = to_timing_metrics(&clocks);
        assert!(timing.wns < 0.0); // FAIL means negative slack
        assert_eq!(timing.failing_endpoints, 1);
    }

    #[test]
    fn test_empty_log() {
        assert!(parse_yosys_stats("").is_none());
        assert!(parse_nextpnr_utilization("").is_none());
        assert!(parse_nextpnr_timing("").is_empty());
    }

    const NEXTPNR_MULTI_CLOCK: &str = r#"
Info: Max frequency for clock 'clk_fast': 120.00 MHz (PASS at 100.00 MHz)
Info: Max frequency for clock 'clk_slow': 25.50 MHz (PASS at 25.00 MHz)
"#;

    #[test]
    fn test_multi_clock_timing() {
        let clocks = parse_nextpnr_timing(NEXTPNR_MULTI_CLOCK);
        assert_eq!(clocks.len(), 2);
        let timing = to_timing_metrics(&clocks);
        assert_eq!(timing.clocks.len(), 2);
        // Both pass, so WNS should be positive
        assert!(timing.wns > 0.0);
    }

    #[test]
    fn test_duplicate_clock_keeps_last() {
        // nextpnr reports Fmax twice: estimate during placement, final after routing
        let log = "\
Info: Max frequency for clock 'clk_sys': 30.30 MHz (PASS at 24.00 MHz)
Info: Routing..
Info: Max frequency for clock 'clk_sys': 30.70 MHz (PASS at 24.00 MHz)
";
        let clocks = parse_nextpnr_timing(log);
        assert_eq!(clocks.len(), 1);
        assert_eq!(clocks[0].name, "clk_sys");
        assert!((clocks[0].achieved_mhz - 30.70).abs() < 0.01);
    }
}
