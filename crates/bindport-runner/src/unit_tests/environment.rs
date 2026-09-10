// SPDX-License-Identifier: MIT

use std::{ffi::OsString, fs};

use crate::{
    spawn_child_on_port, spawn_child_on_port_with_context,
    tests::{signal_forwarding_test_lock, temp_test_path},
};

#[test]
fn spawn_child_context_sets_working_directory_and_environment() {
    let _lock = signal_forwarding_test_lock();
    let cwd = temp_test_path("child-context-cwd");
    fs::create_dir_all(&cwd).expect("create child cwd");
    let cwd = cwd.canonicalize().expect("canonical child cwd");
    let output_path = temp_test_path("child-context-output");
    let command = vec![
        String::from("sh"),
        String::from("-c"),
        String::from("printf '%s|%s|%s' \"$(pwd -P)\" \"$BINDPORT_TEST_ENV\" \"$PORT\" > \"$1\""),
        String::from("bindport-runner-context-test"),
        output_path.display().to_string(),
    ];
    let extra_env = vec![
        (
            OsString::from("BINDPORT_TEST_ENV"),
            OsString::from("context-value"),
        ),
        (OsString::from("PORT"), OsString::from("3000")),
    ];
    let mut child = spawn_child_on_port_with_context(&command, 29_000, Some(&cwd), &extra_env)
        .expect("spawn child with context");

    assert!(child.wait().expect("wait for child").success());
    assert_eq!(
        fs::read_to_string(output_path).expect("read child output"),
        format!("{}|context-value|29000", cwd.display())
    );
    assert_eq!(child.port(), 29_000);
}

#[test]
fn assigned_port_overrides_extra_env_in_string_api() {
    let _lock = signal_forwarding_test_lock();
    let command = vec![
        String::from("sh"),
        String::from("-c"),
        String::from("test \"$PORT\" = 29000 && test \"$BINDPORT_TEST_ENV\" = last"),
    ];

    for value in ["3000", "", "29000"] {
        let extra_env = vec![
            (String::from("PORT"), String::from("3001")),
            (String::from("BINDPORT_TEST_ENV"), String::from("first")),
            (String::from("PORT"), String::from(value)),
            (String::from("BINDPORT_TEST_ENV"), String::from("last")),
        ];
        let mut child = spawn_child_on_port(&command, 29_000, &extra_env).expect("spawn child");

        assert!(
            child.wait().expect("wait for child").success(),
            "PORT override {value:?}"
        );
        assert_eq!(child.port(), 29_000);
    }
}
