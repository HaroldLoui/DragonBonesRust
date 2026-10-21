//! `parser/BinaryDataParser.ts` 的二进制容器 + 头部解码。
//!
//! 只负责把 dbbin 拆成“header JSON + 7 组 TypedArray”，实际模型/IR 解析复用
//! [`super::object_data_parser::ObjectDataParser`]（官方也是 `extends`）：
//! 静态骨架结构走 JSON 路径，animation/geometry 走 BinaryDataParser 的覆写分支。

use serde_json::Value;

/// 对齐 dbbin 的 `DBDT` 格式。
///
/// 布局：4 字节 tag + `u32` headerLength + UTF-8 header JSON + 负载；
/// 负载起点 `12 + headerLength`，根 `offset` 中的偏移均相对该起点。
#[derive(Debug, Clone)]
pub(crate) struct BinaryArrays {
    pub int_array: Vec<i32>,
    pub float_array: Vec<f64>,
    pub frame_int_array: Vec<i32>,
    pub frame_float_array: Vec<f64>,
    pub frame_array: Vec<i32>,
    pub timeline_array: Vec<i32>,
    pub color_array: Vec<i32>,
    /// 整份输入字节（官方 `DragonBonesData.binary` 在 binary 路径保留 ArrayBuffer）。
    pub payload: Vec<u8>,
    /// dbbin 头部 JSON（结构同普通 skeleton JSON，但 animation 只有 offset/timeline 表）。
    pub header: Value,
}

impl BinaryArrays {
    /// 解析 dbbin 头部并解码全部 TypedArray；任何越界/未对齐/tag 不符返回 `None`。
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 || &bytes[0..4] != b"DBDT" {
            return None;
        }
        let header_len = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
        let header_start = 12usize;
        let payload_start = header_start.checked_add(header_len)?;
        if payload_start > bytes.len() {
            return None;
        }
        let header_text = std::str::from_utf8(&bytes[header_start..payload_start]).ok()?;
        let header: Value = serde_json::from_str(header_text).ok()?;
        let offsets: Vec<usize> = header
            .get("offset")?
            .as_array()?
            .iter()
            .map(|value| value.as_f64().map(|v| v as usize))
            .collect::<Option<Vec<_>>>()?;
        if offsets.len() < 12 {
            return None;
        }
        let payload = &bytes[payload_start..];
        let segment = |index: usize| -> Option<(usize, usize)> {
            Some((*offsets.get(index)?, *offsets.get(index + 1)?))
        };

        let (int_off, int_len) = segment(0)?;
        let (float_off, float_len) = segment(2)?;
        let (frame_int_off, frame_int_len) = segment(4)?;
        let (frame_float_off, frame_float_len) = segment(6)?;
        let (frame_off, frame_len) = segment(8)?;
        let (timeline_off, timeline_len) = segment(10)?;
        let (color_off, color_len) = if offsets.len() >= 14 {
            segment(12)?
        } else {
            (0, 0)
        };

        let int_array = decode_i16(payload, int_off, int_len)?;
        let color_array = if color_len == 0 {
            int_array.clone()
        } else {
            decode_i16(payload, color_off, color_len)?
        };

        Some(Self {
            int_array,
            float_array: decode_f32(payload, float_off, float_len)?,
            frame_int_array: decode_i16(payload, frame_int_off, frame_int_len)?,
            frame_float_array: decode_f32(payload, frame_float_off, frame_float_len)?,
            frame_array: decode_i16(payload, frame_off, frame_len)?,
            timeline_array: decode_u16(payload, timeline_off, timeline_len)?,
            color_array,
            payload: bytes.to_vec(),
            header,
        })
    }
}

fn range(payload: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    let end = offset.checked_add(len)?;
    payload.get(offset..end)
}

fn decode_i16(payload: &[u8], offset: usize, len: usize) -> Option<Vec<i32>> {
    if !len.is_multiple_of(2) {
        return None;
    }
    let data = range(payload, offset, len)?;
    Some(
        data.chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as i32)
            .collect(),
    )
}

fn decode_u16(payload: &[u8], offset: usize, len: usize) -> Option<Vec<i32>> {
    if !len.is_multiple_of(2) {
        return None;
    }
    let data = range(payload, offset, len)?;
    Some(
        data.chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]) as i32)
            .collect(),
    )
}

fn decode_f32(payload: &[u8], offset: usize, len: usize) -> Option<Vec<f64>> {
    if !len.is_multiple_of(4) {
        return None;
    }
    let data = range(payload, offset, len)?;
    Some(
        data.chunks_exact(4)
            .map(|chunk| {
                f64::from(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            })
            .collect(),
    )
}
