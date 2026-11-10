use gistpic_creator::i18n::Language;
use gistpic_creator::model::{LayoutTarget, Mode, Rgba};

#[test]
fn default_language_is_simplified_chinese() {
    assert_eq!(Language::default(), Language::ZhCn);
    assert_eq!(Language::default().app_name(), "省流图创造机");
    assert_eq!(Language::EnUs.app_name(), "GistPic Creator");
}

#[test]
fn both_languages_have_distinct_visible_labels() {
    assert_ne!(
        Language::ZhCn.mode_label(Mode::Subtitle),
        Language::EnUs.mode_label(Mode::Subtitle)
    );
    assert_eq!(Language::ZhCn.mode_label(Mode::Subtitle), "字幕模式");
    assert_eq!(Language::ZhCn.mode_label(Mode::Layout), "排版模式");
    assert_eq!(Language::ZhCn.manual_crop(), "手动框选");
    assert_eq!(Language::EnUs.manual_crop(), "Manual crop");
    assert_eq!(Language::ZhCn.layout_label(LayoutTarget::Row), "横向排列");
    assert_eq!(Language::EnUs.layout_label(LayoutTarget::Row), "Row");
    assert_eq!(Language::ZhCn.close_to_tray(), "关闭窗口时隐藏到托盘");
    assert_eq!(Language::EnUs.close_to_tray(), "Hide to tray when closing");
    assert!(Language::ZhCn.close_hint().contains("隐藏窗口"));
    assert!(Language::EnUs.close_hint().contains("hides the window"));
}

#[test]
fn gesture_copy_is_bilingual() {
    assert_eq!(Language::ZhCn.snap_single_line(), "单行字幕");
    assert_eq!(Language::ZhCn.snap_two_lines(), "双行字幕");
    assert_eq!(Language::ZhCn.snap_whole_image(), "整张画面");
    assert_eq!(Language::EnUs.snap_single_line(), "Single line");
    assert_eq!(Language::EnUs.snap_two_lines(), "Two lines");
    assert_eq!(Language::EnUs.snap_whole_image(), "Whole image");
    assert_eq!(Language::ZhCn.copy_short(), "复制");
    assert_eq!(Language::EnUs.copy_short(), "Copy");
    assert!(Language::ZhCn.delete_zone_idle(3).contains("3"));
    assert!(Language::ZhCn.delete_zone_hover().contains("松手"));
    assert!(Language::EnUs.delete_zone_hover().contains("Release"));
    assert_eq!(Language::ZhCn.gap_color_name(Rgba::WHITE), "纯白");
    assert_eq!(Language::ZhCn.gap_color_name(Rgba::CHARCOAL), "炭黑");
    assert_eq!(Language::ZhCn.gap_color_name(Rgba::WARM_GREY), "暖灰");
    assert_eq!(Language::EnUs.gap_color_name(Rgba::WHITE), "White");
}

#[test]
fn export_filename_follows_language() {
    let zh = Language::ZhCn.export_file_name("拼接", "20260101_120000");
    let en = Language::EnUs.export_file_name("Stitch", "20260101_120000");
    assert_eq!(zh, "省流图创造机_拼接_20260101_120000.png");
    assert_eq!(en, "GistPic_Creator_Stitch_20260101_120000.png");
}

#[test]
fn language_switches_all_status_text() {
    assert!(Language::ZhCn.default_hint("Ctrl+V").contains("粘贴"));
    assert!(Language::EnUs.default_hint("Ctrl+V").contains("Paste"));
    assert_eq!(Language::ZhCn.export_png(), "导出 PNG");
    assert_eq!(Language::EnUs.export_png(), "Export PNG");
}

#[test]
fn decode_error_detail_is_localized() {
    use gistpic_creator::imaging::decode::decode_bytes;

    let error = decode_bytes(b"not an image").expect_err("这段字节不是图片");
    assert!(error.localized(Language::ZhCn).contains("不是可识别的图片"));
    assert!(
        error
            .localized(Language::EnUs)
            .starts_with("Unrecognized or corrupted image")
    );
}
