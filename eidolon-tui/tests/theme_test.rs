#[test]
fn test_all_themes_exist() {
    use eidolon_tui::tui::theme::Theme;

    let names = Theme::available_names();
    assert!(names.contains(&"jujutsu"));
    assert!(names.contains(&"limitless"));
    assert!(names.contains(&"cyberpunk"));
    assert!(names.contains(&"hollow"));
    assert!(names.contains(&"synapse"));
    assert!(names.contains(&"tokyo"));
    assert!(names.contains(&"minimal"));
    assert_eq!(names.len(), 7);
}

#[test]
fn test_theme_by_name() {
    use eidolon_tui::tui::theme::Theme;

    let theme = Theme::by_name("jujutsu").unwrap();
    assert_eq!(theme.name, "jujutsu");

    let theme = Theme::by_name("cyberpunk").unwrap();
    assert_eq!(theme.name, "cyberpunk");

    assert!(Theme::by_name("nonexistent").is_none());
}

#[test]
fn test_theme_cycle() {
    use eidolon_tui::tui::theme::Theme;

    let next = Theme::cycle_next("jujutsu");
    assert_eq!(next, "limitless");

    let next = Theme::cycle_next("minimal");
    assert_eq!(next, "jujutsu"); // wraps around
}

#[test]
fn test_default_theme_is_jujutsu() {
    use eidolon_tui::tui::theme::Theme;

    let theme = Theme::default();
    assert_eq!(theme.name, "jujutsu");
}
