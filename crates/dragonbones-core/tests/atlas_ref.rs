//! Multi-atlas registration and per-slot atlas identity.

use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::model::{TextureAtlasData, TextureAtlasRef};
use dragonbones_core::parser::ObjectDataParser;
use serde_json::json;

fn skeleton() -> serde_json::Value {
    json!({
        "frameRate": 24,
        "name": "AtlasData",
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [{
            "type": "Armature",
            "frameRate": 24,
            "name": "A",
            "aabb": { "x": 0, "y": 0, "width": 16, "height": 16 },
            "bone": [{ "name": "root" }],
            "slot": [{ "name": "s", "parent": "root" }],
            "skin": [{
                "name": "",
                "slot": [{
                    "name": "s",
                    "display": [{ "type": "image", "name": "tex", "path": "tex" }]
                }]
            }]
        }]
    })
}

fn atlas(name: &str, textures: serde_json::Value) -> TextureAtlasData {
    let raw = json!({
        "name": name,
        "imagePath": format!("{name}.png"),
        "width": 64,
        "height": 64,
        "SubTexture": textures
    });
    let mut data = TextureAtlasData::default();
    assert!(
        ObjectDataParser::default().parse_texture_atlas_data(&raw, &mut data, 2.0),
        "atlas parses"
    );
    data
}

#[test]
fn texture_resolves_to_the_second_page_and_records_its_identity() {
    let mut factory = BaseFactory::new();
    let data = ObjectDataParser::default()
        .parse_dragon_bones_data(&skeleton(), 1.0)
        .expect("skeleton parses");
    factory.add_dragon_bones_data(data, Some("data"));
    factory.add_texture_atlas_data(atlas("page0", json!([])), Some("pages"));
    factory.add_texture_atlas_data(
        atlas(
            "page1",
            json!([{ "name": "tex", "x": 0, "y": 0, "width": 16, "height": 16 }]),
        ),
        Some("pages"),
    );

    let idx = factory
        .build_armature("A", "data", "", "pages", Box::new(NullArmatureProxy::new()))
        .expect("build");
    factory.advance_one(idx, 0.0);

    let slot = &factory.armature(idx).slots[0];
    assert_eq!(
        slot.texture_atlas_ref(),
        Some(&TextureAtlasRef {
            name: "pages".to_owned(),
            index: 1,
        }),
        "the texture lives on the second page"
    );
    assert_eq!(
        slot.texture_data.as_ref().map(|data| data.name.as_str()),
        Some("tex")
    );
    assert_eq!(
        slot.display_frames[0].texture_atlas_scale,
        Some(2.0),
        "page scale is recorded with the page identity"
    );
}

#[test]
fn atlas_data_management_round_trips() {
    let mut factory = BaseFactory::new();
    factory.add_texture_atlas_data(atlas("old", json!([])), Some("pages"));
    assert!(factory.get_texture_atlas_data("pages").is_some());
    assert!(factory.get_all_texture_atlas_data().contains_key("pages"));

    factory.update_texture_atlases(
        &[atlas(
            "new",
            json!([{ "name": "tex", "x": 0, "y": 0, "width": 8, "height": 8 }]),
        )],
        "pages",
    );
    let updated = factory.get_texture_atlas_data("pages").expect("atlas");
    assert_eq!(updated[0].name, "new");
    assert!(updated[0].get_texture("tex").is_some());

    assert!(factory.remove_texture_atlas_data("pages").is_some());
    assert!(factory.get_texture_atlas_data("pages").is_none());
    factory.clear_data();
    assert!(factory.get_all_dragon_bones_data().is_empty());
    assert!(factory.get_all_texture_atlas_data().is_empty());
}
