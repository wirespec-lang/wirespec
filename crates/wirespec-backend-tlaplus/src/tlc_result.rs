/// Parsed result from TLC model checker execution.
#[derive(Debug)]
pub enum TlcResult {
    Pass {
        states_explored: Option<u64>,
        distinct_states: Option<u64>,
    },
    Fail {
        violated_property: String,
        counterexample: Vec<TlcStep>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug)]
pub struct TlcStep {
    pub step_number: u32,
    pub state_tag: Option<String>,
    pub fields: Vec<(String, String)>,
    pub is_stuttering: bool,
}

/// Parse TLC stdout into a structured result.
pub fn parse_tlc_output(stdout: &str) -> TlcResult {
    // Check for success
    if stdout.contains("Model checking completed. No error found") {
        let states_explored = extract_number(stdout, "states generated");
        let distinct_states = extract_number(stdout, "distinct states found");
        return TlcResult::Pass {
            states_explored,
            distinct_states,
        };
    }

    // Check for property violation
    if let Some(prop) = extract_violated_property(stdout) {
        let counterexample = extract_counterexample(stdout);
        return TlcResult::Fail {
            violated_property: prop,
            counterexample,
        };
    }

    // Check for errors
    if stdout.contains("Error:") || stdout.contains("error:") {
        let msg = stdout
            .lines()
            .filter(|l| l.contains("Error") || l.contains("error"))
            .collect::<Vec<_>>()
            .join("\n");
        return TlcResult::Error { message: msg };
    }

    // Fallback
    TlcResult::Error {
        message: format!("Unable to parse TLC output:\n{}", stdout),
    }
}

fn extract_number(text: &str, key: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(key_pos) = line.find(key) {
            // Extract the number immediately before the key phrase.
            // E.g. "42 states generated, 12 distinct states found."
            // For key "distinct states found", we want the number just before it.
            let prefix = &line[..key_pos];
            // Take the last whitespace-separated token that parses as u64
            for word in prefix.split_whitespace().rev() {
                // Strip commas from numbers like "1,234"
                let cleaned: String = word.chars().filter(|c| *c != ',').collect();
                if let Ok(n) = cleaned.parse::<u64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn extract_violated_property(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        // "Error: Invariant NoDeadlock is violated."
        if line.starts_with("Error: Invariant ") && line.contains("is violated") {
            let prop = line
                .trim_start_matches("Error: Invariant ")
                .trim_end_matches(" is violated.")
                .trim()
                .to_string();
            return Some(prop);
        }
        // "Error: Temporal properties were violated."
        // The property name appears on a subsequent line
        if line.contains("Temporal properties were violated") {
            // Look for the property name in nearby lines
            // "Error: ... Property ... is violated"
            continue;
        }
        if line.starts_with("Error: ") && line.contains("violated") {
            // Generic violation
            let prop = line
                .trim_start_matches("Error: ")
                .replace(" is violated.", "")
                .replace(" is violated", "")
                .trim()
                .to_string();
            return Some(prop);
        }
    }
    None
}

fn extract_counterexample(text: &str) -> Vec<TlcStep> {
    let mut steps = Vec::new();
    let mut current_step: Option<u32> = None;
    let mut current_fields: Vec<(String, String)> = Vec::new();
    let mut is_stuttering = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // "State 1: <Initial predicate>"
        if trimmed.starts_with("State ") && trimmed.contains(':') {
            // Save previous step
            if let Some(step_num) = current_step {
                let tag = current_fields
                    .iter()
                    .find(|(k, _)| k == "tag")
                    .map(|(_, v)| v.trim_matches('"').to_string());
                let fields: Vec<(String, String)> = current_fields
                    .iter()
                    .filter(|(k, v)| k != "tag" && !v.contains("@@null"))
                    .cloned()
                    .collect();
                steps.push(TlcStep {
                    step_number: step_num,
                    state_tag: tag,
                    fields,
                    is_stuttering,
                });
            }

            // Parse step number
            let num_str: String = trimmed
                .chars()
                .skip(6) // "State "
                .take_while(|c| c.is_ascii_digit())
                .collect();
            current_step = num_str.parse().ok();
            current_fields = Vec::new();
            is_stuttering = trimmed.contains("Stuttering");
        }

        // "/\ sm = [tag |-> "Init", path_id |-> 0, ...]"
        // or "/\ sm.tag = ..."
        if trimmed.starts_with("/\\") && trimmed.contains("sm") {
            // Try to parse record fields from "[key |-> val, ...]"
            if let Some(bracket_start) = trimmed.find('[') {
                let record = &trimmed[bracket_start..];
                if let Some(bracket_end) = record.find(']') {
                    let inner = &record[1..bracket_end];
                    for part in inner.split(',') {
                        let part = part.trim();
                        if let Some(arrow_pos) = part.find("|->") {
                            let key = part[..arrow_pos].trim().to_string();
                            let val = part[arrow_pos + 3..].trim().to_string();
                            current_fields.push((key, val));
                        }
                    }
                }
            }
        }
    }

    // Save last step
    if let Some(step_num) = current_step {
        let tag = current_fields
            .iter()
            .find(|(k, _)| k == "tag")
            .map(|(_, v)| v.trim_matches('"').to_string());
        let fields: Vec<(String, String)> = current_fields
            .iter()
            .filter(|(k, v)| k != "tag" && !v.contains("@@null"))
            .cloned()
            .collect();
        steps.push(TlcStep {
            step_number: step_num,
            state_tag: tag,
            fields,
            is_stuttering,
        });
    }

    steps
}

/// Format a TlcResult for human-readable CLI output.
pub fn format_result(result: &TlcResult, sm_name: &str, bound: u32) -> String {
    match result {
        TlcResult::Pass {
            states_explored,
            distinct_states,
        } => {
            let mut out = format!(
                "PASS: All properties verified for {} (bound = {})\n",
                sm_name, bound
            );
            if let Some(n) = states_explored {
                out.push_str(&format!("  States explored: {}\n", n));
            }
            if let Some(n) = distinct_states {
                out.push_str(&format!("  Distinct states: {}\n", n));
            }
            out
        }
        TlcResult::Fail {
            violated_property,
            counterexample,
        } => {
            let mut out = format!("FAIL: {} violated for {}\n", violated_property, sm_name);
            if !counterexample.is_empty() {
                out.push_str(&format!(
                    "\nCounterexample ({} steps):\n",
                    counterexample.len()
                ));
                for step in counterexample {
                    if step.is_stuttering {
                        out.push_str(&format!(
                            "  Step {}: *** STUTTERING ***\n",
                            step.step_number
                        ));
                    } else {
                        let tag = step.state_tag.as_deref().unwrap_or("?");
                        let fields_str = step
                            .fields
                            .iter()
                            .map(|(k, v)| format!("{} = {}", k, v))
                            .collect::<Vec<_>>()
                            .join(", ");
                        if fields_str.is_empty() {
                            out.push_str(&format!("  Step {}: {}\n", step.step_number, tag));
                        } else {
                            out.push_str(&format!(
                                "  Step {}: {} {{ {} }}\n",
                                step.step_number, tag, fields_str
                            ));
                        }
                    }
                }
            }
            out
        }
        TlcResult::Error { message } => {
            format!("ERROR: TLC error for {}:\n  {}\n", sm_name, message)
        }
    }
}
