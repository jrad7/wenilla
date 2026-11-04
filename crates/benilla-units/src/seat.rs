//! Hanging one body from another: a worn item or an aura follows a bone of its wearer's.

use benilla_world::rig_anim::RigPose;
use bevy::prelude::*;

use crate::body::{BodyState, UnitBody};

/// Waits for `wearer` to be dressed, then seats this entity at the first attachment in `points` the
/// wearer's model authors, or at its root if it authors none. A wearer that failed or is gone takes
/// this entity with it. Dressing waits for the seat, so nothing shows at the origin meanwhile.
#[derive(Component, Clone, Debug)]
pub struct SeatOn {
    pub wearer: Entity,
    /// `M2` attachment ids, best first.
    pub points: Vec<u16>,
}

pub(crate) fn seat_riders(
    mut commands: Commands,
    riders: Query<(Entity, &SeatOn)>,
    mut wearers: Query<(&UnitBody, Option<&mut RigPose>)>,
) {
    for (rider, seat) in &riders {
        let Ok((body, pose)) = wearers.get_mut(seat.wearer) else {
            commands.entity(rider).despawn();
            continue;
        };
        let facts = match body.state() {
            BodyState::Loading => continue,
            BodyState::Failed(_) => {
                commands.entity(rider).despawn();
                continue;
            }
            BodyState::Ready(facts) => facts,
        };
        let authored = seat
            .points
            .iter()
            .find_map(|id| facts.attachments.iter().find(|a| a.id == *id));
        let joint = authored.zip(pose).and_then(|(attachment, mut pose)| {
            let joint = pose.anchor_for(&mut commands, seat.wearer, attachment.bone)?;
            Some((joint, attachment.offset))
        });
        let (parent, offset) = joint.unwrap_or((seat.wearer, Vec3::ZERO));
        commands
            .entity(rider)
            .insert((Transform::from_translation(offset), ChildOf(parent)))
            .remove::<SeatOn>();
    }
}
