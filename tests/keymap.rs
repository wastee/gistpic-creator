//! 快捷键字符串解析测试：`KeyBinding::new` 在解析失败时会 panic，
//! 用测试提前挡住启动就崩溃的问题。

#[test]
fn keybindings_parse_without_panic() {
    let bindings = gistpic_creator::app::keymap::bindings();
    assert!(
        bindings.len() >= 10,
        "快捷键绑定数量异常：{}",
        bindings.len()
    );
}
