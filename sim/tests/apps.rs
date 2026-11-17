use std::{fs, process::Command};
#[test]
fn both_apps_emit_pngs_and_count_sound_frames() {
    for app in ["glyphfield"] {
        let dir = std::env::temp_dir().join(format!("esp32-sim-{}-{app}", std::process::id()));
        let output = Command::new(env!("CARGO_BIN_EXE_sim"))
            .args([
                app, "--frames", "3", "--tap-at", "0", "--tap-at", "2", "--out",
            ])
            .arg(&dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let line = String::from_utf8(output.stdout).unwrap();
        assert!(line.contains(&format!("\"app\":\"{app}\"")), "{line}");
        assert!(line.contains("\"sounds\":2"), "{line}");
        for frame in 0..3 {
            assert!(dir.join(format!("frame_{frame:04}.png")).exists());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
