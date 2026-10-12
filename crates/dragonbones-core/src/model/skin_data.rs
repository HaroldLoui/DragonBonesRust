//! 对齐 `model/SkinData.ts`。

use indexmap::IndexMap;

use crate::model::display_data::DisplayData;

/// - 皮肤数据（`SkinData.ts`）。
///
/// `parent` 为骨架名引用（D-1.1）。每个插槽对应一个显示对象列表，元素可为 `null`
/// （对应 TS 的 `Array<DisplayData | null>`）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SkinData {
    pub name: String,
    pub displays: IndexMap<String, Vec<Option<DisplayData>>>,
    pub parent: String,
}

impl SkinData {
    /// 对齐 `SkinData.addDisplay`。
    pub fn add_display(&mut self, slot_name: impl Into<String>, mut value: Option<DisplayData>) {
        let slot_name = slot_name.into();

        if let Some(display) = &mut value {
            display.parent.clone_from(&self.name);
        }

        self.displays.entry(slot_name).or_default().push(value);
    }

    /// 对齐 `SkinData.getDisplay`（按 `display.name` 查找）。
    #[must_use]
    pub fn get_display(&self, slot_name: &str, display_name: &str) -> Option<&DisplayData> {
        self.get_displays(slot_name)?
            .iter()
            .flatten()
            .find(|display| display.name == display_name)
    }

    /// 对齐 `SkinData.getDisplays`。
    #[must_use]
    pub fn get_displays(&self, slot_name: &str) -> Option<&[Option<DisplayData>]> {
        self.displays.get(slot_name).map(Vec::as_slice)
    }
}
