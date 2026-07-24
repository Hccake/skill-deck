#![cfg(windows)]

#[test]
fn test_harness_starts_with_the_dialog_plugin_and_common_controls_manifest() {
    // 只有 Windows 成功加载 test executable 后才会进入这里；不显示 dialog，
    // 因为该测试防护的是 loader 回归，而不是 UI 交互。
    drop(tauri_plugin_dialog::init::<tauri::Wry>());
}
