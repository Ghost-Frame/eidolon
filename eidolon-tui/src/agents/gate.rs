use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DangerLevel {
    Safe,
    Warning,
    Critical,
}

#[derive(Debug, Clone)]
pub struct GateResult {
    pub is_dangerous: bool,
    pub level: DangerLevel,
    pub reasons: Vec<String>,
    pub original_command: String,
}

pub struct GateCheck;

impl GateCheck {
    pub fn check(command: &str) -> GateResult {
        let mut reasons = Vec::new();
        let mut level = DangerLevel::Safe;

        let critical_patterns: Vec<(&str, &str)> = vec![
            (r"rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|(-[a-zA-Z]*f[a-zA-Z]*r))\s", "Recursive force delete (rm -rf)"),
            (r"rm\s+-rf\s", "Recursive force delete (rm -rf)"),
            (r"git\s+push\s+.*--force", "Force push (can overwrite remote history)"),
            (r"git\s+push\s+-f\s", "Force push (can overwrite remote history)"),
            (r"git\s+reset\s+--hard", "Hard reset (destroys uncommitted changes)"),
            (r"(?i)DROP\s+(TABLE|DATABASE|SCHEMA|INDEX)", "SQL DROP operation"),
            (r"(?i)TRUNCATE\s+TABLE", "SQL TRUNCATE operation"),
            (r"(?i)DELETE\s+FROM\s+\w+\s*;?\s*$", "Unqualified DELETE (no WHERE clause)"),
            (r"mkfs\.", "Filesystem format operation"),
            (r"dd\s+if=", "Direct disk write (dd)"),
            (r">\s*/dev/sd[a-z]", "Direct write to block device"),
            (r"chmod\s+-R\s+777\s+/", "Recursive world-writable permissions on root"),
            (r"sshd_config", "SSH daemon configuration change"),
            (r"AllowUsers", "SSH user access control change"),
            (r"PermitRootLogin", "SSH root login setting change"),
            (r"iptables\s+-F", "Flush all firewall rules"),
            (r"ufw\s+disable", "Disable firewall"),
        ];

        let warning_patterns: Vec<(&str, &str)> = vec![
            (r"systemctl\s+(stop|restart|disable)", "Service state change"),
            (r"docker\s+(rm|rmi|system\s+prune)", "Docker resource removal"),
            (r"podman\s+(rm|rmi|system\s+prune)", "Podman resource removal"),
            (r"kill\s+-9", "Force kill process"),
            (r"pkill\s", "Process kill by name"),
            (r"reboot", "System reboot"),
            (r"shutdown", "System shutdown"),
            (r"apt\s+(remove|purge|autoremove)", "Package removal"),
            (r"dnf\s+(remove|erase)", "Package removal"),
            (r"pip\s+uninstall", "Python package removal"),
            (r"npm\s+uninstall", "Node package removal"),
            (r"git\s+branch\s+-[dD]", "Branch deletion"),
            (r"git\s+stash\s+drop", "Stash drop"),
        ];

        for (pattern, reason) in &critical_patterns {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(command) {
                    reasons.push(reason.to_string());
                    level = DangerLevel::Critical;
                }
            }
        }

        if level != DangerLevel::Critical {
            for (pattern, reason) in &warning_patterns {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(command) {
                        reasons.push(reason.to_string());
                        if level == DangerLevel::Safe {
                            level = DangerLevel::Warning;
                        }
                    }
                }
            }
        }

        GateResult {
            is_dangerous: level != DangerLevel::Safe,
            level,
            reasons,
            original_command: command.to_string(),
        }
    }

    pub fn format_warning(result: &GateResult) -> String {
        if !result.is_dangerous {
            return String::new();
        }

        let severity = match result.level {
            DangerLevel::Critical => "CRITICAL",
            DangerLevel::Warning => "WARNING",
            DangerLevel::Safe => return String::new(),
        };

        let reasons_str = result.reasons.join(", ");
        format!(
            "[{}] Dangerous operation detected: {}\nCommand: {}\nApprove? (y/n)",
            severity, reasons_str, result.original_command
        )
    }
}
