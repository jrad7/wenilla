//! Spell chain beams on bodies: Chain Lightning's arcs, the Drain Life beam, Chain Heal. A kit whose
//! `CharProc` decodes to a chain ([`ChainProc`]) draws a polyline of hops, `caster → t1 → t2 → …`,
//! one ribbon per hop, subdivided, jittered every frame and scrolled.
//!
//! The arithmetic is `benilla-app`'s `entities/chain_beam.rs` (the reference's `LightningObject` and
//! `CLightning`), less the network: a caller names the caster and targets as [`UnitBody`] entities
//! and spawns a [`ChainBeam`]; it ends itself when its time is up, when its caster goes, or when the
//! caller despawns it. Ends are re-read from the bodies every frame, so a beam follows moving units,
//! and a hop whose body is not dressed, is hidden, or has gone, is not drawn.
//!
//! Deviations from `benilla-app`: `benilla-units` does not read the `$CSL` marker, so the caster's end
//! is its right hand, else its left, else the client's fallback, `0.75` of the model's height above
//! its root (which on a character's box stands well over its head); and the beam keeps its own age,
//! so its clock starts on the frame it is spawned.

use benilla_assets::texture_url;
use benilla_formats::{ChainEffect, ChainProc, MISSILE_ATTACH_TABLE, SpellVisualCatalog};
use benilla_world::particles::buffer::{EffectVertex, WorldEffectDraw};
use benilla_world::rig_anim::RigPose;
use benilla_world::view::WorldCamera;
use bevy::prelude::*;

use crate::body::{BodyState, UnitBody, attachment_point};
use crate::gear::{ATTACH_HAND_LEFT, ATTACH_HAND_RIGHT};

/// Where the caster's end is, in place of the `$CSL` marker this crate does not read: its right hand,
/// then its left, as a spell leaves the hands.
const CASTER_POINTS: [u16; 2] = [ATTACH_HAND_RIGHT, ATTACH_HAND_LEFT];

/// The caster end's height factor with neither the marker nor a hand, `base + modelHeight × modelScale × 0.75`
/// (`0x6ec73e`–`0x6ec771`).
const CASTER_HEIGHT_FACTOR: f32 = 0.75;

/// The attachment a target end falls back to when the spell's `SpellVisual` field 9 names none, the
/// reference's literal `0x22` (`0x6ec7b5`); below it, the body's root.
pub const CHAIN_ATTACH_FALLBACK: u16 = 34;

/// The subdivision floor (`0x7af716`): even a one-yard hop bends.
const SUBDIVISION_FLOOR: f32 = 2.0;

/// The cross-section normalisation guard (`0x7b01bd`).
const PERP_EPSILON: f32 = 0.001;

/// The per-frame advection weight on interior points (`0x7afc10`), so the re-roll crawls.
const ADVECT_KEEP: f32 = 0.75;

/// `benilla-app`'s cap on one hop's sub-segment count; shipped rows peak at 12.
const MAX_SUBDIVISIONS: usize = 256;

/// The longest step a frame advances a beam by, as `benilla-app` clamps it.
const MAX_STEP: f32 = 0.1;

/// A kit's beam: its chain proc and that proc's `SpellChainEffects` row. `None` for a kit with no
/// chain proc, a proc that draws no strand, or an id with no row (the client's own no-op,
/// `0x6ecc2e`).
pub fn kit_beam(catalog: &SpellVisualCatalog, kit: u32) -> Option<(ChainProc, ChainEffect)> {
    let proc_ = catalog.kit(kit)?.chain_proc()?;
    if proc_.beams == 0 {
        return None;
    }
    let effect = catalog.chain_effect(proc_.effect_id)?.clone();
    Some((proc_, effect))
}

/// The attachment a visual's beam ends at on each target: its `SpellVisual` field 9 through the
/// missile table, else [`CHAIN_ATTACH_FALLBACK`] (`0x6ec780`).
pub fn beam_attachment(catalog: &SpellVisualCatalog, visual: u32) -> u16 {
    catalog
        .stages(visual)
        .and_then(|s| MISSILE_ATTACH_TABLE.get(s.missile_attach as usize).copied())
        .unwrap_or(CHAIN_ATTACH_FALLBACK)
}

/// One hop: lit over `[start, end)` of the beam's age; the live polyline per strand, empty while
/// the hop is not drawn.
#[derive(Clone, Debug)]
struct Bolt {
    start: f32,
    end: f32,
    strands: Vec<Vec<Vec3>>,
}

/// A live chain beam from a caster's body through each target's in turn. Spawn it on an entity of
/// its own; it draws in the effect lane each frame and despawns itself when it is over.
#[derive(Component, Clone, Debug)]
pub struct ChainBeam {
    /// `[caster, t1, t2, …]`.
    nodes: Vec<Entity>,
    bolts: Vec<Bolt>,
    effect: ChainEffect,
    dest_tag: u16,
    /// A channel's beam: no clock, it lasts until its caller despawns it or the caster goes.
    held: bool,
    /// `hops × boltLife`; ignored while held.
    expiry: f32,
    age: f32,
    /// The texture-scroll accumulator.
    phase: f32,
    rng: u32,
    texture: Option<Handle<Image>>,
}

impl ChainBeam {
    /// A beam of `proc_.beams` strands (at most [`benilla_formats::CHAIN_MAX_BEAMS`]) from `caster`
    /// through `targets`, ending on each target at `dest_tag` ([`beam_attachment`]). A held beam is
    /// `proc_.flag`'s channel beam; any other burns hop by hop, `boltStagger` apart, and ends after
    /// `hops × boltLife`.
    pub fn new(
        proc_: ChainProc,
        effect: ChainEffect,
        caster: Entity,
        targets: &[Entity],
        dest_tag: u16,
    ) -> Self {
        let strands = proc_.beams.min(benilla_formats::CHAIN_MAX_BEAMS) as usize;
        let stagger = effect.bolt_stagger_ms.min(u32::from(u16::MAX)) as f32 / 1000.0;
        let life = effect.bolt_life_ms as f32 / 1000.0;
        let bolts: Vec<Bolt> = (0..targets.len())
            .map(|i| {
                let start = i as f32 * stagger;
                Bolt {
                    start,
                    end: start + life,
                    strands: vec![Vec::new(); strands],
                }
            })
            .collect();
        let seed = caster.to_bits() as u32 ^ (proc_.effect_id.wrapping_mul(2_654_435_761));
        Self {
            nodes: std::iter::once(caster)
                .chain(targets.iter().copied())
                .collect(),
            expiry: bolts.len() as f32 * life,
            bolts,
            effect,
            dest_tag,
            held: proc_.flag,
            age: 0.0,
            phase: 0.0,
            // xorshift has no zero state.
            rng: seed | 1,
            texture: None,
        }
    }

    /// Whether it lasts until despawned, a channel's beam.
    pub fn held(&self) -> bool {
        self.held
    }

    /// The hops it draws.
    pub fn hops(&self) -> usize {
        self.bolts.len()
    }

    /// Whether its clock has run out; a held beam's never does.
    pub fn over(&self) -> bool {
        !self.held && self.age >= self.expiry
    }

    /// Whether hop `i` burns now.
    fn lit(&self, i: usize) -> bool {
        self.held
            || self
                .bolts
                .get(i)
                .is_some_and(|b| b.start <= self.age && self.age < b.end)
    }
}

/// One xorshift draw in `[−1, 1)`, `benilla-app`'s deviation from the reference's generator.
fn jitter(rng: &mut u32) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    (*rng >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
}

/// The texture scroll (`0x7af9c7`): `phase = fmod(phase + dt, period)`, `u = −(phase / period)`; a
/// zero period disables it, and a negative one flows back toward the caster.
fn advance_scroll(phase: f32, dt: f32, period: f32) -> (f32, f32) {
    if period == 0.0 {
        return (0.0, 0.0);
    }
    let phase = (phase + dt) % period;
    (phase, -(phase / period))
}

/// One hop's fresh subdivision (`0x7af6d0`): `trunc(len / avgSegLen + 2)` segments, exact ends,
/// each interior point offset by an independent `[−1, 1]³` vector scaled by `len × noiseScale`.
fn subdivide(a: Vec3, b: Vec3, effect: &ChainEffect, rng: &mut u32, out: &mut Vec<Vec3>) {
    let len = a.distance(b);
    let n = if effect.avg_seg_len > 0.0 {
        (len / effect.avg_seg_len + SUBDIVISION_FLOOR).trunc() as usize
    } else {
        SUBDIVISION_FLOOR as usize
    }
    .clamp(SUBDIVISION_FLOOR as usize, MAX_SUBDIVISIONS);
    let amp = len * effect.noise_scale;
    out.clear();
    for i in 0..=n {
        let p = a.lerp(b, i as f32 / n as f32);
        out.push(if i == 0 || i == n {
            p
        } else {
            p + Vec3::new(jitter(rng), jitter(rng), jitter(rng)) * amp
        });
    }
}

/// One strand's polyline as quads `[b₀,b₁,t₁,t₀]`, the ribbon lying in the view plane (`d × cam
/// forward`), both ends collapsed to a point at `v = 0.5`, `u` 0→1 caster→target plus the scroll,
/// white (`0x7b0196`–`0x7b0541`).
fn push_strand(
    verts: &mut Vec<EffectVertex>,
    pts: &[Vec3],
    half_width: f32,
    u_scroll: f32,
    cam_forward: Vec3,
) {
    let count = pts.len();
    if count < 2 {
        return;
    }
    let seg_perp = |i: usize| -> Vec3 {
        let raw = (pts[i + 1] - pts[i]).cross(cam_forward);
        if raw.length() > PERP_EPSILON {
            raw.normalize()
        } else {
            raw
        }
    };
    let rail = |i: usize| -> (Vec3, Vec3, bool) {
        if i == 0 || i == count - 1 {
            (pts[i], pts[i], true)
        } else {
            let off = (seg_perp(i - 1) + seg_perp(i)) * 0.5 * half_width;
            (pts[i] + off, pts[i] - off, false)
        }
    };
    let white = [1.0, 1.0, 1.0, 1.0];
    let span = (count - 1) as f32;
    let v = |end: bool| if end { (0.5, 0.5) } else { (0.0, 1.0) };
    let mut a = rail(0);
    for i in 0..count - 1 {
        let b = rail(i + 1);
        let (ua, ub) = (i as f32 / span + u_scroll, (i + 1) as f32 / span + u_scroll);
        let ((va_t, va_b), (vb_t, vb_b)) = (v(a.2), v(b.2));
        for (pos, uv) in [
            (a.1, [ua, va_b]),
            (b.1, [ub, vb_b]),
            (b.0, [ub, vb_t]),
            (a.0, [ua, va_t]),
        ] {
            verts.push(EffectVertex {
                pos: pos.to_array(),
                uv,
                color: white,
            });
        }
        a = b;
    }
}

/// What a beam reads of each body it runs between.
type Ends<'a> = (
    &'a UnitBody,
    &'a GlobalTransform,
    Option<&'a RigPose>,
    Option<&'a InheritedVisibility>,
);

/// Where a beam meets a body this frame: the caster's hands then the height rule, or a target's
/// attachment cascade then its root. `None` while the body is not dressed or not shown.
fn end_point(end: Ends, caster: bool, dest_tag: u16) -> Option<Vec3> {
    let (body, root, pose, shown) = end;
    if shown.is_some_and(|v| !v.get()) {
        return None;
    }
    let BodyState::Ready(facts) = body.state() else {
        return None;
    };
    let base = root.translation();
    let at = |ids: &[u16]| {
        ids.iter()
            .find_map(|id| attachment_point(facts, pose, root, *id))
    };
    if caster {
        return Some(at(&CASTER_POINTS).unwrap_or_else(|| {
            let scale = root.affine().matrix3.y_axis.length();
            base + Vec3::Y * (scale * facts.height * CASTER_HEIGHT_FACTOR)
        }));
    }
    Some(at(&[dest_tag, CHAIN_ATTACH_FALLBACK]).unwrap_or(base))
}

/// Per frame: age each beam, end what is over, re-read its ends, re-jitter and advect its polylines
/// and write the ribbons to the effect lane: additive, emissive white, two-sided, no fog, no depth
/// write (`0x7afcb0`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_beams(
    mut commands: Commands,
    time: Res<Time>,
    server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    mut draw: WorldEffectDraw,
    camera: Query<(Entity, &GlobalTransform), With<WorldCamera>>,
    ends: Query<Ends>,
    mut beams: Query<(Entity, &mut ChainBeam)>,
    mut scratch: Local<Vec<Vec3>>,
) {
    let dt = time.delta_secs().min(MAX_STEP);
    let cam = camera.single().ok();
    for (entity, mut beam) in &mut beams {
        beam.age += dt;
        if beam.over() || !ends.contains(beam.nodes[0]) {
            commands.entity(entity).try_despawn();
            continue;
        }
        let (phase, u_scroll) = advance_scroll(beam.phase, dt, beam.effect.scroll_period_s);
        beam.phase = phase;
        let texture = match &beam.texture {
            Some(handle) => handle.clone(),
            None => {
                let handle: Handle<Image> =
                    server.load(texture_url(&beam.effect.texture, (true, true)));
                beam.texture = Some(handle.clone());
                handle
            }
        };
        let Some((cam, cam_xf)) = cam else {
            continue;
        };
        if !images.contains(&texture) {
            continue;
        }
        let cam_forward = *cam_xf.forward();
        let lit: Vec<bool> = (0..beam.bolts.len()).map(|i| beam.lit(i)).collect();
        let ChainBeam {
            nodes,
            bolts,
            effect,
            rng,
            dest_tag,
            ..
        } = &mut *beam;
        let mut batch = draw.batch(cam, texture.id());
        let (mut anchor_sum, mut anchor_n) = (Vec3::ZERO, 0.0f32);
        for (i, bolt) in bolts.iter_mut().enumerate() {
            let point = |n: usize| {
                ends.get(nodes[n])
                    .ok()
                    .and_then(|end| end_point(end, n == 0, *dest_tag))
            };
            let (true, Some(from), Some(to)) = (lit[i], point(i), point(i + 1)) else {
                bolt.strands.iter_mut().for_each(Vec::clear);
                continue;
            };
            anchor_sum += (from + to) * 0.5;
            anchor_n += 1.0;
            for pts in bolt.strands.iter_mut() {
                subdivide(from, to, effect, rng, &mut scratch);
                if pts.len() == scratch.len() {
                    let last = pts.len() - 1;
                    pts[0] = scratch[0];
                    pts[last] = scratch[last];
                    for k in 1..last {
                        pts[k] = pts[k] * ADVECT_KEEP + scratch[k] * (1.0 - ADVECT_KEEP);
                    }
                } else {
                    pts.clear();
                    pts.extend_from_slice(&scratch);
                }
                push_strand(
                    batch.verts_mut(),
                    pts,
                    effect.half_width,
                    u_scroll,
                    cam_forward,
                );
            }
        }
        if anchor_n == 0.0 {
            continue;
        }
        batch
            .additive()
            .anchored(anchor_sum / anchor_n)
            .owner(entity)
            .quads();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chain Lightning's row as shipped (`benilla-app`'s test fixture).
    fn lightning() -> ChainEffect {
        ChainEffect {
            avg_seg_len: 2.5,
            half_width: 0.5,
            noise_scale: 0.04,
            scroll_period_s: 0.5,
            bolt_life_ms: 300,
            bolt_stagger_ms: 100,
            texture: "Textures\\SpellChainEffects\\Lightning.blp".into(),
        }
    }

    fn chain(flag: bool, beams: u32) -> ChainProc {
        ChainProc {
            effect_id: 1,
            beams,
            flag,
            ty: if flag { 12 } else { 0 },
        }
    }

    #[test]
    fn a_cast_beam_burns_hop_by_hop_and_ends_after_its_hops() {
        let targets = [
            Entity::from_raw_u32(2).unwrap(),
            Entity::from_raw_u32(3).unwrap(),
        ];
        let caster = Entity::from_raw_u32(1).unwrap();
        let mut beam = ChainBeam::new(chain(false, 1), lightning(), caster, &targets, 15);
        assert!(!beam.held());
        assert_eq!(beam.hops(), 2);
        assert!(beam.lit(0) && !beam.lit(1));
        beam.age = 0.15;
        assert!(beam.lit(0) && beam.lit(1));
        beam.age = 0.35;
        assert!(!beam.lit(0) && beam.lit(1));
        assert!(!beam.over());
        beam.age = 0.6;
        assert!(beam.over());
    }

    #[test]
    fn a_held_beam_burns_every_hop_until_despawned() {
        let caster = Entity::from_raw_u32(1).unwrap();
        let target = [Entity::from_raw_u32(2).unwrap()];
        let mut beam = ChainBeam::new(chain(true, 9), lightning(), caster, &target, 15);
        assert!(beam.held());
        beam.age = 100.0;
        assert!(beam.lit(0) && !beam.over());
        assert_eq!(beam.bolts[0].strands.len(), 3, "clamped to CHAIN_MAX_BEAMS");
    }

    #[test]
    fn subdivision_keeps_its_ends_and_has_at_least_two_segments() {
        let mut rng = 7;
        let mut out = Vec::new();
        let (a, b) = (Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
        subdivide(a, b, &lightning(), &mut rng, &mut out);
        assert_eq!(out.len(), 7, "trunc(10 / 2.5 + 2) = 6 segments");
        assert_eq!((out[0], out[6]), (a, b));
        subdivide(a, Vec3::X * 0.1, &lightning(), &mut rng, &mut out);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn the_scroll_wraps_and_a_negative_period_flows_back() {
        assert_eq!(advance_scroll(0.0, 0.25, 0.5), (0.25, -0.5));
        let (phase, u) = advance_scroll(0.4, 0.2, 0.5);
        assert!((phase - 0.1).abs() < 1e-5 && (u + 0.2).abs() < 1e-5);
        let (_, u) = advance_scroll(0.0, 0.25, -0.5);
        assert!(u > 0.0);
        assert_eq!(advance_scroll(0.3, 0.1, 0.0), (0.0, 0.0));
    }

    #[test]
    fn a_strand_is_a_spindle_of_quads() {
        let mut verts = Vec::new();
        let pts = [Vec3::ZERO, Vec3::X, Vec3::X * 2.0];
        push_strand(&mut verts, &pts, 0.5, 0.0, Vec3::NEG_Z);
        assert_eq!(verts.len(), 8);
        // Both ends collapse to their point at v = 0.5.
        assert_eq!(verts[0].pos, [0.0, 0.0, 0.0]);
        assert_eq!(verts[0].uv[1], 0.5);
        // The middle rails sit half a width either side, in the view plane.
        let spread = (Vec3::from(verts[2].pos) - Vec3::from(verts[1].pos)).length();
        assert!((spread - 1.0).abs() < 1e-5, "{spread}");
    }
}
