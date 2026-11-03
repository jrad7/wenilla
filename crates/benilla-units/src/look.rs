//! What a display looks like, as plain data, so the rules are testable with no GPU and no client.

use benilla_formats::{
    CharSkinSlot, CreatureCatalog, EquipGeosets, ItemDisplay, ItemDisplayCatalog, NpcAppearance,
    forearm_dressed,
};

/// One creature display, resolved.
#[derive(Clone, Debug)]
pub struct CreatureLook {
    pub model_path: String,
    /// `modelScale × creatureModelScale`. The client's unit takes its scale from the server, which
    /// folded this in already; with no server this is the size.
    pub scale: f32,
    /// The display's `Monster1/2/3` skins, bare names beside the model.
    pub skins: [Option<String>; 3],
    /// A humanoid's body, from `CreatureDisplayInfoExtra`.
    pub npc: Option<NpcAppearance>,
}

/// `None` for an unknown display or a zero-scale row, both of which the client draws as nothing.
pub fn creature_look(catalog: &CreatureCatalog, display_id: u32) -> Option<CreatureLook> {
    let model = catalog.model(display_id)?;
    (model.scale > 0.0).then_some(CreatureLook {
        model_path: model.model_path,
        scale: model.scale,
        skins: model.textures,
        npc: model.npc_appearance,
    })
}

/// `Creature\Kobold\Kobold.mdx` to `Creature\Kobold`, where the skins live.
pub fn model_dir(model_path: &str) -> &str {
    model_path
        .rsplit_once(['\\', '/'])
        .map_or("", |(dir, _)| dir)
}

/// A `Character\` model, whose body the customization pipeline dresses. It follows the file, not
/// the display's kind.
pub fn is_character_model(model_path: &str) -> bool {
    model_path
        .get(..10)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("character\\"))
}

/// Which texture one batch of a model wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartSkin<'a> {
    /// What the model file named, or none at all.
    Embedded,
    /// A creature skin slot: this file beside the model.
    Creature(&'a str),
    /// A humanoid's body: the baked atlas of this name.
    Baked(&'a str),
    /// A humanoid's hair (`CharSkinSlot::Hair`).
    Hair,
    /// A furred race's extra skin (`CharSkinSlot::SkinExtra`).
    SkinExtra,
}

/// A batch's texture. A humanoid's body slots read its appearance, and a plain creature's read its
/// skins. A body with no bake name draws its fallback, since composing one from `CharSections` is
/// not here yet.
pub fn part_skin(
    look: &CreatureLook,
    char_slot: Option<CharSkinSlot>,
    skin_slot: Option<u8>,
    embedded: bool,
) -> PartSkin<'_> {
    if let Some(npc) = look
        .npc
        .as_ref()
        .filter(|_| is_character_model(&look.model_path))
    {
        match char_slot {
            Some(CharSkinSlot::Body) => {
                return npc
                    .bake_name
                    .as_deref()
                    .map_or(PartSkin::Embedded, PartSkin::Baked);
            }
            Some(CharSkinSlot::Hair) => return PartSkin::Hair,
            Some(CharSkinSlot::SkinExtra) => return PartSkin::SkinExtra,
            Some(CharSkinSlot::Object) | None => {}
        }
    }
    if embedded {
        return PartSkin::Embedded;
    }
    skin_slot
        .and_then(|slot| look.skins.get(usize::from(slot)))
        .and_then(|name| name.as_deref())
        .map_or(PartSkin::Embedded, PartSkin::Creature)
}

/// A humanoid NPC's worn geoset inputs: its extra row's columns are `ItemDisplayInfo` ids, 2 to 9
/// the armor and 0 the helm. It has no cloak column.
pub(crate) fn npc_geosets(npc: &NpcAppearance, items: Option<&ItemDisplayCatalog>) -> EquipGeosets {
    let mut geosets = EquipGeosets::default();
    let Some(items) = items else {
        return geosets;
    };
    let mut worn: [Option<&ItemDisplay>; 8] = [None; 8];
    for (slot, id) in npc.equipment[2..].iter().enumerate() {
        if *id != 0 {
            worn[slot] = items.get(*id);
            geosets.bodyslots[slot] = worn[slot].map(|row| row.geoset_groups);
        }
    }
    geosets.forearm_dressed = forearm_dressed(&worn);
    if npc.equipment[0] != 0 {
        // A row naming no head model can still carry a hide mask, which must not bald the wearer.
        geosets.helm_vis = items
            .get(npc.equipment[0])
            .and_then(ItemDisplay::worn_helm_vis);
    }
    geosets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appearance(bake_name: Option<&str>) -> NpcAppearance {
        NpcAppearance {
            race: 1,
            sex: 0,
            skin: 0,
            face: 0,
            hair_style: 0,
            hair_color: 0,
            facial_hair: 0,
            equipment: [0; 10],
            bake_name: bake_name.map(str::to_owned),
        }
    }

    fn look(model_path: &str, npc: Option<NpcAppearance>) -> CreatureLook {
        CreatureLook {
            model_path: model_path.to_owned(),
            scale: 1.0,
            skins: [Some("Skin0".to_owned()), None, Some("Skin2".to_owned())],
            npc,
        }
    }

    #[test]
    fn a_creature_skin_slot_wears_its_named_skin() {
        let beast = look("Creature\\Bat\\Bat.mdx", None);
        assert_eq!(
            part_skin(&beast, None, Some(0), false),
            PartSkin::Creature("Skin0")
        );
        assert_eq!(
            part_skin(&beast, None, Some(2), false),
            PartSkin::Creature("Skin2")
        );
    }

    #[test]
    fn an_empty_or_absent_skin_slot_falls_back_and_a_named_texture_wins() {
        let beast = look("Creature\\Bat\\Bat.mdx", None);
        assert_eq!(part_skin(&beast, None, Some(1), false), PartSkin::Embedded);
        assert_eq!(part_skin(&beast, None, Some(9), false), PartSkin::Embedded);
        assert_eq!(part_skin(&beast, None, Some(0), true), PartSkin::Embedded);
        assert_eq!(part_skin(&beast, None, None, false), PartSkin::Embedded);
    }

    #[test]
    fn a_humanoids_body_wears_its_bake_and_its_hair_its_tables() {
        let npc = look(
            "Character\\Human\\Male\\HumanMale.mdx",
            Some(appearance(Some("DefiasBake"))),
        );
        assert_eq!(
            part_skin(&npc, Some(CharSkinSlot::Body), None, false),
            PartSkin::Baked("DefiasBake")
        );
        assert_eq!(
            part_skin(&npc, Some(CharSkinSlot::Hair), None, false),
            PartSkin::Hair
        );
        assert_eq!(
            part_skin(&npc, Some(CharSkinSlot::SkinExtra), None, false),
            PartSkin::SkinExtra
        );
    }

    #[test]
    fn a_humanoid_with_no_bake_draws_its_fallback_body() {
        let npc = look(
            "Character\\Human\\Male\\HumanMale.mdx",
            Some(appearance(None)),
        );
        assert_eq!(
            part_skin(&npc, Some(CharSkinSlot::Body), None, false),
            PartSkin::Embedded
        );
    }

    // The customization follows the model file: an extra row on a creature model changes nothing.
    #[test]
    fn an_extra_row_on_a_creature_model_keeps_its_creature_skins() {
        let odd = look("Creature\\Bat\\Bat.mdx", Some(appearance(Some("Bake"))));
        assert_eq!(
            part_skin(&odd, Some(CharSkinSlot::Body), Some(0), false),
            PartSkin::Creature("Skin0")
        );
    }

    #[test]
    fn model_paths_split_as_the_client_writes_them() {
        assert_eq!(
            model_dir("Creature\\Kobold\\Kobold.mdx"),
            "Creature\\Kobold"
        );
        assert_eq!(model_dir("Bare.mdx"), "");
        assert!(is_character_model("CHARACTER\\Human\\Male\\HumanMale.mdx"));
        assert!(!is_character_model("Creature\\Kobold\\Kobold.mdx"));
    }

    #[test]
    fn with_no_item_table_a_humanoid_is_naked() {
        let mut npc = appearance(None);
        npc.equipment = [7; 10];
        let geosets = npc_geosets(&npc, None);
        assert!(geosets.bodyslots.iter().all(Option::is_none));
        assert!(geosets.helm_vis.is_none());
    }

    // Against the client's own tables, skipped without one: a spawn's displays name real files.
    #[test]
    fn these_displays_resolve_in_a_real_client() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let catalog = benilla_formats::load_creature_catalog(&mut chain).expect("creatures");
        for id in [2316, 2318, 184, 2029, 1084, 4214, 4312, 755] {
            let found = creature_look(&catalog, id).expect("display");
            println!(
                "{id}: {} scale {} skins {:?} bake {:?} equipment {:?}",
                found.model_path,
                found.scale,
                found.skins,
                found.npc.as_ref().map(|n| &n.bake_name),
                found.npc.as_ref().map(|n| n.equipment),
            );
        }
    }
}
