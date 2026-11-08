//! What a body wears and holds, as plain data: which item models stand on it, and where. The armor's
//! textures and geosets need nothing here, since the composite and the geoset table read the same
//! columns. Taken from the path and attachment rules of `benilla-app`'s `entities/equipment`.

use benilla_formats::{ChrRaces, ItemDisplayCatalog, NpcAppearance};
use bevy::prelude::*;

/// `M2` attachment ids a worn item hangs from. 0 is the left forearm, where a shield goes.
pub const ATTACH_SHIELD: u16 = 0;
pub const ATTACH_HAND_RIGHT: u16 = 1;
pub const ATTACH_HAND_LEFT: u16 = 2;
pub const ATTACH_SHOULDER_RIGHT: u16 = 5;
pub const ATTACH_SHOULDER_LEFT: u16 = 6;
pub const ATTACH_HELM: u16 = 11;

/// Where an appearance's columns hold the helm and the shoulders.
const COLUMN_HELM: usize = 0;
const COLUMN_SHOULDER: usize = 1;

/// Where a cloak's texture lives.
pub(crate) const CAPE_DIR: &str = "Item\\ObjectComponents\\Cape";

/// `ItemDisplayInfo` ids in a body's hands, 0 for nothing: a player's weapons, or a creature's from
/// its equipment row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Held {
    pub main_hand: u32,
    pub off_hand: u32,
    pub ranged: u32,
}

/// `ItemDisplayInfo` ids on a body, 0 for nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Worn {
    /// In body-slot order: head, shoulder, shirt, chest, belt, pants, boots, wrist, gloves, tabard,
    /// as `CreatureDisplayInfoExtra` holds them.
    pub equipment: [u32; 10],
    /// The cloak, which `CreatureDisplayInfoExtra` has no column for: a geoset and a texture on the
    /// body's own cape batches.
    pub back: u32,
    pub held: Held,
}

/// Which of a body's items a worn model is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WornSlot {
    Head,
    Shoulder,
    MainHand,
    OffHand,
    Ranged,
}

impl WornSlot {
    /// A held item stands only where its wearer's model authors the point: a creature with no hand
    /// holds nothing, rather than wearing a sword at its feet.
    pub fn is_held(self) -> bool {
        matches!(self, Self::MainHand | Self::OffHand | Self::Ranged)
    }
}

/// Which of a body's held items show, on its root; none is [`Drawn::Melee`]. The 1.12 client shows a
/// ranged weapon only while it is drawn, in the hands, and the melee pair otherwise. With no item
/// sheath types to hand, the pair is hidden while the ranged one is drawn, not stowed on the back.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Drawn {
    #[default]
    Melee,
    Ranged,
}

impl Drawn {
    pub fn shows(self, slot: WornSlot) -> bool {
        match slot {
            WornSlot::Head | WornSlot::Shoulder => true,
            WornSlot::MainHand | WornSlot::OffHand => self == Self::Melee,
            WornSlot::Ranged => self == Self::Ranged,
        }
    }
}

/// On every item model standing on a body, so a caller can find what its wearer wears, and see one
/// that failed.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WornBy {
    pub wearer: Entity,
    pub slot: WornSlot,
}

/// One item model to stand on a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rider {
    pub path: String,
    pub texture: Option<String>,
    pub attach: u16,
    pub slot: WornSlot,
}

fn rider(dir: &str, name: String, texture: Option<&String>, attach: u16, slot: WornSlot) -> Rider {
    Rider {
        path: format!("Item\\ObjectComponents\\{dir}\\{name}"),
        texture: texture.cloned(),
        attach,
        slot,
    }
}

/// A cloak on a body: its first geoset group and its texture under [`CAPE_DIR`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cape {
    pub geoset: u32,
    pub texture: String,
}

/// A cloak display's cape, as `benilla-app`'s `char_skin` reads it: `model_texture[0]` and the first
/// geoset group. `None` for nothing worn, or a row naming no texture.
pub(crate) fn cape(display: u32, items: Option<&ItemDisplayCatalog>) -> Option<Cape> {
    let row = items?.get(display).filter(|_| display != 0)?;
    Some(Cape {
        geoset: row.geoset_groups[0],
        texture: row.model_texture[0].clone()?,
    })
}

/// `<stem>_<prefix><M|F>.m2`: a helm has a model per race and sex. `None` for a race with no prefix.
fn helm_file(model: &str, races: &ChrRaces, race: u8, sex: u8) -> Option<String> {
    let prefix = races.client_prefix(u32::from(race))?;
    let letter = if sex == 1 { 'F' } else { 'M' };
    let stem = model.strip_suffix(".m2").unwrap_or(model);
    Some(format!("{stem}_{prefix}{letter}.m2"))
}

fn named(model: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        model
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    })
}

/// A shield's model, which lives under `Shield\` and hangs from the forearm. The client knows by the
/// item's inventory type, which a display row does not carry; every shield and buckler model the
/// client ships is named for what it is.
fn is_shield(model: &str) -> bool {
    named(model, &["Shield_", "Buckler_"])
}

/// The folder under `Item\ObjectComponents` a held item's model is read from: `Shield` for a
/// shield ([`is_shield`]), else `Weapon`, which also holds torches, orbs and the like.
pub fn held_folder(model: &str) -> &'static str {
    if is_shield(model) { "Shield" } else { "Weapon" }
}

/// A bow, drawn in the left hand; a gun, crossbow, wand or thrown weapon is drawn in the right
/// (`benilla-app`'s `placement`, by inventory type there and by the file's name here).
fn is_bow(model: &str) -> bool {
    named(model, &["Bow_"])
}

/// Every item model an appearance's helm and shoulder columns stand on it. A row naming no model
/// adds nothing: a helm can be only a hide mask, and most armor is only textures.
pub(crate) fn npc_riders(
    npc: &NpcAppearance,
    items: Option<&ItemDisplayCatalog>,
    races: Option<&ChrRaces>,
) -> Vec<Rider> {
    let Some(items) = items else {
        return Vec::new();
    };
    let mut riders = Vec::new();
    if let Some(row) = items.get(npc.equipment[COLUMN_HELM])
        && let (Some(model), Some(races)) = (row.model[0].as_deref(), races)
        && let Some(file) = helm_file(model, races, npc.race, npc.sex)
    {
        riders.push(rider(
            "Head",
            file,
            row.model_texture[0].as_ref(),
            ATTACH_HELM,
            WornSlot::Head,
        ));
    }
    if let Some(row) = items.get(npc.equipment[COLUMN_SHOULDER]) {
        // The display's first column is the left shoulder.
        for (column, attach) in [ATTACH_SHOULDER_LEFT, ATTACH_SHOULDER_RIGHT]
            .into_iter()
            .enumerate()
        {
            if let Some(model) = row.model[column].clone() {
                riders.push(rider(
                    "Shoulder",
                    model,
                    row.model_texture[column].as_ref(),
                    attach,
                    WornSlot::Shoulder,
                ));
            }
        }
    }
    riders
}

/// The item models in a body's hands, each where the client draws it: the main hand's in the right,
/// an off-hand shield on the forearm and any other off-hand item in the left, a bow in the left and
/// any other ranged item in the right. All of them stand; [`Drawn`] says which show.
pub(crate) fn held_riders(held: &Held, items: Option<&ItemDisplayCatalog>) -> Vec<Rider> {
    let Some(items) = items else {
        return Vec::new();
    };
    let mut riders = Vec::new();
    for (display, slot) in [
        (held.main_hand, WornSlot::MainHand),
        (held.off_hand, WornSlot::OffHand),
        (held.ranged, WornSlot::Ranged),
    ] {
        let Some(row) = items.get(display).filter(|_| display != 0) else {
            continue;
        };
        // A thrown weapon names its model twice; the first is the one in the hand.
        let Some(model) = row.model[0].clone() else {
            continue;
        };
        let shield = is_shield(&model);
        let dir = held_folder(&model);
        let attach = match slot {
            WornSlot::OffHand if shield => ATTACH_SHIELD,
            WornSlot::OffHand => ATTACH_HAND_LEFT,
            WornSlot::Ranged if is_bow(&model) => ATTACH_HAND_LEFT,
            _ => ATTACH_HAND_RIGHT,
        };
        riders.push(rider(
            dir,
            model,
            row.model_texture[0].as_ref(),
            attach,
            slot,
        ));
    }
    riders
}

/// Keeps each held item's visibility to its wearer's [`Drawn`], leaving armor alone. Shown is
/// `Inherited`, so a hidden wearer hides it too.
pub(crate) fn show_drawn(
    mut riders: Query<(&WornBy, &mut Visibility)>,
    wearers: Query<Option<&Drawn>>,
) {
    for (worn, mut visibility) in &mut riders {
        if !worn.slot.is_held() {
            continue;
        }
        let drawn = wearers
            .get(worn.wearer)
            .ok()
            .flatten()
            .copied()
            .unwrap_or_default();
        let want = if drawn.shows(worn.slot) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(want);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use benilla_formats::{ItemDisplay, load_chr_races, load_item_display_catalog};

    use super::*;

    fn wearing(race: u8, sex: u8, helm: u32, shoulder: u32) -> NpcAppearance {
        let mut equipment = [0; 10];
        equipment[COLUMN_HELM] = helm;
        equipment[COLUMN_SHOULDER] = shoulder;
        NpcAppearance {
            race,
            sex,
            skin: 0,
            face: 0,
            hair_style: 0,
            hair_color: 0,
            facial_hair: 0,
            equipment,
            bake_name: None,
        }
    }

    fn row(model: &str, texture: &str, geoset: u32) -> ItemDisplay {
        ItemDisplay {
            model: [(!model.is_empty()).then(|| model.to_owned()), None],
            model_texture: [Some(texture.to_owned()), None],
            geoset_groups: [geoset, 0, 0],
            ..ItemDisplay::default()
        }
    }

    fn catalog(rows: Vec<(u32, ItemDisplay)>) -> ItemDisplayCatalog {
        ItemDisplayCatalog::from_displays(rows.into_iter().collect::<HashMap<_, _>>())
    }

    #[test]
    fn with_no_item_table_nothing_stands_on_a_body() {
        assert!(npc_riders(&wearing(1, 0, 7, 7), None, None).is_empty());
        let held = Held {
            main_hand: 7,
            off_hand: 7,
            ranged: 7,
        };
        assert!(held_riders(&held, None).is_empty());
        assert_eq!(cape(7, None), None);
    }

    #[test]
    fn each_hand_holds_its_item_where_the_client_draws_it() {
        let items = catalog(vec![
            (1, row("Sword_1H_Short_A_02.m2", "Sword", 0)),
            (2, row("Shield_Round_A_01.m2", "Shield", 0)),
            (3, row("buckler_round_a_01.m2", "Buckler", 0)),
            (4, row("Hand_1H_Claw_A_01Left.m2", "Claw", 0)),
            (5, row("Bow_1H_Standard_B_01.m2", "Bow", 0)),
            (6, row("Firearm_2H_Rifle_A_01.m2", "Rifle", 0)),
            (7, row("Crossbow_1H_A_01.m2", "Crossbow", 0)),
            (8, row("", "Cape", 0)),
        ]);
        let at = |held: Held| -> Vec<(String, u16, WornSlot)> {
            held_riders(&held, Some(&items))
                .into_iter()
                .map(|r| (r.path, r.attach, r.slot))
                .collect()
        };
        let weapon = |name: &str| format!("Item\\ObjectComponents\\Weapon\\{name}");
        let shield = |name: &str| format!("Item\\ObjectComponents\\Shield\\{name}");
        assert_eq!(
            at(Held {
                main_hand: 1,
                off_hand: 2,
                ranged: 5,
            }),
            [
                (weapon("Sword_1H_Short_A_02.m2"), 1, WornSlot::MainHand),
                (shield("Shield_Round_A_01.m2"), 0, WornSlot::OffHand),
                (weapon("Bow_1H_Standard_B_01.m2"), 2, WornSlot::Ranged),
            ]
        );
        assert_eq!(
            at(Held {
                main_hand: 0,
                off_hand: 3,
                ranged: 6,
            }),
            [
                (shield("buckler_round_a_01.m2"), 0, WornSlot::OffHand),
                (weapon("Firearm_2H_Rifle_A_01.m2"), 1, WornSlot::Ranged),
            ],
            "a buckler is a shield whatever its case, and a gun is drawn in the right hand"
        );
        assert_eq!(
            at(Held {
                main_hand: 1,
                off_hand: 4,
                ranged: 7,
            }),
            [
                (weapon("Sword_1H_Short_A_02.m2"), 1, WornSlot::MainHand),
                (weapon("Hand_1H_Claw_A_01Left.m2"), 2, WornSlot::OffHand),
                (weapon("Crossbow_1H_A_01.m2"), 1, WornSlot::Ranged),
            ],
            "an off-hand weapon is in the left hand, and a crossbow is not a bow"
        );
        assert!(
            at(Held {
                main_hand: 8,
                off_hand: 99,
                ranged: 0,
            })
            .is_empty(),
            "a row with no model, an unknown display and an empty slot hold nothing"
        );
    }

    #[test]
    fn a_cloak_is_its_first_geoset_and_its_texture() {
        let items = catalog(vec![
            (8, row("", "Cape_Leather_B_01Black", 3)),
            (9, ItemDisplay::default()),
        ]);
        assert_eq!(
            cape(8, Some(&items)),
            Some(Cape {
                geoset: 3,
                texture: "Cape_Leather_B_01Black".to_owned(),
            })
        );
        assert_eq!(cape(9, Some(&items)), None, "a row with no texture");
        assert_eq!(cape(0, Some(&items)), None, "nothing worn");
    }

    #[test]
    fn the_melee_pair_or_the_ranged_item_shows_and_armor_always_does() {
        use WornSlot::*;
        for slot in [Head, Shoulder, MainHand, OffHand] {
            assert!(Drawn::Melee.shows(slot), "{slot:?}");
        }
        assert!(!Drawn::Melee.shows(Ranged));
        assert!(Drawn::Ranged.shows(Ranged));
        assert!(!Drawn::Ranged.shows(MainHand) && !Drawn::Ranged.shows(OffHand));
        assert!(Drawn::Ranged.shows(Head) && Drawn::Ranged.shows(Shoulder));
        assert!(!Head.is_held() && !Shoulder.is_held());
        assert!(MainHand.is_held() && OffHand.is_held() && Ranged.is_held());
    }

    #[test]
    fn a_held_item_follows_its_wearers_stance() {
        let mut app = App::new();
        app.add_systems(Update, show_drawn);
        let wearer = app.world_mut().spawn(Drawn::Ranged).id();
        let bare = app.world_mut().spawn_empty().id();
        let item = |app: &mut App, wearer, slot| {
            app.world_mut()
                .spawn((WornBy { wearer, slot }, Visibility::Inherited))
                .id()
        };
        let sword = item(&mut app, wearer, WornSlot::MainHand);
        let bow = item(&mut app, wearer, WornSlot::Ranged);
        let helm = item(&mut app, wearer, WornSlot::Head);
        let other_bow = item(&mut app, bare, WornSlot::Ranged);
        app.update();
        let seen = |app: &App, e| *app.world().get::<Visibility>(e).expect("visibility");
        assert_eq!(seen(&app, sword), Visibility::Hidden);
        assert_eq!(seen(&app, bow), Visibility::Inherited);
        assert_eq!(seen(&app, helm), Visibility::Inherited);
        assert_eq!(
            seen(&app, other_bow),
            Visibility::Hidden,
            "no stance is melee"
        );
        app.world_mut().entity_mut(wearer).insert(Drawn::Melee);
        app.update();
        assert_eq!(seen(&app, sword), Visibility::Inherited);
        assert_eq!(seen(&app, bow), Visibility::Hidden);
    }

    // Against the client's own tables, skipped without one: the probe's helm, shoulders and sword
    // name real files, for two races' helms, and so do v1's shields, bow, guns, claws and cloak.
    #[test]
    fn worn_items_name_real_files_in_a_real_client() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let items = load_item_display_catalog(&mut chain).expect("items");
        let races = load_chr_races(&mut chain).expect("races");
        let mut check = |rider: &Rider| {
            println!("{rider:?}");
            assert!(chain.read_file(&rider.path).is_ok(), "{}", rider.path);
            let dir = rider.path.rsplit_once('\\').expect("a dir").0;
            let texture = format!("{dir}\\{}.blp", rider.texture.as_ref().expect("a skin"));
            assert!(chain.read_file(&texture).is_ok(), "{texture}");
        };
        for (race, sex) in [(1, 0), (6, 1)] {
            let riders = npc_riders(&wearing(race, sex, 1128, 1068), Some(&items), Some(&races));
            let points: Vec<u16> = riders.iter().map(|r| r.attach).collect();
            assert_eq!(points, [11, 6, 5], "{riders:?}");
            riders.iter().for_each(&mut check);
        }
        // A sword, a tower shield and a bow; a sabre, a buckler and a rifle; a claw in each hand
        // and a thrown axe.
        for (held, points) in [
            ((862, 2559, 20668), [1, 0, 2]),
            ((7420, 18484, 6593), [1, 0, 1]),
            ((7430, 21144, 16751), [1, 2, 1]),
        ] {
            let held = Held {
                main_hand: held.0,
                off_hand: held.1,
                ranged: held.2,
            };
            let riders = held_riders(&held, Some(&items));
            let got: Vec<u16> = riders.iter().map(|r| r.attach).collect();
            assert_eq!(got, points, "{riders:?}");
            riders.iter().for_each(&mut check);
        }
        let cloak = cape(22998, Some(&items)).expect("a cloak");
        let texture = format!("{CAPE_DIR}\\{}.blp", cloak.texture);
        assert!(chain.read_file(&texture).is_ok(), "{texture}");
    }
}
