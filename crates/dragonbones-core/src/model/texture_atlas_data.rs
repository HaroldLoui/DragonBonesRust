//! 对齐 `model/TextureAtlasData.ts`（`TextureAtlasData` + `TextureData`）。
//!
//! core 内图集只有单一实现，抽象类 `TextureAtlasData` / `TextureData` 落地为 struct。

use indexmap::IndexMap;

use crate::geom::rectangle::Rectangle;

/// - 贴图集数据（`TextureAtlasData.ts`）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextureAtlasData {
    pub auto_search: bool,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub name: String,
    pub image_path: String,
    pub textures: IndexMap<String, TextureData>,
}

impl Default for TextureAtlasData {
    fn default() -> Self {
        Self {
            auto_search: false,
            width: 0.0,
            height: 0.0,
            scale: 1.0,
            name: String::new(),
            image_path: String::new(),
            textures: IndexMap::new(),
        }
    }
}

impl TextureAtlasData {
    /// 对齐 `TextureAtlasData.addTexture`（重名忽略）。
    pub fn add_texture(&mut self, mut value: TextureData) {
        if self.textures.contains_key(&value.name) {
            log::warn!("Same texture: {}", value.name);
            return;
        }

        value.parent.clone_from(&self.name);
        self.textures.insert(value.name.clone(), value);
    }

    /// 对齐 `TextureAtlasData.getTexture`。
    #[must_use]
    pub fn get_texture(&self, texture_name: &str) -> Option<&TextureData> {
        self.textures.get(texture_name)
    }

    /// 对齐 `TextureAtlasData.copyFrom`。
    pub fn copy_from(&mut self, value: &TextureAtlasData) {
        self.auto_search = value.auto_search;
        self.scale = value.scale;
        self.width = value.width;
        self.height = value.height;
        self.name.clone_from(&value.name);
        self.image_path.clone_from(&value.image_path);

        self.textures.clear();
        for (k, texture) in &value.textures {
            let mut copy = TextureData::default();
            copy.copy_from(texture);
            self.textures.insert(k.clone(), copy);
        }
    }
}

/// - 贴图数据（`TextureData.ts`）。
///
/// `parent` 为图集名引用（D-1.1）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TextureData {
    pub rotated: bool,
    pub name: String,
    pub region: Rectangle,
    pub parent: String,
    pub frame: Option<Rectangle>,
}

impl TextureData {
    /// 对齐 `TextureData.copyFrom`。
    pub fn copy_from(&mut self, value: &TextureData) {
        self.rotated = value.rotated;
        self.name.clone_from(&value.name);
        self.region.copy_from(value.region);
        self.parent.clone_from(&value.parent);

        match (self.frame, &value.frame) {
            (None, Some(_)) => self.frame = Some(Rectangle::default()),
            (Some(_), None) => self.frame = None,
            _ => {}
        }

        if let (Some(frame), Some(other)) = (&mut self.frame, &value.frame) {
            frame.copy_from(*other);
        }
    }
}
