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
