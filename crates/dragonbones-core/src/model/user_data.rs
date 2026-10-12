//! 对齐 `model/UserData.ts`（`UserData` + `ActionData`）。

use crate::core::enums::ActionType;

/// - 用户自定义数据（`UserData.ts`）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct UserData {
    pub ints: Vec<i32>,
    pub floats: Vec<f64>,
    pub strings: Vec<String>,
}

impl UserData {
    /// 对齐 `UserData.addInt`。
    pub fn add_int(&mut self, value: i32) {
        self.ints.push(value);
    }

    /// 对齐 `UserData.addFloat`。
    pub fn add_float(&mut self, value: f64) {
        self.floats.push(value);
    }

    /// 对齐 `UserData.addString`。
    pub fn add_string(&mut self, value: impl Into<String>) {
        self.strings.push(value.into());
    }

    /// 对齐 `UserData.getInt`（越界返回 0）。
    #[must_use]
    pub fn get_int(&self, index: usize) -> i32 {
        self.ints.get(index).copied().unwrap_or(0)
    }

    /// 对齐 `UserData.getFloat`（越界返回 0.0）。
    #[must_use]
    pub fn get_float(&self, index: usize) -> f64 {
        self.floats.get(index).copied().unwrap_or(0.0)
    }

    /// 对齐 `UserData.getString`（越界返回空串）。
    #[must_use]
    pub fn get_string(&self, index: usize) -> &str {
        self.strings.get(index).map_or("", String::as_str)
    }
}

/// - 动作数据（`ActionData.ts`）。
///
/// `bone` / `slot` 为名字引用（D-1.1），`data` 直接拥有。
#[derive(Debug, Clone, PartialEq)]
pub struct ActionData {
    pub action_type: ActionType,
    /// Frame 事件名 | Sound 事件名 | 动画名。
    pub name: String,
    pub bone: Option<String>,
    pub slot: Option<String>,
    pub data: Option<Box<UserData>>,
}

impl Default for ActionData {
    fn default() -> Self {
        Self {
            action_type: ActionType::Play,
            name: String::new(),
            bone: None,
            slot: None,
            data: None,
        }
    }
}
