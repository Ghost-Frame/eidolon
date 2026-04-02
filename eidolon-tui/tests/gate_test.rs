#[test]
fn test_detects_rm_rf() {
    use eidolon_tui::agents::gate::{GateCheck, DangerLevel};

    let result = GateCheck::check("rm -rf /var/data");
    assert!(result.is_dangerous);
    assert_eq!(result.level, DangerLevel::Critical);
    assert!(!result.reasons.is_empty());
}

#[test]
fn test_detects_force_push() {
    use eidolon_tui::agents::gate::{GateCheck, DangerLevel};

    let result = GateCheck::check("git push --force origin main");
    assert!(result.is_dangerous);
    assert_eq!(result.level, DangerLevel::Critical);
}

#[test]
fn test_detects_drop_table() {
    use eidolon_tui::agents::gate::{GateCheck, DangerLevel};

    let result = GateCheck::check("DROP TABLE users;");
    assert!(result.is_dangerous);
    assert_eq!(result.level, DangerLevel::Critical);
}

#[test]
fn test_detects_systemctl_stop() {
    use eidolon_tui::agents::gate::{GateCheck, DangerLevel};

    let result = GateCheck::check("systemctl stop engram");
    assert!(result.is_dangerous);
    assert_eq!(result.level, DangerLevel::Warning);
}

#[test]
fn test_safe_commands_pass() {
    use eidolon_tui::agents::gate::GateCheck;

    let result = GateCheck::check("ls -la /tmp");
    assert!(!result.is_dangerous);

    let result = GateCheck::check("cat /etc/hostname");
    assert!(!result.is_dangerous);

    let result = GateCheck::check("git status");
    assert!(!result.is_dangerous);
}

#[test]
fn test_detects_ssh_config_changes() {
    use eidolon_tui::agents::gate::{GateCheck, DangerLevel};

    let result = GateCheck::check("sed -i 's/AllowUsers.*/AllowUsers nobody/' /etc/ssh/sshd_config");
    assert!(result.is_dangerous);
    assert_eq!(result.level, DangerLevel::Critical);
}

#[test]
fn test_detects_chmod_recursive() {
    use eidolon_tui::agents::gate::GateCheck;

    let result = GateCheck::check("chmod -R 777 /");
    assert!(result.is_dangerous);
}

// T1: Comprehensive gate pattern regression tests

#[test]
fn test_critical_patterns_blocked() {
    use eidolon_tui::agents::gate::{GateCheck, DangerLevel};

    let critical_commands = vec![
        ("rm -rf /", "rm -rf root"),
        ("rm -rf /home/zan", "rm -rf /home"),
        ("rm -rf ~/", "rm -rf home dir"),
        ("git push --force origin main", "force push main"),
        ("git push -f origin master", "force push master"),
        ("git reset --hard HEAD~5", "hard reset"),
        ("DROP TABLE users", "SQL DROP TABLE"),
        ("TRUNCATE TABLE sessions", "SQL TRUNCATE"),
        ("DELETE FROM users;", "unqualified DELETE"),
        ("mkfs.ext4 /dev/sda1", "filesystem format"),
        ("dd if=/dev/zero of=/dev/sda", "disk write"),
        ("chmod -R 777 /", "recursive world-writable root"),
        ("echo 'AllowUsers nobody' >> /etc/ssh/sshd_config", "sshd_config change"),
        ("sed -i 's/PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config", "permit root login"),
        ("iptables -F", "flush firewall"),
        ("ufw disable", "disable firewall"),
    ];

    for (cmd, label) in &critical_commands {
        let result = GateCheck::check(cmd);
        assert!(
            result.is_dangerous && result.level == DangerLevel::Critical,
            "Expected CRITICAL for '{}' ({}), got {:?}",
            cmd, label, result.level
        );
    }
}

#[test]
fn test_warning_patterns_flagged() {
    use eidolon_tui::agents::gate::{GateCheck, DangerLevel};

    let warning_commands = vec![
        ("systemctl stop nginx", "stop service"),
        ("systemctl restart postgresql", "restart service"),
        ("docker rm container1", "docker rm"),
        ("podman rmi myimage", "podman rmi"),
        ("kill -9 1234", "force kill"),
        ("pkill node", "pkill"),
        ("reboot", "reboot"),
        ("shutdown -h now", "shutdown"),
        ("apt remove nginx", "apt remove"),
        ("dnf remove httpd", "dnf remove"),
        ("pip uninstall requests", "pip uninstall"),
        ("npm uninstall express", "npm uninstall"),
        ("git branch -D feature", "branch delete"),
        ("git stash drop", "stash drop"),
    ];

    for (cmd, label) in &warning_commands {
        let result = GateCheck::check(cmd);
        assert!(
            result.is_dangerous && result.level == DangerLevel::Warning,
            "Expected WARNING for '{}' ({}), got {:?} (dangerous={})",
            cmd, label, result.level, result.is_dangerous
        );
    }
}

#[test]
fn test_safe_commands_not_flagged() {
    use eidolon_tui::agents::gate::GateCheck;

    let safe_commands = vec![
        "ls -la /tmp",
        "cat /etc/hostname",
        "git status",
        "git log --oneline",
        "git diff HEAD~1",
        "git push origin feature-branch",
        "git commit -m 'test'",
        "cargo build",
        "cargo test",
        "npm install",
        "pip install requests",
        "docker ps",
        "docker logs container1",
        "systemctl status nginx",
        "journalctl -u nginx",
        "ssh zan@server ls",
        "curl http://localhost:8080/health",
        "echo 'hello world'",
        "grep -r 'pattern' .",
        "find . -name '*.rs'",
        "rm -f /tmp/test.txt",
        "rm /tmp/cache/*",
    ];

    for cmd in &safe_commands {
        let result = GateCheck::check(cmd);
        assert!(
            !result.is_dangerous,
            "Expected SAFE for '{}', got {:?} with reasons: {:?}",
            cmd, result.level, result.reasons
        );
    }
}

#[test]
fn test_gate_format_warning_critical() {
    use eidolon_tui::agents::gate::GateCheck;

    let result = GateCheck::check("rm -rf /var");
    let warning = GateCheck::format_warning(&result);
    assert!(warning.contains("CRITICAL"));
    assert!(warning.contains("rm -rf /var"));
}

#[test]
fn test_gate_format_warning_safe() {
    use eidolon_tui::agents::gate::GateCheck;

    let result = GateCheck::check("ls -la");
    let warning = GateCheck::format_warning(&result);
    assert!(warning.is_empty());
}
