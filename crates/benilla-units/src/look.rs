//! What a display looks like, as plain data, so the rules are testable with no GPU and no client.

use crate::gear::{Cape, Worn};

use benilla_formats::{
    CharCreateCatalog, CharSkinSlot, CreatureCatalog, EquipGeosets, ItemDisplay,
    ItemDisplayCatalog, NpcAppearance, forearm_dressed,
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
    /// What an item's object batches wear, a bare name beside the model.
    pub object_texture: Option<String>,
    /// A character's cloak: its cape geoset, and what the body's object batches wear.
    pub cape: Option<Cape>,
}

/// A bare model at its file's scale: an item or a spell's effect.
pub(crate) fn prop_look(model_path: String, object_texture: Option<String>) -> CreatureLook {
    CreatureLook {
        model_path,
        scale: 1.0,
        skins: [None, None, None],
        npc: None,
        object_texture,
        cape: None,
    }
}

/// `None` for an unknown display or a zero-scale row, both of which the client draws as nothing.
pub fn creature_look(catalog: &CreatureCatalog, display_id: u32) -> Option<CreatureLook> {
    let model = catalog.model(display_id)?;
    (model.scale > 0.0).then_some(CreatureLook {
        model_path: model.model_path,
        scale: model.scale,
        skins: model.textures,
        npc: model.npc_appearance,
        object_texture: None,
        cape: None,
    })
}

/// A player's looks: `ChrRaces` race, sex (0 male, 1 female) and the character screen's dials.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CharacterDials {
    pub race: u8,
    pub sex: u8,
    pub skin: u8,
    pub face: u8,
    pub hair_style: u8,
    pub hair_color: u8,
    pub facial_hair: u8,
}

/// A race's body display wearing these dials; `None` for a race or sex with no body.
pub fn character_look(
    creatures: &CreatureCatalog,
    create: &CharCreateCatalog,
    dials: CharacterDials,
    worn: &Worn,
) -> Option<CreatureLook> {
    let display = create.body_display(dials.race, dials.sex)?;
    Some(dressed_look(
        creature_look(creatures, display)?,
        dials,
        worn,
    ))
}

/// A body display's look with the dials on, as an unbaked NPC wearing `worn`'s columns.
pub fn dressed_look(body: CreatureLook, dials: CharacterDials, worn: &Worn) -> CreatureLook {
    CreatureLook {
        npc: Some(NpcAppearance {
            race: dials.race,
            sex: dials.sex,
            skin: dials.skin,
            face: dials.face,
            hair_style: dials.hair_style,
            hair_color: dials.hair_color,
            facial_hair: dials.facial_hair,
            equipment: worn.equipment,
            bake_name: None,
        }),
        ..body
    }
}

/// The appearance whose body skin must be composited: a `Character\` model with no bake.
pub(crate) fn composited(look: &CreatureLook) -> Option<&NpcAppearance> {
    look.npc
        .as_ref()
        .filter(|npc| npc.bake_name.is_none() && is_character_model(&look.model_path))
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
    /// A body with no bake: the atlas composited from `CharSections`.
    Composite,
    /// A humanoid's hair (`CharSkinSlot::Hair`).
    Hair,
    /// A furred race's extra skin (`CharSkinSlot::SkinExtra`).
    SkinExtra,
    /// A character's cloak on its object batches: this file under `Item\ObjectComponents\Cape`.
    Cape(&'a str),
}

/// A batch's texture. A humanoid's body slots read its appearance, and a plain creature's read its
/// skins.
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
                    .map_or(PartSkin::Composite, PartSkin::Baked);
            }
            Some(CharSkinSlot::Hair) => return PartSkin::Hair,
            Some(CharSkinSlot::SkinExtra) => return PartSkin::SkinExtra,
            Some(CharSkinSlot::Object) => {
                if let Some(cape) = &look.cape {
                    return PartSkin::Cape(&cape.texture);
                }
            }
            None => {}
        }
    }
    if char_slot == Some(CharSkinSlot::Object)
        && let Some(name) = look.object_texture.as_deref()
    {
        return PartSkin::Creature(name);
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
/// the armor and 0 the helm. It has no cloak column, so a character's cloak comes as `cape`.
pub(crate) fn npc_geosets(
    npc: &NpcAppearance,
    cape: Option<&Cape>,
    items: Option<&ItemDisplayCatalog>,
) -> EquipGeosets {
    let mut geosets = EquipGeosets {
        cloak: cape.map(|cape| cape.geoset),
        ..EquipGeosets::default()
    };
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
            object_texture: None,
            cape: None,
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
    fn a_humanoid_with_no_bake_wears_a_composite() {
        let npc = look(
            "Character\\Human\\Male\\HumanMale.mdx",
            Some(appearance(None)),
        );
        assert_eq!(
            part_skin(&npc, Some(CharSkinSlot::Body), None, false),
            PartSkin::Composite
        );
        assert!(composited(&npc).is_some());
    }

    #[test]
    fn a_characters_cloak_dresses_its_object_batches_and_a_weapons_texture_stays() {
        let mut body = look(
            "Character\\Human\\Male\\HumanMale.mdx",
            Some(appearance(None)),
        );
        let object = Some(CharSkinSlot::Object);
        assert_eq!(part_skin(&body, object, None, true), PartSkin::Embedded);
        body.cape = Some(Cape {
            geoset: 2,
            texture: "Cape_Cloth_A_02Gold".to_owned(),
        });
        assert_eq!(
            part_skin(&body, object, None, true),
            PartSkin::Cape("Cape_Cloth_A_02Gold")
        );
        assert_eq!(
            npc_geosets(body.npc.as_ref().expect("npc"), body.cape.as_ref(), None).cloak,
            Some(2)
        );
        let mut sword = look("Item\\ObjectComponents\\Weapon\\Sword.m2", None);
        sword.object_texture = Some("SwordSkin".to_owned());
        sword.cape = body.cape.clone();
        assert_eq!(
            part_skin(&sword, object, None, true),
            PartSkin::Creature("SwordSkin"),
            "only a character model wears a cape"
        );
    }

    #[test]
    fn only_an_unbaked_character_model_is_composited() {
        let baked = look(
            "Character\\Human\\Male\\HumanMale.mdx",
            Some(appearance(Some("Bake"))),
        );
        assert!(composited(&baked).is_none());
        let odd = look("Creature\\Bat\\Bat.mdx", Some(appearance(None)));
        assert!(composited(&odd).is_none());
        let beast = look("Creature\\Bat\\Bat.mdx", None);
        assert!(composited(&beast).is_none());
    }

    // The dials reach the look, and the body's own model and scale stay.
    #[test]
    fn a_character_is_its_body_display_with_the_dials_on() {
        let dials = CharacterDials {
            race: 6,
            sex: 1,
            skin: 2,
            face: 3,
            hair_style: 4,
            hair_color: 5,
            facial_hair: 6,
        };
        let body = CreatureLook {
            scale: 1.35,
            ..look("Character\\Tauren\\Female\\TaurenFemale.mdx", None)
        };
        let dressed = dressed_look(body, dials, &Worn::default());
        assert_eq!(dressed.scale, 1.35);
        let npc = composited(&dressed).expect("composited");
        assert_eq!((npc.race, npc.sex, npc.skin, npc.face), (6, 1, 2, 3));
        assert_eq!((npc.hair_style, npc.hair_color, npc.facial_hair), (4, 5, 6));
        assert_eq!(npc.equipment, [0; 10]);
    }

    // Against the client's own tables, skipped without one: all 8 races, both sexes, composite.
    #[test]
    fn every_race_and_sex_composites_in_a_real_client() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let catalog = benilla_formats::load_creature_catalog(&mut chain).expect("creatures");
        let create = CharCreateCatalog::load(&mut chain).expect("create");
        let sections = benilla_formats::CharSections::load(&mut chain).expect("sections");
        for race in 1..=8 {
            for sex in 0..=1 {
                let dials = CharacterDials {
                    race,
                    sex,
                    ..CharacterDials::default()
                };
                let found =
                    character_look(&catalog, &create, dials, &Worn::default()).expect("look");
                assert!(
                    is_character_model(&found.model_path),
                    "{}",
                    found.model_path
                );
                let m2 = found.model_path.replace(".mdx", ".m2");
                assert!(chain.read_file(&m2).is_ok(), "{m2}");
                let plan = sections
                    .composite_plan(race, sex, 0, 0, 0, 0, 0, [None; 8], None, false)
                    .expect("a base skin row");
                let atlas = plan
                    .run(|path| benilla_formats::read_texture_mip_chain(&mut chain, path))
                    .expect("composite");
                println!(
                    "race {race} sex {sex}: {} scale {} atlas {}x{}",
                    found.model_path, found.scale, atlas.width, atlas.height
                );
                assert!(atlas.width >= 256 && atlas.height >= 256);
                println!("  dials {:?}", create.ranges(race, sex));
            }
        }
    }

    // Against the client's own tables, skipped without one: each dial shows on the body.
    #[test]
    fn the_dials_change_the_skin_and_the_geosets_in_a_real_client() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let sections = benilla_formats::CharSections::load(&mut chain).expect("sections");
        let geosets = benilla_formats::CharacterGeosets::load(&mut chain).expect("geosets");
        let mut skin = |skin, face, facial, style, color| {
            let plan = sections
                .composite_plan(
                    1, 0, skin, face, facial, style, color, [None; 8], None, false,
                )
                .expect("a base skin row");
            let atlas = plan
                .run(|path| benilla_formats::read_texture_mip_chain(&mut chain, path))
                .expect("composite");
            atlas.into_rgba8().mips.swap_remove(0)
        };
        let base = skin(0, 0, 0, 0, 0);
        assert_ne!(base, skin(1, 0, 0, 0, 0), "skin");
        assert_ne!(base, skin(0, 1, 0, 0, 0), "face");
        assert_ne!(skin(0, 0, 1, 0, 0), skin(0, 0, 2, 0, 0), "facial hair");
        let worn = EquipGeosets::default();
        let visible = |style, facial| {
            let set = geosets.visible_geosets(1, 0, style, facial, &worn);
            (0..1700).filter(|id| set.contains(id)).collect::<Vec<_>>()
        };
        assert_ne!(visible(0, 0), visible(3, 0), "hair style");
        assert_ne!(visible(0, 1), visible(0, 4), "facial hair");
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

    // An item's batches wear what its display row names, never a guess from the file.
    #[test]
    fn an_object_slot_wears_the_items_texture() {
        let sword = prop_look("Item\\Sword.m2".to_owned(), Some("Sword_Blue".to_owned()));
        assert_eq!(
            part_skin(&sword, Some(CharSkinSlot::Object), None, false),
            PartSkin::Creature("Sword_Blue")
        );
        assert_eq!(part_skin(&sword, None, None, true), PartSkin::Embedded);
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
        let geosets = npc_geosets(&npc, None, None);
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
