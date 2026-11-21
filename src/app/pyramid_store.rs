//! 预览的源分辨率金字塔状态：每张图的顶层、层图块、LOD，以及源图解码缓存。
//!
//! 这里只管数据和几何，不起后台任务；AppRoot 负责把 plan 返回的缺块排进
//! GPU 后台队列，并在结果回来时调用 insert_top / insert_tile。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use gpui::RenderImage;
use image::DynamicImage;

use crate::app::pyramid::{
    self, BlockGeometry, LevelMeta, LodState, PyramidPlan, SourceRevision, TileKey,
};
use crate::model::{Item, ItemId};

/// 顶层最长边。超过这个值的图先缩到这一层常驻。
pub(crate) const TOP_MAX: u32 = 512;
/// 每张图最多保留多少图块像素，超过按最近使用淘汰。512 图块约 1MB。
const TILE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
/// 源图解码缓存上限。一张 4K RGB 约 24MB。
const SOURCE_BUDGET_BYTES: usize = 256 * 1024 * 1024;

/// 从 Item 生成源版本。指纹、宽高任一变化都会换版本。
pub(crate) fn revision_for(item: &Item) -> SourceRevision {
    SourceRevision {
        item: item.id,
        fingerprint: item.fingerprint.0,
        width: item.width,
        height: item.height,
    }
}

/// 一张图的源分辨率金字塔。
pub(crate) struct ImagePyramid {
    pub(crate) revision: SourceRevision,
    pub(crate) levels: Vec<LevelMeta>,
    pub(crate) top: Option<Arc<RenderImage>>,
    pub(crate) tiles: HashMap<TileKey, Arc<RenderImage>>,
    pub(crate) used: HashMap<TileKey, u64>,
    pub(crate) lod: LodState,
    pub(crate) failed: HashSet<TileKey>,
    pub(crate) top_failed: bool,
    tile_bytes: usize,
}

impl ImagePyramid {
    fn new(revision: SourceRevision, levels: Vec<LevelMeta>) -> Self {
        Self {
            revision,
            levels,
            top: None,
            tiles: HashMap::new(),
            used: HashMap::new(),
            lod: LodState::default(),
            failed: HashSet::new(),
            top_failed: false,
            tile_bytes: 0,
        }
    }

    pub(crate) fn top_level(&self) -> u32 {
        self.levels.len().saturating_sub(1) as u32
    }

    pub(crate) fn level(&self, index: u32) -> Option<&LevelMeta> {
        self.levels.get(index as usize)
    }

    /// 按当前显示尺寸和可见范围算一帧的渲染计划，并标记这一帧用到的块。
    pub(crate) fn plan(&mut self, geometry: &BlockGeometry, frame: u64) -> PyramidPlan {
        let revision = self.revision;
        let levels = &self.levels;
        let tiles = &self.tiles;
        let top = self.top.clone();
        let plan = pyramid::plan_block(
            revision,
            levels,
            top.as_ref(),
            &mut self.lod,
            geometry,
            frame,
            |key| tiles.get(key).cloned(),
        );
        let top_level = self.top_level();
        for layer in &plan.layers {
            for tile in &layer.tiles {
                if tile.key.level < top_level {
                    self.used.insert(tile.key, frame);
                }
            }
        }
        plan
    }

    /// 收下顶层。返回被替换掉的旧顶层，交给窗口释放。
    pub(crate) fn insert_top(&mut self, image: Arc<RenderImage>) -> Option<Arc<RenderImage>> {
        self.top_failed = false;
        self.top.replace(image)
    }

    /// 收下一块图块。成功时返回被替换的旧块；版本/层级不对时把新块退回去。
    pub(crate) fn insert_tile(
        &mut self,
        key: TileKey,
        image: Arc<RenderImage>,
    ) -> Result<Option<Arc<RenderImage>>, Arc<RenderImage>> {
        if key.revision != self.revision {
            return Err(image);
        }
        if self.level(key.level).is_none() {
            return Err(image);
        }
        let old = self.tiles.insert(key, image);
        if old.is_none() {
            self.tile_bytes = self
                .tile_bytes
                .saturating_add(self.tile_bytes_of(key));
        }
        self.failed.remove(&key);
        Ok(old)
    }

    fn tile_bytes_of(&self, key: TileKey) -> usize {
        let Some(level) = self.level(key.level) else {
            return 0;
        };
        let rect = pyramid::texture_rect(level, key.col, key.row);
        rect.width as usize * rect.height as usize * 4
    }

    /// 超预算时按最久未用淘汰，这一帧计划里用到的块不动。
    pub(crate) fn trim(&mut self) -> Vec<Arc<RenderImage>> {
        if self.tile_bytes <= TILE_BUDGET_BYTES {
            return Vec::new();
        }
        let mut candidates: Vec<TileKey> = self
            .tiles
            .keys()
            .copied()
            .filter(|key| self.used.get(key).copied().unwrap_or(0) == 0)
            .collect();
        // 没被任何一帧标记过的先淘汰；再按最久未用淘汰。
        candidates.sort_by_key(|key| self.used.get(key).copied().unwrap_or(0));
        let mut released = Vec::new();
        for key in candidates {
            if self.tile_bytes <= TILE_BUDGET_BYTES {
                break;
            }
            if let Some(image) = self.tiles.remove(&key) {
                self.tile_bytes = self.tile_bytes.saturating_sub(self.tile_bytes_of(key));
                self.used.remove(&key);
                self.failed.remove(&key);
                released.push(image);
            }
        }
        released
    }

    fn take_tiles(&mut self) -> Vec<Arc<RenderImage>> {
        self.tile_bytes = 0;
        self.used.clear();
        self.failed.clear();
        self.tiles.drain().map(|(_, image)| image).collect()
    }

    fn take_top(&mut self) -> Option<Arc<RenderImage>> {
        self.top.take()
    }
}

/// 后台任务登记：同一张图同一时刻只跑一个批次。
#[derive(Clone, Debug)]
pub(crate) struct JobState {
    pub(crate) revision: SourceRevision,
    pub(crate) top: bool,
    pub(crate) tiles: HashSet<TileKey>,
}

/// 全部图片的金字塔、任务登记和源图缓存。
pub(crate) struct PreviewStore {
    pub(crate) pyramids: HashMap<ItemId, ImagePyramid>,
    pub(crate) jobs: HashMap<ItemId, JobState>,
    pub(crate) source: Arc<Mutex<SourceCache>>,
    released: Vec<Arc<RenderImage>>,
}

impl PreviewStore {
    pub(crate) fn new() -> Self {
        Self {
            pyramids: HashMap::new(),
            jobs: HashMap::new(),
            source: Arc::new(Mutex::new(SourceCache::new(SOURCE_BUDGET_BYTES))),
            released: Vec::new(),
        }
    }

    /// 取这张图的金字塔；源版本变了就整体换新，旧纹理排进释放队列。
    pub(crate) fn ensure(&mut self, item: &Item) -> &mut ImagePyramid {
        let revision = revision_for(item);
        let id = item.id;
        let needs_new = self
            .pyramids
            .get(&id)
            .is_none_or(|pyramid| pyramid.revision != revision);
        if needs_new {
            if let Some(mut old) = self.pyramids.remove(&id) {
                self.released.extend(old.take_tiles());
                if let Some(top) = old.take_top() {
                    self.released.push(top);
                }
            }
            let levels = pyramid::levels(item.width, item.height, TOP_MAX);
            self.pyramids
                .insert(id, ImagePyramid::new(revision, levels));
        }
        self.pyramids.get_mut(&id).expect("刚插入的金字塔必须存在")
    }

    pub(crate) fn get(&self, id: ItemId) -> Option<&ImagePyramid> {
        self.pyramids.get(&id)
    }

    pub(crate) fn top_for(&self, id: ItemId) -> Option<Arc<RenderImage>> {
        self.pyramids
            .get(&id)
            .and_then(|pyramid| pyramid.top.clone())
    }

    pub(crate) fn take_released(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.released)
    }

    /// 把一张图图块缓存里的纹理交出来，顶层保留。
    pub(crate) fn release_tiles(&mut self, id: ItemId) {
        if let Some(pyramid) = self.pyramids.get_mut(&id) {
            self.released.extend(pyramid.take_tiles());
        }
    }

    /// 屏幕外的图只保留顶层；图块等再进入屏幕时按源分辨率重建。
    pub(crate) fn release_offscreen_tiles(&mut self, visible: &[ItemId]) {
        let ids: Vec<ItemId> = self
            .pyramids
            .keys()
            .copied()
            .filter(|id| !visible.contains(id))
            .collect();
        for id in ids {
            self.release_tiles(id);
        }
    }

    pub(crate) fn remove(&mut self, ids: &[ItemId]) {
        for id in ids {
            if let Some(mut pyramid) = self.pyramids.remove(id) {
                self.released.extend(pyramid.take_tiles());
                if let Some(top) = pyramid.take_top() {
                    self.released.push(top);
                }
            }
            self.jobs.remove(id);
        }
    }

    pub(crate) fn clear(&mut self) {
        let ids: Vec<ItemId> = self.pyramids.keys().copied().collect();
        self.remove(&ids);
        self.jobs.clear();
        if let Ok(mut source) = self.source.lock() {
            source.clear();
        }
    }

    pub(crate) fn trim(&mut self) -> Vec<Arc<RenderImage>> {
        let mut released = Vec::new();
        for pyramid in self.pyramids.values_mut() {
            released.extend(pyramid.trim());
        }
        released
    }
}

/// 源图解码结果缓存，按源版本存，带简单的最近使用淘汰。
pub(crate) struct SourceCache {
    entries: HashMap<SourceRevision, SourceEntry>,
    lru: VecDeque<SourceRevision>,
    bytes: usize,
    budget: usize,
}

struct SourceEntry {
    image: Arc<DynamicImage>,
    bytes: usize,
}

impl SourceCache {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            bytes: 0,
            budget,
        }
    }

    pub(crate) fn get(&mut self, revision: SourceRevision) -> Option<Arc<DynamicImage>> {
        let image = self.entries.get(&revision)?.image.clone();
        self.touch(revision);
        Some(image)
    }

    /// 取源图；缓存里没有就解码一次并放进缓存。尺寸和记录不一致时报错。
    pub(crate) fn load_or_decode(
        &mut self,
        revision: SourceRevision,
        source: &crate::model::SourceImage,
        name: &str,
        item_size: (u32, u32),
    ) -> Result<Arc<DynamicImage>, String> {
        if let Some(image) = self.get(revision) {
            return Ok(image);
        }
        let rgb = source
            .load_rgb()
            .map_err(|err| format!("读取图片 {name} 失败：{err}"))?;
        let actual = (rgb.width(), rgb.height());
        if actual != item_size {
            return Err(format!(
                "图片 {name} 的尺寸变了：记录 {}×{}，实际 {}×{}；请重新导入这张图",
                item_size.0, item_size.1, actual.0, actual.1
            ));
        }
        let image = Arc::new(crate::imaging::render::as_dynamic(rgb));
        let bytes = image.width() as usize * image.height() as usize * 3;
        self.insert(revision, image.clone(), bytes);
        Ok(image)
    }

    fn insert(&mut self, revision: SourceRevision, image: Arc<DynamicImage>, bytes: usize) {
        if let Some(old) = self.entries.remove(&revision) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.lru.retain(|key| *key != revision);
        self.entries.insert(revision, SourceEntry { image, bytes });
        self.lru.push_back(revision);
        self.bytes = self.bytes.saturating_add(bytes);
        self.evict();
    }

    fn touch(&mut self, revision: SourceRevision) {
        self.lru.retain(|key| *key != revision);
        self.lru.push_back(revision);
    }

    fn evict(&mut self) {
        while self.bytes > self.budget {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.bytes = 0;
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imaging::render::as_dynamic;
    use image::{Frame, RgbImage, RgbaImage};

    fn fake_image() -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(4, 4))]))
    }

    fn revision() -> SourceRevision {
        SourceRevision {
            item: 1,
            fingerprint: 7,
            width: 2000,
            height: 1000,
        }
    }

    fn item() -> crate::model::Item {
        crate::model::Item {
            id: 1,
            name: "a".into(),
            width: 2000,
            height: 1000,
            source: crate::model::SourceImage::Pixels(Arc::new(RgbImage::new(2, 1))),
            block: None,
            status: crate::model::ItemStatus::Ready,
            keep_whole: false,
            manual: None,
            analysis: crate::model::AnalysisState::default(),
            fingerprint: crate::imaging::decode::ImageFingerprint(7),
        }
    }

    #[test]
    fn levels_start_at_native_and_end_at_top() {
        let levels = pyramid::levels(2000, 1000, TOP_MAX);
        assert_eq!(levels[0].width, 2000);
        assert_eq!(levels.last().unwrap().width, 500);
        assert_eq!(levels.last().unwrap().height, 250);
    }

    #[test]
    fn ensure_reuses_pyramid_for_same_revision() {
        let mut store = PreviewStore::new();
        let mut item = item();
        let levels = store.ensure(&item).levels.len();
        let again = store.ensure(&item).levels.len();
        assert_eq!(levels, again);
        item.fingerprint = crate::imaging::decode::ImageFingerprint(8);
        let changed = store.ensure(&item).revision.fingerprint;
        assert_eq!(changed, 8);
    }

    #[test]
    fn insert_tile_rejects_other_revision() {
        let mut item = item();
        let mut store = PreviewStore::new();
        let pyramid = store.ensure(&item);
        let key = TileKey {
            revision: pyramid.revision,
            level: 1,
            col: 0,
            row: 0,
        };
        assert!(pyramid.insert_tile(key, fake_image()).is_ok());
        let other = TileKey {
            revision: SourceRevision {
                fingerprint: 99,
                ..revision()
            },
            ..key
        };
        assert!(pyramid.insert_tile(other, fake_image()).is_err());
        item.width = 100;
        let rev = store.ensure(&item).revision;
        assert_eq!(rev.width, 100);
    }

    #[test]
    fn source_cache_reuses_decoded_source() {
        let mut cache = SourceCache::new(usize::MAX);
        let source = crate::model::SourceImage::Pixels(Arc::new(RgbImage::new(3, 2)));
        let first = cache
            .load_or_decode(revision(), &source, "a", (3, 2))
            .expect("应能解码");
        let second = cache
            .load_or_decode(revision(), &source, "a", (3, 2))
            .expect("应命中缓存");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn source_cache_evicts_old_entries() {
        let mut cache = SourceCache::new(1);
        let source = crate::model::SourceImage::Pixels(Arc::new(RgbImage::new(3, 2)));
        cache
            .load_or_decode(revision(), &source, "a", (3, 2))
            .expect("应能解码");
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn source_cache_rejects_changed_dimensions() {
        let mut cache = SourceCache::new(usize::MAX);
        let source = crate::model::SourceImage::Pixels(Arc::new(RgbImage::new(3, 2)));
        let err = cache
            .load_or_decode(revision(), &source, "a", (4, 2))
            .expect_err("尺寸不一致应报错");
        assert!(err.contains("尺寸变了"));
    }

    #[test]
    fn as_dynamic_keeps_pixels() {
        let image = as_dynamic(RgbImage::new(2, 2));
        assert_eq!(image.width(), 2);
    }
}
