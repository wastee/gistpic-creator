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
    ZoomIn,
    ZoomOut,
    ZoomReset,
    DialogSkip,
    Quit,
]);
