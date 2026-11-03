//! [`benilla_formats::set_data_dir`] ends the install ladder. It fixes a process-global once, so
//! this one test has a file, and so a process, of its own.

#[test]
fn a_chosen_install_is_the_only_candidate() {
    let chosen = std::env::temp_dir().join(format!("benilla-chosen-{}", std::process::id()));
    std::fs::create_dir_all(&chosen).unwrap();
    std::env::set_var("WOW_DATA", std::env::temp_dir());

    benilla_formats::set_data_dir(Some(chosen.clone()));
    benilla_formats::set_data_dir(None);

    assert_eq!(benilla_formats::candidates(), [chosen.clone()]);
    assert_eq!(benilla_formats::wow_data(), Some(chosen.clone()));
    std::fs::remove_dir_all(&chosen).ok();
    assert_eq!(
        benilla_formats::wow_data(),
        None,
        "a chosen install that is gone must not fall through to $WOW_DATA"
    );
}
