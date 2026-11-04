//! A display id to entities on screen. The root is the caller's; the batches are its children.

use benilla_assets::coords::wow_to_bevy;
use benilla_assets::materials::WowModelMaterial;
use benilla_assets::{
    M2Model, ModelAttachment, ModelSubmesh, WorldAssets, m2_url, skin_url, texture_url,
};
use benilla_formats::{ModelBlend, NpcAppearance, VisibleGeosets};
use benilla_world::billboard::BillboardCard;
use benilla_world::doodad_anim::{MatAnim, UvAnimMaterials};
use benilla_world::entity_shade::GroundShade;
use benilla_world::interior::{BodyBakeCenter, part_interior_lit};
use benilla_world::mat_anim_table::MatAnimTable;
use benilla_world::mesh_tag::spawn_tag;
use benilla_world::model_forms::ModelForms;
use benilla_world::model_render::{
    BatchVariants, EntityUvLane, M2BatchMaterials, ModelKind, ModelPart,
};
use benilla_world::particles::{self, EmitClock, EmitterFrames, OwnerLoss};
use benilla_world::ribbons::{self, RibbonSeq};
use benilla_world::rig_anim::{GlobalSeqDrive, RigPose};
use benilla_world::rig_palette::{RigPalettes, RigPart, RigSkin};
use bevy::animation::graph::AnimationGraphHandle;
use bevy::animation::transition::AnimationTransitions;
use bevy::asset::LoadState;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::system::SystemParam;
use bevy::mesh::MeshTag;
use bevy::prelude::*;

use crate::clips::ClipDriver;
use crate::composite::{BodyAtlas, SkinComposites, SkinKey, composite_plan, work};
use crate::gear::{Worn, npc_riders};
use crate::look::{
    CharacterDials, CreatureLook, PartSkin, character_look, composited, creature_look, model_dir,
    npc_geosets, part_skin, prop_look,
};
use crate::seat::SeatOn;
use crate::tables::UnitTables;

/// What the client sets on a character texture's sampler: both axes clamp.
const CHAR_TEXTURE_WRAP: (bool, bool) = (false, false);

/// The creature table, when a body asks and no client was opened.
const NO_TABLES: &str = "DBFilesClient/CreatureDisplayInfo.dbc";
/// The race table, when a character has no body there.
const NO_RACE: &str = "DBFilesClient/ChrRaces.dbc";
/// The skin table, when a body has no composite.
const NO_SKIN: &str = "DBFilesClient/CharSections.dbc";

/// What a body was asked to be.
enum BodySource {
    Display(u32),
    Character(CharacterDials, Worn),
    /// A bare model: a worn item, or a spell's effect.
    Model {
        path: String,
        texture: Option<String>,
        /// Draw its particles and ribbons.
        fx: bool,
    },
}

/// A body standing in the world. Put it on a root with a `Transform` and a `Visibility`: the root's
/// translation and rotation stay the caller's, and dressing sets its scale from the display.
#[derive(Component)]
#[require(ClipDriver)]
pub struct UnitBody {
    source: BodySource,
    state: BodyState,
    look: Option<CreatureLook>,
    model: Option<Handle<M2Model>>,
    /// A composited body's skin, once it has landed.
    atlas: Option<Handle<Image>>,
}

impl UnitBody {
    /// A `CreatureDisplayInfo.dbc` id.
    pub fn creature(display: u32) -> Self {
        Self::of(BodySource::Display(display))
    }

    /// A player's body: the race's display, its skin composited from these dials. It draws nothing
    /// until the composite lands, as the client does for a unit's first.
    pub fn character(dials: CharacterDials) -> Self {
        Self::character_wearing(dials, Worn::default())
    }

    /// A player's body wearing these item displays: the armor is painted into its composite and
    /// shapes its geosets, and the helm, shoulders and weapon stand on it as riders ([`SeatOn`]).
    pub fn character_wearing(dials: CharacterDials, worn: Worn) -> Self {
        Self::of(BodySource::Character(dials, worn))
    }

    /// A bare model at its file's own scale: `texture` is what its object batches wear, a bare
    /// name beside the model.
    pub fn prop(path: impl Into<String>, texture: Option<String>) -> Self {
        Self::of(BodySource::Model {
            path: path.into(),
            texture,
            fx: false,
        })
    }

    /// A spell's model, with its particles and ribbons, looping its idle. Despawning it stops the
    /// emitters, and what they have already thrown ages out on its own.
    pub fn effect(path: impl Into<String>) -> Self {
        Self::of(BodySource::Model {
            path: path.into(),
            texture: None,
            fx: true,
        })
    }

    fn of(source: BodySource) -> Self {
        Self {
            source,
            state: BodyState::Loading,
            look: None,
            model: None,
            atlas: None,
        }
    }

    pub fn state(&self) -> &BodyState {
        &self.state
    }
}

/// Where a body stands.
#[derive(Clone)]
pub enum BodyState {
    Loading,
    Ready(BodyFacts),
    /// The file or table that could not be used. No part spawned.
    Failed(String),
}

/// What a dressed model says about itself.
#[derive(Clone, Default)]
pub struct BodyFacts {
    pub attachments: Vec<ModelAttachment>,
    /// The authored box's height in model-local yards, before the root's scale.
    pub height: f32,
}

/// A billboard card belongs to its body but is a world root, as the billboard system requires, so
/// this despawns it with the body.
#[derive(Component)]
#[relationship(relationship_target = BodyCards)]
pub struct CardOf(pub Entity);

/// The cards of a body.
#[derive(Component, Default)]
#[relationship_target(relationship = CardOf, linked_spawn)]
pub struct BodyCards(Vec<Entity>);

/// The world point of attachment `id` at the composed pose; `None` for a model that does not author
/// it, or one with no rig.
pub fn attachment_point(
    facts: &BodyFacts,
    pose: Option<&RigPose>,
    root: &GlobalTransform,
    id: u16,
) -> Option<Vec3> {
    let attachment = facts.attachments.iter().find(|a| a.id == id)?;
    pose?.posed_point(root, attachment.bone, attachment.offset)
}

/// What dressing writes to. The models are a separate parameter, since a batch being dressed is
/// borrowed from them.
#[derive(SystemParam)]
pub(crate) struct Dressing<'w, 's> {
    commands: Commands<'w, 's>,
    server: Res<'w, AssetServer>,
    forms: ResMut<'w, ModelForms>,
    meshes: ResMut<'w, Assets<Mesh>>,
    mats: M2BatchMaterials<'w>,
    palettes: ResMut<'w, RigPalettes>,
    uv_reg: ResMut<'w, UvAnimMaterials>,
    anim_table: ResMut<'w, MatAnimTable>,
    tables: Option<Res<'w, UnitTables>>,
    composites: ResMut<'w, SkinComposites>,
    assets: Option<Res<'w, WorldAssets>>,
}

pub(crate) fn dress_bodies(
    // A rider waits for its seat, or it would stand at the origin until then.
    mut bodies: Query<(Entity, &mut UnitBody, &mut Transform), Without<SeatOn>>,
    m2s: Res<Assets<M2Model>>,
    mut dressing: Dressing,
) {
    for (root, mut body, mut transform) in &mut bodies {
        if matches!(body.state, BodyState::Loading)
            && let Some(state) = dressing.step(root, &mut body, &mut transform, &m2s)
        {
            // Nobody watches a worn item, so its failure is said here, and the wearer stands on.
            if let (BodyState::Failed(path), BodySource::Model { fx: false, .. }) =
                (&state, &body.source)
            {
                warn!("worn item {path} failed");
            }
            body.state = state;
        }
    }
}

impl Dressing<'_, '_> {
    /// A new state, or `None` to look again next frame.
    fn step(
        &mut self,
        root: Entity,
        body: &mut UnitBody,
        transform: &mut Transform,
        m2s: &Assets<M2Model>,
    ) -> Option<BodyState> {
        if body.look.is_none() {
            let Some(tables) = self.tables.as_ref() else {
                return Some(BodyState::Failed(NO_TABLES.to_owned()));
            };
            let look = match &body.source {
                BodySource::Display(display) => creature_look(&tables.creatures, *display)
                    .ok_or_else(|| format!("{NO_TABLES} (no display {display})")),
                BodySource::Model { path, texture, .. } => {
                    Ok(prop_look(path.clone(), texture.clone()))
                }
                BodySource::Character(dials, worn) => tables
                    .create
                    .as_ref()
                    .and_then(|create| character_look(&tables.creatures, create, *dials, worn))
                    .ok_or_else(|| {
                        format!(
                            "{NO_RACE} (no body for race {} sex {})",
                            dials.race, dials.sex
                        )
                    }),
            };
            let look = match look {
                Ok(look) => look,
                Err(failed) => return Some(BodyState::Failed(failed)),
            };
            body.model = Some(self.server.load(m2_url(&look.model_path)));
            transform.scale = Vec3::splat(look.scale);
            body.look = Some(look);
        }
        let (look, handle) = (body.look.as_ref()?, body.model.as_ref()?);
        if let Some(LoadState::Failed(_)) = self.server.get_load_state(handle.id()) {
            return Some(BodyState::Failed(look.model_path.clone()));
        }
        if body.atlas.is_none()
            && let Some(npc) = composited(look)
        {
            let key = SkinKey::of(npc);
            match self.atlas(key) {
                BodyAtlas::Pending => return None,
                BodyAtlas::Ready(Some(atlas)) => body.atlas = Some(atlas),
                BodyAtlas::Ready(None) => {
                    return Some(BodyState::Failed(format!(
                        "{NO_SKIN} (no body skin for race {} sex {} skin {})",
                        key.race, key.sex, key.skin
                    )));
                }
            }
        }
        // No shared light buffer yet means no materials to build.
        if !self.mats.ready() {
            return None;
        }
        let model = m2s.get(handle)?;
        let atlas = body.atlas.as_ref();
        let fx = matches!(body.source, BodySource::Model { fx: true, .. });
        let main_hand = match body.source {
            BodySource::Character(_, worn) => worn.main_hand,
            _ => 0,
        };
        let facts = self.dress(root, look, handle, model, atlas, fx);
        self.spawn_riders(root, look, main_hand);
        Some(BodyState::Ready(facts))
    }

    /// A look's composite: done, or running, its work started here on a miss.
    fn atlas(&mut self, key: SkinKey) -> BodyAtlas {
        let (tables, assets) = (self.tables.as_ref(), self.assets.as_ref());
        self.composites.request(key, || {
            let tables = tables?;
            let plan = composite_plan(&key, tables.sections.as_ref()?, tables.items.as_ref())?;
            Some(work(plan, assets?.chain.clone()))
        })
    }

    fn dress(
        &mut self,
        root: Entity,
        look: &CreatureLook,
        handle: &Handle<M2Model>,
        model: &M2Model,
        atlas: Option<&Handle<Image>>,
        fx: bool,
    ) -> BodyFacts {
        // Uncapped, like the client's one-off lanes: a handful of models at most.
        self.forms
            .ensure_now_rigged(handle, &model.submeshes, &mut self.meshes);
        let bones = model.skeleton.joints.len() as u32;
        let rig = (bones > 0)
            .then(|| {
                RigSkin::allocate_bones(&mut self.palettes, bones, model.inverse_bindposes.clone())
            })
            .flatten();
        // Slot 0 is no palette: static parts at the bind pose.
        let slot = rig.as_ref().map_or(0, |rig| rig.slot);
        if bones > 0 && slot == 0 {
            warn!(
                "rig palette full: {} stands at its bind pose",
                look.model_path
            );
        }
        let mut pose = (slot != 0).then(|| RigPose::new(root, &model.skeleton));
        let center = model.bounds.map_or(Vec3::ZERO, |b| {
            wow_to_bevy([
                (b.bbox_min[0] + b.bbox_max[0]) * 0.5,
                (b.bbox_min[1] + b.bbox_max[1]) * 0.5,
                (b.bbox_min[2] + b.bbox_max[2]) * 0.5,
            ])
        });
        let mut entity = self.commands.entity(root);
        entity.insert((BodyBakeCenter(center), GroundShade::default()));
        if let Some(rig) = rig {
            entity.insert(rig);
        }
        if let Some(anims) = model.animations.as_ref() {
            arm_idle(&mut self.commands, root, anims, bones, slot != 0);
        }

        let visible = look.npc.as_ref().and_then(|npc| self.visible_geosets(npc));
        for (index, sub) in model.submeshes.iter().enumerate() {
            let hidden = visible
                .as_ref()
                .is_some_and(|set| !set.contains(&sub.geoset_id));
            if !hidden {
                let batch = Batch {
                    root,
                    handle,
                    sub,
                    index,
                    slot,
                    center,
                    atlas,
                };
                self.spawn_part(look, &batch, pose.as_mut());
            }
        }
        if fx {
            self.spawn_fx(root, model, pose.as_mut());
        }
        if let Some(pose) = pose {
            self.commands.entity(root).insert(pose);
        }
        BodyFacts {
            attachments: model.attachments.clone(),
            height: model
                .bounds
                .map_or(0.0, |b| (b.bbox_max[2] - b.bbox_min[2]).max(0.0)),
        }
    }

    /// A humanoid's helm, shoulders and weapon, each its own body waiting for its seat.
    fn spawn_riders(&mut self, root: Entity, look: &CreatureLook, main_hand: u32) {
        let Some(npc) = look.npc.as_ref() else {
            return;
        };
        let riders = self.tables.as_ref().map_or_else(Vec::new, |tables| {
            npc_riders(npc, main_hand, tables.items.as_ref(), tables.races.as_ref())
        });
        for rider in riders {
            self.commands.spawn((
                Transform::default(),
                Visibility::default(),
                UnitBody::prop(rider.path, rider.texture),
                SeatOn {
                    wearer: root,
                    points: vec![rider.attach],
                },
            ));
        }
    }

    /// An effect's particles and ribbons, each riding its bone's live anchor, as `benilla-app`'s
    /// `spell_fx` hangs them on its joints. Losing the owner stops the emitter and lets its pool age
    /// out, since a spell's model is over when it is over.
    fn spawn_fx(&mut self, root: Entity, model: &M2Model, mut pose: Option<&mut RigPose>) {
        // The clock is the model's own player when it has clips, which `arm_idle` has started.
        let clock = if model.animations.is_some() {
            EmitClock::Host(root)
        } else {
            EmitClock::Pinned
        };
        for emitter in &model.emitters {
            let owner = pose
                .as_deref_mut()
                .and_then(|pose| pose.anchor_for(&mut self.commands, root, emitter.def.bone))
                .map_or((root, [0.0; 3]), |joint| (joint, emitter.bone_pivot));
            particles::spawn_emitter(
                &mut self.commands,
                emitter,
                Transform::IDENTITY,
                EmitterFrames {
                    owner: Some(owner),
                    // The cloud sorts at the model; the bone only places births.
                    anchor: Some(root),
                    alpha: Some(root),
                    light_node: None,
                    on_owner_loss: OwnerLoss::Drain,
                },
                clock,
            );
        }
        for ribbon in &model.ribbons {
            let joint = pose
                .as_deref_mut()
                .and_then(|pose| pose.anchor_for(&mut self.commands, root, ribbon.def.bone));
            ribbons::spawn_ribbon(
                &mut self.commands,
                ribbon,
                joint.unwrap_or(root),
                joint.is_some(),
                1.0,
                RibbonSeq::Host(root),
                Some(root),
                None,
            );
        }
    }

    fn visible_geosets(&self, npc: &NpcAppearance) -> Option<VisibleGeosets> {
        let tables = self.tables.as_ref()?;
        let equip = npc_geosets(npc, tables.items.as_ref());
        Some(tables.geosets.as_ref()?.visible_geosets(
            npc.race,
            npc.sex,
            npc.hair_style,
            npc.facial_hair,
            &equip,
        ))
    }

    /// The batch's materials: a humanoid's body slots read its look, all else its model and skins.
    fn variants(
        &mut self,
        look: &CreatureLook,
        sub: &ModelSubmesh,
        index: usize,
        atlas: Option<&Handle<Image>>,
    ) -> Option<BatchVariants> {
        let sections = self.tables.as_ref().and_then(|t| t.sections.as_ref());
        let appearance = look.npc.as_ref();
        let skin = part_skin(look, sub.char_slot, sub.skin_slot, sub.texture.is_some());
        if let (PartSkin::Composite, Some(atlas)) = (skin, atlas) {
            return self
                .mats
                .char_variants(atlas.clone(), ModelBlend::Opaque, sub.two_sided);
        }
        let char_path = match skin {
            PartSkin::Baked(name) => Some(format!("Textures\\BakedNpcTextures\\{name}")),
            PartSkin::Hair => appearance
                .zip(sections)
                .and_then(|(npc, sections)| {
                    sections.hair_mesh_texture(npc.race, npc.sex, npc.hair_style, npc.hair_color)
                })
                .map(str::to_owned),
            PartSkin::SkinExtra => appearance
                .zip(sections)
                .and_then(|(npc, sections)| {
                    sections.skin_extra_texture(npc.race, npc.sex, npc.skin)
                })
                .map(str::to_owned),
            PartSkin::Embedded | PartSkin::Creature(_) | PartSkin::Composite => None,
        };
        if let Some(path) = char_path {
            let texture = self.server.load(texture_url(&path, CHAR_TEXTURE_WRAP));
            // The body is one opaque sheet; hair and fringe keep their batches' own blend.
            let blend = match skin {
                PartSkin::Baked(_) => ModelBlend::Opaque,
                _ => sub.blend,
            };
            return self.mats.char_variants(texture, blend, sub.two_sided);
        }
        let texture = match skin {
            PartSkin::Creature(name) => Some(
                self.server
                    .load(skin_url(model_dir(&look.model_path), name)),
            ),
            _ => sub.texture.clone(),
        };
        // Batch order biases the transparent sort, so coplanar batches draw in file order.
        let order = u16::try_from(index + 1).unwrap_or(u16::MAX);
        let mut uv = EntityUvLane {
            reg: &mut self.uv_reg,
            table: &mut self.anim_table,
            instance: None,
        };
        self.mats.entity_variants(sub, texture, order, &mut uv)
    }

    fn spawn_part(&mut self, look: &CreatureLook, batch: &Batch, pose: Option<&mut RigPose>) {
        let Batch {
            root,
            handle,
            sub,
            index,
            slot,
            center,
            atlas,
        } = *batch;
        let Some(variants) = self.variants(look, sub, index, atlas) else {
            return;
        };
        let built = self.forms.slices(handle);
        let stat = built.stat.get(index);
        let aabb = stat.and_then(|(_, aabb)| *aabb);
        let static_mesh = stat.map(|(mesh, _)| mesh.clone()).unwrap_or_default();
        let skinned = (slot != 0)
            .then(|| built.skin.and_then(|forms| forms.get(index)).cloned())
            .flatten();
        let lit = part_interior_lit(
            &variants.steady,
            Some(&variants.interior),
            Some(&variants.interior_bake),
            center,
            root,
        );
        let part = ModelPart {
            kind: ModelKind::Creature,
            blend: sub.blend,
        };
        let material = MeshMaterial3d::<WowModelMaterial>(variants.steady);

        let mut child = match &sub.billboard {
            // Its transform belongs to the billboard system, and it follows its live joint.
            Some(info) => {
                let joint =
                    pose.and_then(|pose| pose.anchor_for(&mut self.commands, root, info.bone));
                let card = match joint {
                    Some(joint) => BillboardCard::following_joint(info, joint),
                    None => BillboardCard::following(info, root),
                };
                self.commands.spawn((
                    Mesh3d(static_mesh),
                    material,
                    Transform::default(),
                    part,
                    card,
                    CardOf(root),
                ))
            }
            None => {
                let rigged = skinned.is_some();
                let mesh = skinned.unwrap_or(static_mesh);
                let mut child = self.commands.spawn((
                    Mesh3d(mesh),
                    material,
                    Transform::default(),
                    ChildOf(root),
                    part,
                ));
                if rigged {
                    // The bones wander from the build-time box, so nothing may cull by it.
                    child.insert((RigPart(root), NoFrustumCulling));
                }
                child
            }
        };
        // The render meshes are render-world only, so nothing derives a bound from them.
        if let Some(aabb) = aabb {
            child.insert(aabb);
        }
        child.insert(MeshTag(spawn_tag(slot, 1.0)));
        if let Some(lit) = lit {
            child.insert(lit);
        }
        if let Some(anim) = &sub.alpha_anim {
            child.insert(MatAnim::following(anim.clone(), root));
        }
    }
}

/// One submesh on its way to the screen.
struct Batch<'a> {
    root: Entity,
    handle: &'a Handle<M2Model>,
    sub: &'a ModelSubmesh,
    index: usize,
    slot: u16,
    center: Vec3,
    atlas: Option<&'a Handle<Image>>,
}

/// Loops the model's idle, until its [`ClipDriver`] asks for another clip.
fn arm_idle(
    commands: &mut Commands,
    root: Entity,
    anims: &benilla_assets::ModelAnimations,
    bones: u32,
    rigged: bool,
) {
    let mut player = AnimationPlayer::default();
    let mut transitions = AnimationTransitions::new();
    if let Some(idle) = anims.idle_clip() {
        transitions
            .play(&mut player, idle.node, std::time::Duration::ZERO)
            .repeat();
    }
    let mut entity = commands.entity(root);
    entity.insert((
        player,
        transitions,
        AnimationGraphHandle(anims.graph.clone()),
        anims.clone(),
    ));
    if rigged && let Some(drive) = GlobalSeqDrive::new_rig(&anims.global_bones, bones as usize) {
        entity.insert(drive);
    }
}
