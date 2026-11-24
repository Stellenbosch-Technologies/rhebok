#![cfg(not(feature = "shoot"))]

use std::process::Command;

#[test]
fn a_build_without_the_shoot_feature_refuses_the_screenshot_flags() {
    for flag in ["--screenshot", "--side-by-side", "--measure-graph"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rhebok-app"))
            .arg(flag)
            .output()
            .expect("run rhebok-app");
        assert_eq!(output.status.code(), Some(2), "{flag}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(
                "rhebok-app was built without screenshot support; rebuild with --features shoot"
            ),
            "{flag}"
        );
    }
}
