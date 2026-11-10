//! 撤销 / 重做。
//!
//! 每一步保存一份项目快照。图片像素用 `Arc` 共享，所以快照本身很轻。

use crate::model::{Item, Mode, Project, Settings};

/// 一份项目快照。
#[derive(Clone)]
pub struct Snapshot {
    pub mode: Mode,
    pub settings: Settings,
    pub items: Vec<Item>,
}

impl Snapshot {
    fn of(project: &Project) -> Self {
        Snapshot {
            mode: project.mode,
            settings: project.settings.clone(),
            items: project.items.clone(),
        }
    }

    fn restore(self, project: &mut Project) {
        project.mode = self.mode;
        project.settings = self.settings;
        project.items = self.items;
    }
}

pub struct UndoStack {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    limit: usize,
}

impl UndoStack {
    pub fn new(limit: usize) -> Self {
        UndoStack {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// 在改动项目之前调用，记录当前状态。
    pub fn record(&mut self, project: &Project) {
        self.undo.push(Snapshot::of(project));
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot::of(project));
        snapshot.restore(project);
        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot::of(project));
        snapshot.restore(project);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[allow(dead_code)] // 键盘重做已经能用，界面暂时只放撤销按钮。
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
