use crate::{
    AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds, DevicePixels, PlatformAtlas,
    Point, Size, TileId, platform::AtlasTextureList,
};
use anyhow::Result;
use blade_graphics as gpu;
use blade_util::{BufferBelt, BufferBeltDescriptor};
use collections::FxHashMap;
use etagere::BucketedAtlasAllocator;
use parking_lot::Mutex;
use std::{borrow::Cow, ops, sync::Arc};

pub(crate) struct BladeAtlas(Mutex<BladeAtlasState>);

struct PendingUpload {
    id: AtlasTextureId,
    bounds: Bounds<DevicePixels>,
    data: gpu::BufferPiece,
}

struct BladeAtlasState {
    gpu: Arc<gpu::Context>,
    upload_belt: BufferBelt,
    storage: BladeAtlasStorage,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    initializations: Vec<AtlasTextureId>,
    uploads: Vec<PendingUpload>,
    /// 已经从 tiles_by_key 摘掉、但还没还给分配器的图块。
    pending_frees: Vec<(AtlasTextureId, TileId)>,
    /// 最近一次提交的那一帧的同步点。回收图块要等它画完。
    last_frame_sync: Option<gpu::SyncPoint>,
}

#[cfg(gles)]
unsafe impl Send for BladeAtlasState {}

impl BladeAtlasState {
    fn destroy(&mut self) {
        // 待回收的图块不用单独处理：它们的纹理随 storage 一起销毁。
        self.pending_frees.clear();
        self.storage.destroy(&self.gpu);
        self.upload_belt.destroy(&self.gpu);
    }
}

pub struct BladeTextureInfo {
    pub raw_view: gpu::TextureView,
}

impl BladeAtlas {
    pub(crate) fn new(gpu: &Arc<gpu::Context>) -> Self {
        BladeAtlas(Mutex::new(BladeAtlasState {
            gpu: Arc::clone(gpu),
            upload_belt: BufferBelt::new(BufferBeltDescriptor {
                memory: gpu::Memory::Upload,
                min_chunk_size: 0x10000,
                alignment: 64, // Vulkan `optimalBufferCopyOffsetAlignment` on Intel XE
            }),
            storage: BladeAtlasStorage::default(),
            tiles_by_key: Default::default(),
            initializations: Vec::new(),
            uploads: Vec::new(),
            pending_frees: Vec::new(),
            last_frame_sync: None,
        }))
    }

    pub(crate) fn destroy(&self) {
        self.0.lock().destroy();
    }

    pub fn before_frame(&self, gpu_encoder: &mut gpu::CommandEncoder) {
        let mut lock = self.0.lock();
        lock.reclaim_freed_tiles();
        lock.flush(gpu_encoder);
    }

    pub fn after_frame(&self, sync_point: &gpu::SyncPoint) {
        let mut lock = self.0.lock();
        lock.upload_belt.flush(sync_point);
        lock.last_frame_sync = Some(sync_point.clone());
    }

    pub fn get_texture_info(&self, id: AtlasTextureId) -> BladeTextureInfo {
        let lock = self.0.lock();
        let texture = &lock.storage[id];
        BladeTextureInfo {
            raw_view: texture.raw_view,
        }
    }
}

impl PlatformAtlas for BladeAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        let mut lock = self.0.lock();
        if let Some(tile) = lock.tiles_by_key.get(key) {
            Ok(Some(tile.clone()))
        } else {
            profiling::scope!("new tile");
            let Some((size, bytes)) = build()? else {
                return Ok(None);
            };
            let tile = lock.allocate(size, key.texture_kind());
            lock.upload_texture(tile.texture_id, tile.bounds, &bytes);
            lock.tiles_by_key.insert(key.clone(), tile.clone());
            Ok(Some(tile))
        }
    }

    fn remove(&self, key: &AtlasKey) {
        let mut lock = self.0.lock();

        let Some(tile) = lock.tiles_by_key.remove(key) else {
            return;
        };
        let id = tile.texture_id;

        let Some(texture_slot) = lock.storage[id.kind].textures.get_mut(id.index as usize) else {
            return;
        };
        let Some(texture) = texture_slot.as_mut() else {
            return;
        };
        texture.decrement_ref_count();

        // 这块矩形不能立刻还回去：上一帧可能还在采它。记下来，等确认那一帧画完
        // 再交给分配器，否则新图块会盖在还在用的数据上，画面出现错位色块。
        lock.pending_frees.push((id, tile.tile_id));
    }
}

impl BladeAtlasState {
    /// 把已经确认安全释放的图块还给分配器，顺手销毁空出来的纹理。
    ///
    /// 图块删掉时只记账（见 `BladeAtlas::remove`），真正的归还挪到这里：先问一次
    /// 上一帧提交完了没有，没有就留到下一帧再试。急着还的话，新分到的图块会写进
    /// 上一帧还在采样的区域。
    fn reclaim_freed_tiles(&mut self) {
        if self.pending_frees.is_empty() {
            return;
        }
        if let Some(sync) = self.last_frame_sync.take() {
            // 超时传 0 就是只查一次，不阻塞界面。
            if !self.gpu.wait_for(&sync, 0) {
                self.last_frame_sync = Some(sync);
                return;
            }
        }

        for (id, tile) in std::mem::take(&mut self.pending_frees) {
            if let Some(Some(texture)) = self.storage[id.kind].textures.get_mut(id.index as usize) {
                texture.allocator.deallocate(tile.into());
            }
        }

        self.destroy_unreferenced_textures(AtlasTextureKind::Monochrome);
        self.destroy_unreferenced_textures(AtlasTextureKind::Polychrome);
    }

    /// 引用计数归零的纹理连同它占的显存一起释放，位置留给后面的新图集。
    ///
    /// 只在 `reclaim_freed_tiles` 里调用：那时所有待释放的图块都已经还给分配器，
    /// 引用计数归零的纹理不可能再被分到新图块。
    fn destroy_unreferenced_textures(&mut self, kind: AtlasTextureKind) {
        let empty: Vec<usize> = self.storage[kind]
            .textures
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                slot.as_ref()
                    .is_some_and(|texture| texture.is_unreferenced())
            })
            .map(|(index, _)| index)
            .collect();
        if empty.is_empty() {
            return;
        }
        let gpu = Arc::clone(&self.gpu);
        let list = &mut self.storage[kind];
        for index in empty {
            if let Some(mut texture) = list.textures[index].take() {
                texture.destroy(&gpu);
            }
            list.free_list.push(index);
        }
    }

    fn allocate(&mut self, size: Size<DevicePixels>, texture_kind: AtlasTextureKind) -> AtlasTile {
        {
            let textures = &mut self.storage[texture_kind];

            if let Some(tile) = textures
                .iter_mut()
                .rev()
                .find_map(|texture| texture.allocate(size))
            {
                return tile;
            }
        }

        let texture = self.push_texture(size, texture_kind);
        texture.allocate(size).unwrap()
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
    ) -> &mut BladeAtlasTexture {
        const DEFAULT_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(1024),
            height: DevicePixels(1024),
        };

        let size = min_size.max(&DEFAULT_ATLAS_SIZE);
        let format;
        let usage;
        match kind {
            AtlasTextureKind::Monochrome => {
                format = gpu::TextureFormat::R8Unorm;
                usage = gpu::TextureUsage::COPY | gpu::TextureUsage::RESOURCE;
            }
            AtlasTextureKind::Polychrome => {
                format = gpu::TextureFormat::Bgra8Unorm;
                usage = gpu::TextureUsage::COPY | gpu::TextureUsage::RESOURCE;
            }
        }

        let raw = self.gpu.create_texture(gpu::TextureDesc {
            name: "atlas",
            format,
            size: gpu::Extent {
                width: size.width.into(),
                height: size.height.into(),
                depth: 1,
            },
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: 1,
            dimension: gpu::TextureDimension::D2,
            usage,
            external: None,
        });
        let raw_view = self.gpu.create_texture_view(
            raw,
            gpu::TextureViewDesc {
                name: "",
                format,
                dimension: gpu::ViewDimension::D2,
                subresources: &Default::default(),
            },
        );

        let texture_list = &mut self.storage[kind];
        let index = texture_list.free_list.pop();

        let atlas_texture = BladeAtlasTexture {
            id: AtlasTextureId {
                index: index.unwrap_or(texture_list.textures.len()) as u32,
                kind,
            },
            allocator: etagere::BucketedAtlasAllocator::new(size.into()),
            format,
            raw,
            raw_view,
            live_atlas_keys: 0,
        };

        self.initializations.push(atlas_texture.id);

        if let Some(ix) = index {
            texture_list.textures[ix] = Some(atlas_texture);
            texture_list.textures.get_mut(ix).unwrap().as_mut().unwrap()
        } else {
            texture_list.textures.push(Some(atlas_texture));
            texture_list.textures.last_mut().unwrap().as_mut().unwrap()
        }
    }

    fn upload_texture(&mut self, id: AtlasTextureId, bounds: Bounds<DevicePixels>, bytes: &[u8]) {
        let data = self.upload_belt.alloc_bytes(bytes, &self.gpu);
        self.uploads.push(PendingUpload { id, bounds, data });
    }

    fn flush_initializations(&mut self, encoder: &mut gpu::CommandEncoder) {
        for id in self.initializations.drain(..) {
            // 纹理可能在同一帧里刚建好就被回收掉（图块建完立刻被丢弃）。
            let Some(Some(texture)) = self.storage[id.kind]
                .textures
                .get(id.index as usize)
            else {
                continue;
            };
            encoder.init_texture(texture.raw);
        }
    }

    fn flush(&mut self, encoder: &mut gpu::CommandEncoder) {
        self.flush_initializations(encoder);

        let mut transfers = encoder.transfer("atlas");
        for upload in self.uploads.drain(..) {
            let Some(Some(texture)) = self.storage[upload.id.kind]
                .textures
                .get(upload.id.index as usize)
            else {
                continue;
            };
            transfers.copy_buffer_to_texture(
                upload.data,
                upload.bounds.size.width.to_bytes(texture.bytes_per_pixel()),
                gpu::TexturePiece {
                    texture: texture.raw,
                    mip_level: 0,
                    array_layer: 0,
                    origin: [
                        upload.bounds.origin.x.into(),
                        upload.bounds.origin.y.into(),
                        0,
                    ],
                },
                gpu::Extent {
                    width: upload.bounds.size.width.into(),
                    height: upload.bounds.size.height.into(),
                    depth: 1,
                },
            );
        }
    }
}

#[derive(Default)]
struct BladeAtlasStorage {
    monochrome_textures: AtlasTextureList<BladeAtlasTexture>,
    polychrome_textures: AtlasTextureList<BladeAtlasTexture>,
}

impl ops::Index<AtlasTextureKind> for BladeAtlasStorage {
    type Output = AtlasTextureList<BladeAtlasTexture>;
    fn index(&self, kind: AtlasTextureKind) -> &Self::Output {
        match kind {
            crate::AtlasTextureKind::Monochrome => &self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &self.polychrome_textures,
        }
    }
}

impl ops::IndexMut<AtlasTextureKind> for BladeAtlasStorage {
    fn index_mut(&mut self, kind: AtlasTextureKind) -> &mut Self::Output {
        match kind {
            crate::AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        }
    }
}

impl ops::Index<AtlasTextureId> for BladeAtlasStorage {
    type Output = BladeAtlasTexture;
    fn index(&self, id: AtlasTextureId) -> &Self::Output {
        let textures = match id.kind {
            crate::AtlasTextureKind::Monochrome => &self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &self.polychrome_textures,
        };
        textures[id.index as usize].as_ref().unwrap()
    }
}

impl BladeAtlasStorage {
    fn destroy(&mut self, gpu: &gpu::Context) {
        for mut texture in self.monochrome_textures.drain().flatten() {
            texture.destroy(gpu);
        }
        for mut texture in self.polychrome_textures.drain().flatten() {
            texture.destroy(gpu);
        }
    }
}

struct BladeAtlasTexture {
    id: AtlasTextureId,
    allocator: BucketedAtlasAllocator,
    raw: gpu::Texture,
    raw_view: gpu::TextureView,
    format: gpu::TextureFormat,
    live_atlas_keys: u32,
}

impl BladeAtlasTexture {
    fn allocate(&mut self, size: Size<DevicePixels>) -> Option<AtlasTile> {
        let allocation = self.allocator.allocate(size.into())?;
        let tile = AtlasTile {
            texture_id: self.id,
            tile_id: allocation.id.into(),
            padding: 0,
            bounds: Bounds {
                origin: allocation.rectangle.min.into(),
                size,
            },
        };
        self.live_atlas_keys += 1;
        Some(tile)
    }

    fn destroy(&mut self, gpu: &gpu::Context) {
        gpu.destroy_texture(self.raw);
        gpu.destroy_texture_view(self.raw_view);
    }

    fn bytes_per_pixel(&self) -> u8 {
        self.format.block_info().size
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys -= 1;
    }

    fn is_unreferenced(&self) -> bool {
        self.live_atlas_keys == 0
    }
}

impl From<Size<DevicePixels>> for etagere::Size {
    fn from(size: Size<DevicePixels>) -> Self {
        etagere::Size::new(size.width.into(), size.height.into())
    }
}

impl From<etagere::Point> for Point<DevicePixels> {
    fn from(value: etagere::Point) -> Self {
        Point {
            x: DevicePixels::from(value.x),
            y: DevicePixels::from(value.y),
        }
    }
}

impl From<etagere::Size> for Size<DevicePixels> {
    fn from(size: etagere::Size) -> Self {
        Size {
            width: DevicePixels::from(size.width),
            height: DevicePixels::from(size.height),
        }
    }
}

impl From<etagere::Rectangle> for Bounds<DevicePixels> {
    fn from(rectangle: etagere::Rectangle) -> Self {
        Bounds {
            origin: rectangle.min.into(),
            size: rectangle.size().into(),
        }
    }
}
