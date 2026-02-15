use lazy_static::lazy_static;
use regex::{Regex, RegexSet};

/// A rule mapping a shell command pattern to its RTK equivalent.
struct RtkRule {
    rtk_cmd: &'static str,
    category: &'static str,
    savings_pct: f64,
    subcmd_savings: &'static [(&'static str, f64)],
    subcmd_status: &'static [(&'static str, super::report::RtkStatus)],
}

/// Result of classifying a command.
#[derive(Debug, PartialEq)]
pub enum Classification {
    Supported {
        rtk_equivalent: &'static str,
        category: &'static str,
        estimated_savings_pct: f64,
        status: super::report::RtkStatus,
    },
    Unsupported {
        base_command: String,
    },
    Ignored,
}

/// Average token counts per category for estimation when no output_len available.
pub fn category_avg_tokens(category: &str, subcmd: &str) -> usize {
    match category {
        "Git" => match subcmd {
            "log" | "diff" | "show" => 200,
            _ => 40,
        },
        "Cargo" => match subcmd {
            "test" => 500,
            _ => 150,
        },
        "Tests" => 800,
        "Files" => 100,
        "Build" => 300,
        "Infra" => 120,
        "Cloud" => 220,
        "Data" => 260,
        "Scripts" => 180,
        "Network" => 150,
        "GitHub" => 200,
        "PackageManager" => 150,
        _ => 150,
    }
}

// Patterns ordered to match RTK_RULES indices exactly.
const PATTERNS: &[&str] = &[
    r"^git\s+(status|log|diff|show|add|commit|push|pull|branch|fetch|stash|worktree)",
    r"^gh\s+(pr|issue|run|repo|api)",
    r"^cargo\s+(build|test|clippy|check|fmt)",
    r"^pnpm\s+(list|ls|outdated|install)",
    r"^npm\s+(run|exec)",
    r"^npx\s+",
    r"^(cat|head|tail)\s+",
    r"^(rg|grep)\s+",
    r"^ls(\s|$)",
    r"^find\s+",
    r"^(npx\s+|pnpm\s+)?tsc(\s|$)",
    r"^(npx\s+|pnpm\s+)?(eslint|biome|lint)(\s|$)",
    r"^(npx\s+|pnpm\s+)?prettier",
    r"^(npx\s+|pnpm\s+)?next\s+build",
    r"^(pnpm\s+|npx\s+)?(vitest|jest|test)(\s|$)",
    r"^(npx\s+|pnpm\s+)?playwright",
    r"^(npx\s+|pnpm\s+)?prisma",
    r"^docker\s+(compose|ps|images|logs|run|build|exec)(\s|$)",
    r"^kubectl\s+(get|logs|describe|apply)(\s|$)",
    r"^gcloud(\s|$)",
    r"^bq(\s|$)",
    r"^sqlite3(\s|$)",
    r"^curl\s+",
    r"^wget\s+",
    r"^python(3)?(\s|$)",
    r"^(python(3)?\s+-m\s+)?pytest(\s|$)",
    r"^(python(3)?\s+-m\s+)?ruff(\s|$)",
    r"^pip\s+(list|outdated|install|show)(\s|$)",
];

const RULES: &[RtkRule] = &[
    RtkRule {
        rtk_cmd: "rtk git",
        category: "Git",
        savings_pct: 70.0,
        subcmd_savings: &[
            ("diff", 80.0),
            ("show", 80.0),
            ("add", 59.0),
            ("commit", 59.0),
        ],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk gh",
        category: "GitHub",
        savings_pct: 82.0,
        subcmd_savings: &[("pr", 87.0), ("run", 82.0), ("issue", 80.0)],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk cargo",
        category: "Cargo",
        savings_pct: 80.0,
        subcmd_savings: &[("test", 90.0), ("check", 80.0)],
        subcmd_status: &[("fmt", super::report::RtkStatus::Passthrough)],
    },
    RtkRule {
        rtk_cmd: "rtk pnpm",
        category: "PackageManager",
        savings_pct: 80.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk npm",
        category: "PackageManager",
        savings_pct: 70.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk npx",
        category: "PackageManager",
        savings_pct: 70.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk read",
        category: "Files",
        savings_pct: 60.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk grep",
        category: "Files",
        savings_pct: 75.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk ls",
        category: "Files",
        savings_pct: 65.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk find",
        category: "Files",
        savings_pct: 70.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk tsc",
        category: "Build",
        savings_pct: 83.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk lint",
        category: "Build",
        savings_pct: 84.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk prettier",
        category: "Build",
        savings_pct: 70.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk next",
        category: "Build",
        savings_pct: 87.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk vitest",
        category: "Tests",
        savings_pct: 99.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk playwright",
        category: "Tests",
        savings_pct: 94.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk prisma",
        category: "Build",
        savings_pct: 88.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk docker",
        category: "Infra",
        savings_pct: 85.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk kubectl",
        category: "Infra",
        savings_pct: 85.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk proxy gcloud",
        category: "Cloud",
        savings_pct: 0.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk proxy bq",
        category: "Data",
        savings_pct: 0.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk proxy sqlite3",
        category: "Data",
        savings_pct: 0.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk curl",
        category: "Network",
        savings_pct: 70.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk wget",
        category: "Network",
        savings_pct: 65.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk proxy python",
        category: "Scripts",
        savings_pct: 0.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk pytest",
        category: "Tests",
        savings_pct: 90.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk ruff",
        category: "Build",
        savings_pct: 84.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
    RtkRule {
        rtk_cmd: "rtk pip",
        category: "PackageManager",
        savings_pct: 70.0,
        subcmd_savings: &[],
        subcmd_status: &[],
    },
];

/// Commands to ignore (shell builtins, trivial, already rtk).
const IGNORED_PREFIXES: &[&str] = &[
    "#",
    "//",
    "```",
    "cd ",
    "cd\t",
    "echo ",
    "printf ",
    "export ",
    "source ",
    "mkdir ",
    "rm ",
    "mv ",
    "cp ",
    "chmod ",
    "chown ",
    "touch ",
    "which ",
    "type ",
    "command ",
    "test ",
    "true",
    "false",
    "sleep ",
    "wait",
    "kill ",
    "set ",
    "unset ",
    "wc ",
    "sort ",
    "uniq ",
    "tr ",
    "cut ",
    "awk ",
    "sed ",
    "python3 -c",
    "python -c",
    "node -e",
    "ruby -e",
    "rtk ",
    "pwd",
    "bash ",
    "sh ",
    "then\n",
    "then ",
    "else\n",
    "else ",
    "fi",
    "do\n",
    "do ",
    "done",
    "for ",
    "while ",
    "if ",
    "case ",
];

const IGNORED_EXACT: &[&str] = &["cd", "echo", "true", "false", "wait", "pwd", "bash", "sh"];

lazy_static! {
    static ref REGEX_SET: RegexSet = RegexSet::new(PATTERNS).expect("invalid regex patterns");
    static ref COMPILED: Vec<Regex> = PATTERNS
        .iter()
        .map(|p| Regex::new(p).expect("invalid regex"))
        .collect();
    static ref ENV_PREFIX: Regex =
        Regex::new(r"^(?:sudo\s+|env\s+|[A-Z_][A-Z0-9_]*=[^\s]*\s+)+").unwrap();
    static ref RUNNER_PREFIX: Regex = Regex::new(
        r"^(?:(?:uv|poetry|pipenv|hatch|rye)\s+run\s+|(?:npm|pnpm)\s+exec(?:\s+--)?\s+)"
    )
    .unwrap();
    static ref BARE_ENV_TOKEN: Regex = Regex::new(r"^[A-Z_][A-Z0-9_]*$").unwrap();
}

fn is_ignored_command(cmd: &str) -> bool {
    if IGNORED_EXACT.contains(&cmd) {
        return true;
    }
    IGNORED_PREFIXES.iter().any(|prefix| cmd.starts_with(prefix))
}

fn normalize_command_for_classification(cmd: &str) -> Option<String> {
    let mut normalized = cmd.trim();
    if normalized.is_empty() {
        return None;
    }

    // For multiline snippets, keep only the first meaningful line.
    if normalized.contains('\n') {
        normalized = normalized
            .lines()
            .map(str::trim)
            .find(|line| {
                !line.is_empty()
                    && !line.starts_with('#')
                    && !line.starts_with("//")
                    && !line.starts_with("```")
            })
            .unwrap_or("");
    }

    if normalized.is_empty() {
        return None;
    }

    // Strip common shell prompt markers.
    loop {
        let stripped = if let Some(rest) = normalized.strip_prefix("$ ") {
            Some(rest)
        } else if let Some(rest) = normalized.strip_prefix("% ") {
            Some(rest)
        } else if let Some(rest) = normalized.strip_prefix("> ") {
            Some(rest)
        } else {
            None
        };

        if let Some(rest) = stripped {
            normalized = rest.trim_start();
        } else {
            break;
        }
    }

    // Allow escaped command names like "\ cp file".
    if let Some(rest) = normalized.strip_prefix('\\') {
        normalized = rest.trim_start();
    }

    if normalized.is_empty() {
        return None;
    }

    // Noise guard: bare env-like symbol (e.g., ADMIN_TOKEN) is not a command.
    if BARE_ENV_TOKEN.is_match(normalized) {
        return None;
    }

    Some(normalized.to_string())
}

/// Classify a single (already-split) command.
pub fn classify_command(cmd: &str) -> Classification {
    let normalized = match normalize_command_for_classification(cmd) {
        Some(c) => c,
        None => return Classification::Ignored,
    };

    if normalized.is_empty() {
        return Classification::Ignored;
    }

    if is_ignored_command(&normalized) {
        return Classification::Ignored;
    }

    // Strip env prefixes (sudo, env VAR=val, VAR=val)
    let stripped_env = ENV_PREFIX.replace(&normalized, "");
    // Strip runner wrappers so wrapped commands classify the same way.
    let stripped_runner = RUNNER_PREFIX.replace(stripped_env.trim(), "");
    let cmd_clean = stripped_runner.trim();
    if cmd_clean.is_empty() {
        return Classification::Ignored;
    }
    if is_ignored_command(cmd_clean) {
        return Classification::Ignored;
    }

    // Fast check with RegexSet — take the last (most specific) match
    let matches: Vec<usize> = REGEX_SET.matches(cmd_clean).into_iter().collect();
    if let Some(&idx) = matches.last() {
        let rule = &RULES[idx];

        // Extract subcommand for savings override and status detection
        let (savings, status) = if let Some(caps) = COMPILED[idx].captures(cmd_clean) {
            if let Some(sub) = caps.get(1) {
                let subcmd = sub.as_str();
                // Check if this subcommand has a special status
                let status = rule
                    .subcmd_status
                    .iter()
                    .find(|(s, _)| *s == subcmd)
                    .map(|(_, st)| *st)
                    .unwrap_or_else(|| {
                        if rule.rtk_cmd.starts_with("rtk proxy ") {
                            super::report::RtkStatus::Passthrough
                        } else {
                            super::report::RtkStatus::Existing
                        }
                    });

                // Check if this subcommand has custom savings
                let savings = rule
                    .subcmd_savings
                    .iter()
                    .find(|(s, _)| *s == subcmd)
                    .map(|(_, pct)| *pct)
                    .unwrap_or(rule.savings_pct);

                (savings, status)
            } else {
                (rule.savings_pct, super::report::RtkStatus::Existing)
            }
        } else {
            let status = if rule.rtk_cmd.starts_with("rtk proxy ") {
                super::report::RtkStatus::Passthrough
            } else {
                super::report::RtkStatus::Existing
            };
            (rule.savings_pct, status)
        };

        Classification::Supported {
            rtk_equivalent: rule.rtk_cmd,
            category: rule.category,
            estimated_savings_pct: savings,
            status,
        }
    } else {
        // Extract base command for unsupported
        let base = extract_base_command(cmd_clean);
        if base.is_empty() {
            Classification::Ignored
        } else {
            Classification::Unsupported {
                base_command: base.to_string(),
            }
        }
    }
}

/// Extract the base command (first word, or first two if it looks like a subcommand pattern).
fn extract_base_command(cmd: &str) -> &str {
    let parts: Vec<&str> = cmd.splitn(3, char::is_whitespace).collect();
    match parts.len() {
        0 => "",
        1 => parts[0],
        _ => {
            let second = parts[1];
            // If the second token looks like a subcommand (no leading -)
            if !second.starts_with('-') && !second.contains('/') && !second.contains('.') {
                // Return "cmd subcmd"
                let end = cmd
                    .find(char::is_whitespace)
                    .and_then(|i| {
                        let rest = &cmd[i..];
                        let trimmed = rest.trim_start();
                        trimmed
                            .find(char::is_whitespace)
                            .map(|j| i + (rest.len() - trimmed.len()) + j)
                    })
                    .unwrap_or(cmd.len());
                &cmd[..end]
            } else {
                parts[0]
            }
        }
    }
}

/// Split a command chain on `&&`, `||`, `;` outside quotes.
/// For pipes `|`, only keep the first command.
/// Lines with `<<` (heredoc) or `$((` are returned whole.
pub fn split_command_chain(cmd: &str) -> Vec<&str> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return vec![];
    }

    // Heredoc or arithmetic expansion: treat as single command
    if trimmed.contains("<<") || trimmed.contains("$((") {
        return vec![trimmed];
    }

    let mut results = Vec::new();
    let mut start = 0;
    let bytes = trimmed.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut pipe_seen = false;

    while i < len {
        let b = bytes[i];
        match b {
            b'\'' if !in_double => {
                in_single = !in_single;
                i += 1;
            }
            b'"' if !in_single => {
                in_double = !in_double;
                i += 1;
            }
            b'|' if !in_single && !in_double => {
                if i + 1 < len && bytes[i + 1] == b'|' {
                    // ||
                    let segment = trimmed[start..i].trim();
                    if !segment.is_empty() {
                        results.push(segment);
                    }
                    i += 2;
                    start = i;
                } else {
                    // pipe: keep only first command
                    let segment = trimmed[start..i].trim();
                    if !segment.is_empty() {
                        results.push(segment);
                    }
                    pipe_seen = true;
                    break;
                }
            }
            b'&' if !in_single && !in_double && i + 1 < len && bytes[i + 1] == b'&' => {
                let segment = trimmed[start..i].trim();
                if !segment.is_empty() {
                    results.push(segment);
                }
                i += 2;
                start = i;
            }
            b';' if !in_single && !in_double => {
                let segment = trimmed[start..i].trim();
                if !segment.is_empty() {
                    results.push(segment);
                }
                i += 1;
                start = i;
            }
            b'\n' | b'\r' if !in_single && !in_double => {
                // Respect line continuation with trailing backslash.
                if i > 0 && bytes[i - 1] == b'\\' {
                    i += 1;
                    continue;
                }
                let segment = trimmed[start..i].trim();
                if !segment.is_empty() {
                    results.push(segment);
                }
                i += 1;
                start = i;
            }
            _ => {
                i += 1;
            }
        }
    }

    if !pipe_seen && start < len {
        let segment = trimmed[start..].trim();
        if !segment.is_empty() {
            results.push(segment);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::super::report::RtkStatus;
    use super::*;

    #[test]
    fn test_classify_git_status() {
        assert_eq!(
            classify_command("git status"),
            Classification::Supported {
                rtk_equivalent: "rtk git",
                category: "Git",
                estimated_savings_pct: 70.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_git_diff_cached() {
        assert_eq!(
            classify_command("git diff --cached"),
            Classification::Supported {
                rtk_equivalent: "rtk git",
                category: "Git",
                estimated_savings_pct: 80.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_cargo_test_filter() {
        assert_eq!(
            classify_command("cargo test filter::"),
            Classification::Supported {
                rtk_equivalent: "rtk cargo",
                category: "Cargo",
                estimated_savings_pct: 90.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_npx_tsc() {
        assert_eq!(
            classify_command("npx tsc --noEmit"),
            Classification::Supported {
                rtk_equivalent: "rtk tsc",
                category: "Build",
                estimated_savings_pct: 83.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_cat_file() {
        assert_eq!(
            classify_command("cat src/main.rs"),
            Classification::Supported {
                rtk_equivalent: "rtk read",
                category: "Files",
                estimated_savings_pct: 60.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_cd_ignored() {
        assert_eq!(classify_command("cd /tmp"), Classification::Ignored);
    }

    #[test]
    fn test_classify_rtk_already() {
        assert_eq!(classify_command("rtk git status"), Classification::Ignored);
    }

    #[test]
    fn test_classify_echo_ignored() {
        assert_eq!(
            classify_command("echo hello world"),
            Classification::Ignored
        );
    }

    #[test]
    fn test_classify_terraform_unsupported() {
        match classify_command("terraform plan -var-file=prod.tfvars") {
            Classification::Unsupported { base_command } => {
                assert_eq!(base_command, "terraform plan");
            }
            other => panic!("expected Unsupported, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_env_prefix_stripped() {
        assert_eq!(
            classify_command("GIT_SSH_COMMAND=ssh git push"),
            Classification::Supported {
                rtk_equivalent: "rtk git",
                category: "Git",
                estimated_savings_pct: 70.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_sudo_stripped() {
        assert_eq!(
            classify_command("sudo docker ps"),
            Classification::Supported {
                rtk_equivalent: "rtk docker",
                category: "Infra",
                estimated_savings_pct: 85.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_cargo_check() {
        assert_eq!(
            classify_command("cargo check"),
            Classification::Supported {
                rtk_equivalent: "rtk cargo",
                category: "Cargo",
                estimated_savings_pct: 80.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_cargo_check_all_targets() {
        assert_eq!(
            classify_command("cargo check --all-targets"),
            Classification::Supported {
                rtk_equivalent: "rtk cargo",
                category: "Cargo",
                estimated_savings_pct: 80.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_cargo_fmt_passthrough() {
        assert_eq!(
            classify_command("cargo fmt"),
            Classification::Supported {
                rtk_equivalent: "rtk cargo",
                category: "Cargo",
                estimated_savings_pct: 80.0,
                status: RtkStatus::Passthrough,
            }
        );
    }

    #[test]
    fn test_classify_cargo_clippy_savings() {
        assert_eq!(
            classify_command("cargo clippy --all-targets"),
            Classification::Supported {
                rtk_equivalent: "rtk cargo",
                category: "Cargo",
                estimated_savings_pct: 80.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_runner_wrapped_pytest() {
        assert_eq!(
            classify_command("uv run python -m pytest tests/ -q"),
            Classification::Supported {
                rtk_equivalent: "rtk pytest",
                category: "Tests",
                estimated_savings_pct: 90.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_runner_wrapped_ruff() {
        assert_eq!(
            classify_command("poetry run ruff check ."),
            Classification::Supported {
                rtk_equivalent: "rtk ruff",
                category: "Build",
                estimated_savings_pct: 84.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_runner_wrapped_already_rtk_ignored() {
        assert_eq!(
            classify_command("uv run rtk git status"),
            Classification::Ignored
        );
    }

    #[test]
    fn test_classify_npm_exec_eslint() {
        assert_eq!(
            classify_command("npm exec -- eslint ."),
            Classification::Supported {
                rtk_equivalent: "rtk lint",
                category: "Build",
                estimated_savings_pct: 84.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_docker_compose() {
        assert_eq!(
            classify_command("docker compose up -d"),
            Classification::Supported {
                rtk_equivalent: "rtk docker",
                category: "Infra",
                estimated_savings_pct: 85.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_kubectl_describe() {
        assert_eq!(
            classify_command("kubectl describe pod app"),
            Classification::Supported {
                rtk_equivalent: "rtk kubectl",
                category: "Infra",
                estimated_savings_pct: 85.0,
                status: RtkStatus::Existing,
            }
        );
    }

    #[test]
    fn test_classify_gcloud_supported_passthrough() {
        assert_eq!(
            classify_command("gcloud run services list"),
            Classification::Supported {
                rtk_equivalent: "rtk proxy gcloud",
                category: "Cloud",
                estimated_savings_pct: 0.0,
                status: RtkStatus::Passthrough,
            }
        );
    }

    #[test]
    fn test_classify_bq_supported_passthrough() {
        assert_eq!(
            classify_command("bq query --nouse_legacy_sql 'select 1'"),
            Classification::Supported {
                rtk_equivalent: "rtk proxy bq",
                category: "Data",
                estimated_savings_pct: 0.0,
                status: RtkStatus::Passthrough,
            }
        );
    }

    #[test]
    fn test_classify_sqlite3_supported_passthrough() {
        assert_eq!(
            classify_command("sqlite3 app.db '.tables'"),
            Classification::Supported {
                rtk_equivalent: "rtk proxy sqlite3",
                category: "Data",
                estimated_savings_pct: 0.0,
                status: RtkStatus::Passthrough,
            }
        );
    }

    #[test]
    fn test_classify_runner_wrapped_python_passthrough() {
        assert_eq!(
            classify_command("uv run python scripts/audit.py"),
            Classification::Supported {
                rtk_equivalent: "rtk proxy python",
                category: "Scripts",
                estimated_savings_pct: 0.0,
                status: RtkStatus::Passthrough,
            }
        );
    }

    #[test]
    fn test_classify_noise_comment_ignored() {
        assert_eq!(classify_command("# Check credentials"), Classification::Ignored);
    }

    #[test]
    fn test_classify_noise_bare_env_symbol_ignored() {
        assert_eq!(classify_command("ADMIN_TOKEN"), Classification::Ignored);
    }

    #[test]
    fn test_classify_escaped_cp_ignored() {
        assert_eq!(
            classify_command("\\ cp source.txt target.txt"),
            Classification::Ignored
        );
    }

    #[test]
    fn test_patterns_rules_length_match() {
        assert_eq!(
            PATTERNS.len(),
            RULES.len(),
            "PATTERNS and RULES must be aligned"
        );
    }

    #[test]
    fn test_registry_covers_all_cargo_subcommands() {
        // Verify that every CargoCommand variant (Build, Test, Clippy, Check, Fmt)
        // except Other has a matching pattern in the registry
        for subcmd in ["build", "test", "clippy", "check", "fmt"] {
            let cmd = format!("cargo {subcmd}");
            match classify_command(&cmd) {
                Classification::Supported { .. } => {}
                other => panic!("cargo {subcmd} should be Supported, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_registry_covers_all_git_subcommands() {
        // Verify that every GitCommand subcommand has a matching pattern
        for subcmd in [
            "status", "log", "diff", "show", "add", "commit", "push", "pull", "branch", "fetch",
            "stash", "worktree",
        ] {
            let cmd = format!("git {subcmd}");
            match classify_command(&cmd) {
                Classification::Supported { .. } => {}
                other => panic!("git {subcmd} should be Supported, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_split_chain_and() {
        assert_eq!(split_command_chain("a && b"), vec!["a", "b"]);
    }

    #[test]
    fn test_split_chain_semicolon() {
        assert_eq!(split_command_chain("a ; b"), vec!["a", "b"]);
    }

    #[test]
    fn test_split_pipe_first_only() {
        assert_eq!(split_command_chain("a | b"), vec!["a"]);
    }

    #[test]
    fn test_split_single() {
        assert_eq!(split_command_chain("git status"), vec!["git status"]);
    }

    #[test]
    fn test_split_quoted_and() {
        assert_eq!(
            split_command_chain(r#"echo "a && b""#),
            vec![r#"echo "a && b""#]
        );
    }

    #[test]
    fn test_split_heredoc_no_split() {
        let cmd = "cat <<'EOF'\nhello && world\nEOF";
        assert_eq!(split_command_chain(cmd), vec![cmd]);
    }

    #[test]
    fn test_split_newline() {
        assert_eq!(split_command_chain("git status\nls -la"), vec!["git status", "ls -la"]);
    }

    #[test]
    fn test_split_newline_with_line_continuation() {
        assert_eq!(
            split_command_chain("echo hello \\\nworld"),
            vec!["echo hello \\\nworld"]
        );
    }
}
