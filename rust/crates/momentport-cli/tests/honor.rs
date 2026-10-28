//! 从实际扫描入口验证仅文件名识别及 App 使用的 JSON 预览/转换契约。
use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "momentport-honor-cli-{}",
            momentport_core::uuid::new_uuid_upper()
        ));
        std::fs::create_dir_all(path.join("input")).unwrap();
        // 无 EXIF；封面故意为第三帧，避免只验证隐含的“选第一帧”。
        for frame in ["002", "005", "090_COVER"] {
            std::fs::write(
                path.join("input")
                    .join(format!("IMG_20230108_205027_BURST{}.JPG", frame)),
                [0xff, 0xd8, 0xff, 0xd9],
            )
            .unwrap();
        }
        Self(path)
    }
    fn run(&self, flags: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_momentport-cli"))
            .arg(self.0.join("input"))
            .args(flags)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn filename_only_preview_cover_and_cli_overrides() {
    let f = Fixture::new();
    let preview = f.run(&["--list-json", "--gap", "0"]);
    assert!(preview.contains("\"keyIndex\": 2"));
    assert!(preview.contains("荣耀连拍文件名"));
    assert!(preview.find("BURST002").unwrap() < preview.find("BURST090").unwrap());
    assert!(f
        .run(&["--list-json", "--key-index", "0"])
        .contains("\"keyIndex\": 0"));
    assert!(f
        .run(&["--list-json", "--key-indexes", "1"])
        .contains("\"keyIndex\": 1"));
    assert!(f
        .run(&["--list-json", "--key-indexes", "99"])
        .contains("\"keyIndex\": 2"));
    assert_eq!(std::fs::read_dir(&f.0).unwrap().count(), 1); // 预览无输出文件。

    let output = f.0.join("output");
    f.run(&[output.to_str().unwrap()]);
    let manifest = std::fs::read_to_string(output.join("Burst_01/BurstManifest.json")).unwrap();
    assert!(manifest.contains("\"keyFrameIndex\": 2"));
    assert!(manifest.contains("\"keyFrameFileName\": \"IMG_20230108_205027_BURST090_COVER.JPG\""));
    assert_eq!(manifest.matches("\"isKeyFrame\": true").count(), 1);
}
