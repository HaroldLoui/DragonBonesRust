//! 阶段 6：per-Armature 缓存帧（P1-4）。
//!
//! 官方把缓存放在共享 data 上；Rust data 不可变（D-3.3），因此每 Armature
//! 持有一份等价缓存（D-6.1）：
//! - `frames`：扁平 f64 数组，每槽位 10 个值（矩阵 a,b,c,d,tx,ty + rotation,skew,scaleX,scaleY）；
//! - `animations`：每个 AnimationData 的 `cachedFrames: bool[]` 与 bone/slot 索引表（初值 -1）。

use std::collections::HashMap;

use crate::geom::Matrix;
use crate::geom::transform::Transform;
use crate::model::ArmatureData;

#[derive(Debug, Default, Clone)]
pub struct AnimationFrameCache {
    pub cached_frames: Vec<bool>,
    pub bone_indices: HashMap<String, Vec<i32>>,
    pub slot_indices: HashMap<String, Vec<i32>>,
    pub rate: f64,
}

#[derive(Debug, Default, Clone)]
pub struct FrameCache {
    pub frames: Vec<f64>,
    pub animations: HashMap<String, AnimationFrameCache>,
}

impl FrameCache {
    /// 对齐 `ArmatureData.cacheFrames` / `AnimationData.cacheFrames`。
    ///
    /// 官方 `if (cacheFrameRate > 0) return`（TODO clear）；这里同样只初始化一次。
    pub fn initialize(&mut self, armature_data: &ArmatureData, rate: f64) {
        if rate <= 0.0 || !self.animations.is_empty() {
            return;
        }
        for (name, animation) in &armature_data.animations {
            let cache_rate = (rate * animation.scale).ceil().max(1.0);
            let count = (cache_rate * animation.duration).ceil() as usize + 1;
            let mut cache = AnimationFrameCache {
                cached_frames: vec![false; count],
                rate: cache_rate,
                ..Default::default()
            };
            for bone in &armature_data.sorted_bones {
                cache.bone_indices.insert(bone.clone(), vec![-1; count]);
            }
            for slot in &armature_data.sorted_slots {
                cache.slot_indices.insert(slot.clone(), vec![-1; count]);
            }
            self.animations.insert(name.clone(), cache);
        }
    }

    #[must_use]
    pub fn animation(&self, name: &str) -> Option<&AnimationFrameCache> {
        self.animations.get(name)
    }

    pub fn animation_mut(&mut self, name: &str) -> Option<&mut AnimationFrameCache> {
        self.animations.get_mut(name)
    }

    /// 对齐 `ArmatureData.setCacheFrame`。
    pub fn set_frame(&mut self, matrix: &Matrix, transform: &Transform) -> i32 {
        let offset = self.frames.len() as i32;
        self.frames.extend_from_slice(&[
            matrix.a,
            matrix.b,
            matrix.c,
            matrix.d,
            matrix.tx,
            matrix.ty,
            transform.rotation,
            transform.skew,
            transform.scale_x,
            transform.scale_y,
        ]);
        offset
    }

    /// 对齐 `ArmatureData.getCacheFrame`。
    pub fn get_frame(&self, matrix: &mut Matrix, transform: &mut Transform, offset: i32) {
        let offset = offset as usize;
        let Some(values) = self.frames.get(offset..offset + 10) else {
            return;
        };
        matrix.a = values[0];
        matrix.b = values[1];
        matrix.c = values[2];
        matrix.d = values[3];
        matrix.tx = values[4];
        matrix.ty = values[5];
        transform.rotation = values[6];
        transform.skew = values[7];
        transform.scale_x = values[8];
        transform.scale_y = values[9];
        transform.x = matrix.tx;
        transform.y = matrix.ty;
    }
}
