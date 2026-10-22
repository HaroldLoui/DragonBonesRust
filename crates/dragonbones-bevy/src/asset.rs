use std::path::Path;
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{Asset, AssetLoader, AssetPath, Handle, LoadContext};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use dragonbones_core::model::{DragonBonesData, TextureAtlasData};
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

/// Parsed `_ske.json` skeleton data, ready to be registered with the runtime.
#[derive(Asset, TypePath, Clone)]
pub struct SkeletonAsset {
    pub data: Arc<DragonBonesData>,
}

/// Parsed `_tex.json` atlas plus the resolved page image.
#[derive(Asset, TypePath, Clone)]
pub struct AtlasAsset {
    pub data: TextureAtlasData,
    #[dependency]
    pub texture: Handle<Image>,
}

/// Errors produced by the adapter's asset loaders.
#[derive(Debug, thiserror::Error)]
pub enum DragonBonesAssetError {
    #[error("failed to read asset: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("JSON is not valid DragonBones skeleton data")]
    InvalidSkeleton,
    #[error("JSON is not valid DragonBones texture atlas data")]
    InvalidAtlas,
    #[error("texture atlas is missing its imagePath")]
    MissingImagePath,
}

/// Loader for `_ske.json`.
#[derive(Default, TypePath)]
pub struct SkeletonAssetLoader;

impl AssetLoader for SkeletonAssetLoader {
    type Asset = SkeletonAsset;
    type Settings = ();
    type Error = DragonBonesAssetError;

    fn extensions(&self) -> &[&str] {
        &["json"]
    }

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let value: Value = serde_json::from_slice(&bytes)?;
        let mut parser = ObjectDataParser::default();
        let data = parser
            .parse_dragon_bones_data(&value, 1.0)
            .ok_or(DragonBonesAssetError::InvalidSkeleton)?;
        Ok(SkeletonAsset {
            data: Arc::new(data),
        })
    }
}

/// Loader for `_tex.json`. Resolves the page image relative to the atlas file.
#[derive(Default, TypePath)]
pub struct AtlasAssetLoader;

impl AssetLoader for AtlasAssetLoader {
    type Asset = AtlasAsset;
    type Settings = ();
    type Error = DragonBonesAssetError;

    fn extensions(&self) -> &[&str] {
        &["json"]
    }

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let value: Value = serde_json::from_slice(&bytes)?;
        let image_path = value
            .get("imagePath")
            .and_then(Value::as_str)
            .ok_or(DragonBonesAssetError::MissingImagePath)?;

        let mut data = TextureAtlasData::default();
        let mut parser = ObjectDataParser::default();
        if !parser.parse_texture_atlas_data(&value, &mut data, 1.0) {
            return Err(DragonBonesAssetError::InvalidAtlas);
        }

        let base = load_context
            .path()
            .path()
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let page = base.join(image_path);
        let texture = load_context.load::<Image>(AssetPath::from_path_buf(page));
        Ok(AtlasAsset { data, texture })
    }
}
