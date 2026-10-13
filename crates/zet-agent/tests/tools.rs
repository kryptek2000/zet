use zet_agent::tools::unified_diff;

#[test]
fn identical_is_no_changes() {
    assert_eq!(unified_diff("a\nb\n", "a\nb\n"), "(no changes)");
}

#[test]
fn change_marks_lines() {
    let d = unified_diff("a\nb\nc\n", "a\nB\nc\nd\n");
    assert!(d.contains("-b"), "{d}");
    assert!(d.contains("+B"), "{d}");
    assert!(d.contains("+d"), "{d}");
    assert!(d.contains("@@"), "{d}");
}

#[test]
fn created_new_shows_all_added() {
    let d = unified_diff("", "x\ny\n");
    assert!(d.contains("+x") && d.contains("+y"), "{d}");
    let dels: Vec<&str> = d
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .collect();
    assert!(dels.is_empty(), "{d}");
}

#[test]
fn huge_inputs_degrade_to_counts() {
    let big = "l\n".repeat(500);
    let d = unified_diff(&big, &format!("{big}extra\n"));
    assert!(d.contains("too large"), "{d}");
}
