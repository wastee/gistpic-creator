//! 键盘快捷键对应的动作。

use gpui::actions;

actions!([
    PasteImage,
    OpenImages,
    ExportPng,
    CopyImage,
    UndoEdit,
    RedoEdit,
    RemoveSelected,
    ClearAll,
    SelectAll,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Dismiss,
    Quit,
]);
