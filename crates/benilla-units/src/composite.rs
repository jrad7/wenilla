//! Composited body skins, one atlas per look, taken from `benilla-app`'s `entities/skin_composite.rs`:
//! a first composite runs off the main thread, and every body wearing a look shares its atlas. Left
//! out: the forced lane for previews, guild emblems, and sweeping the cache by distance, since a
//! program with no world streaming holds a handful of looks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use benilla_assets::{LockRecover, for_upload, repeat_texture_authored};
use benilla_formats::{
    BlpMipChain, Chain, CharSections, CompositePlan, ItemDisplay, ItemDisplayCatalog,
    NpcAppearance, blp_bytes_to_mip_chain,
};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};

/// What decides a composited skin: race and sex pick the `CharSections` rows, the dials pick the
/// variations, and `equip` holds the worn armor display ids by body slot − 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SkinKey {
    pub(crate) race: u8,
    pub(crate) sex: u8,
    pub(crate) skin: u8,
    face: u8,
    facial_hair: u8,
    hair_style: u8,
    hair_color: u8,
    equip: [u32; 8],
}

impl SkinKey {
    pub(crate) fn of(npc: &NpcAppearance) -> Self {
        let mut equip = [0; 8];
        equip.copy_from_slice(&npc.equipment[2..]);
        Self {
            race: npc.race,
            sex: npc.sex,
            skin: npc.skin,
            face: npc.face,
            facial_hair: npc.facial_hair,
            hair_style: npc.hair_style,
            hair_color: npc.hair_color,
            equip,
        }
    }
}

/// Where a body's atlas stands.
#[derive(Debug, PartialEq)]
pub(crate) enum BodyAtlas {
    /// Ready to bind; `None` for a look with no atlas (no base skin row, or art that never read).
    Ready(Option<Handle<Image>>),
    /// Its composite is running.
    Pending,
}

/// A composite's whole work, run where it lands: reads, decodes and blits.
pub(crate) type Work = Box<dyn FnOnce() -> Option<BlpMipChain> + Send>;

/// Finished atlases by look, and the composites still running.
#[derive(Resource, Default)]
pub(crate) struct SkinComposites {
    /// `None` for a look with no atlas, so it is never retried.
    done: HashMap<SkinKey, Option<Handle<Image>>>,
    /// One per look, however many bodies wait on it.
    running: HashMap<SkinKey, Task<Option<BlpMipChain>>>,
}

impl SkinComposites {
    /// A look's atlas: finished, or running, its work started here on a miss. `work` is called only
    /// then; `None` is a look with no atlas.
    pub(crate) fn request(
        &mut self,
        key: SkinKey,
        work: impl FnOnce() -> Option<Work>,
    ) -> BodyAtlas {
        if let Some(done) = self.done.get(&key) {
            return BodyAtlas::Ready(done.clone());
        }
        if self.running.contains_key(&key) {
            return BodyAtlas::Pending;
        }
        let Some(work) = work() else {
            self.done.insert(key, None);
            return BodyAtlas::Ready(None);
        };
        self.running.insert(
            key,
            AsyncComputeTaskPool::get().spawn(async move { work() }),
        );
        BodyAtlas::Pending
    }

    /// Moves every finished composite into the cache.
    fn land(&mut self, images: &mut Assets<Image>) {
        let mut finished = Vec::new();
        self.running
            .retain(|key, task| match block_on(future::poll_once(task)) {
                Some(atlas) => {
                    finished.push((*key, atlas));
                    false
                }
                None => true,
            });
        for (key, atlas) in finished {
            // Through the upload gate like every texture: a no-op on this RGBA8 composite, but it
            // keeps the format and the bytes in agreement.
            let handle =
                atlas.map(|a| images.add(repeat_texture_authored(for_upload(a), (true, true))));
            self.done.insert(key, handle);
        }
    }
}

/// What a look's composite reads, from the tables and the worn displays' rows.
pub(crate) fn composite_plan(
    key: &SkinKey,
    sections: &CharSections,
    items: Option<&ItemDisplayCatalog>,
) -> Option<CompositePlan> {
    let mut worn: [Option<&ItemDisplay>; 8] = [None; 8];
    if let Some(items) = items {
        for (slot, id) in key.equip.iter().enumerate() {
            if *id != 0 {
                worn[slot] = items.get(*id);
            }
        }
    }
    sections.composite_plan(
        key.race,
        key.sex,
        key.skin,
        key.face,
        key.facial_hair,
        key.hair_style,
        key.hair_color,
        worn,
        None,
        false,
    )
}

/// A plan's work over the shared chain, which each read locks for its archive lookup alone.
pub(crate) fn work(plan: CompositePlan, chain: Arc<Mutex<Chain>>) -> Work {
    Box::new(move || {
        plan.run(|path| read_texture(&chain, path))
            .inspect_err(|error| warn!("body skin composite failed: {error:#}"))
            .ok()
    })
}

/// Reads and decodes one BLP off `chain`, locked only to find its archive.
fn read_texture(chain: &Mutex<Chain>, path: &str) -> anyhow::Result<BlpMipChain> {
    let name = path.replace('/', "\\");
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        let archive = chain.lock_recover().archive_for(&name)?;
        archive
            .read_file(&name)
            .with_context(|| format!("reading texture '{name}'"))?
    };
    // wasm32: the chain has no archives, only the host's Data URLs.
    #[cfg(target_arch = "wasm32")]
    let bytes = chain
        .lock_recover()
        .read(&name)
        .with_context(|| format!("reading texture '{name}'"))?;
    blp_bytes_to_mip_chain(&bytes).with_context(|| format!("decoding texture '{name}'"))
}

/// Lands every finished composite before this frame's bodies ask for theirs.
pub(crate) fn land_skin_composites(
    mut composites: ResMut<SkinComposites>,
    mut images: ResMut<Assets<Image>>,
) {
    composites.land(&mut images);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    fn key(race: u8) -> SkinKey {
        let npc = NpcAppearance {
            race,
            sex: 0,
            skin: 0,
            face: 0,
            hair_style: 0,
            hair_color: 0,
            facial_hair: 0,
            equipment: [0; 10],
            bake_name: None,
        };
        SkinKey::of(&npc)
    }

    fn atlas() -> BlpMipChain {
        BlpMipChain {
            width: 1,
            height: 1,
            texels: benilla_formats::BlpTexels::Rgba8Unorm,
            mips: vec![vec![255; 4]],
        }
    }

    /// Work that finishes when the test says so and counts its runs. It gives up after two seconds,
    /// so work run inline on the test's thread fails the test, never hangs it.
    fn gated(ran: &Arc<AtomicUsize>) -> (mpsc::Sender<()>, Work) {
        let (tx, rx) = mpsc::channel::<()>();
        let ran = ran.clone();
        let work: Work = Box::new(move || {
            ran.fetch_add(1, Ordering::SeqCst);
            rx.recv_timeout(Duration::from_secs(2)).ok()?;
            Some(atlas())
        });
        (tx, work)
    }

    #[test]
    fn a_look_composites_once_off_the_main_thread() {
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let mut images = Assets::<Image>::default();
        let mut composites = SkinComposites::default();
        let ran = Arc::new(AtomicUsize::new(0));
        let (tx, first) = gated(&ran);
        let mut first = Some(first);
        assert_eq!(
            composites.request(key(1), || first.take()),
            BodyAtlas::Pending
        );
        // A second body of the same look waits on the same composite.
        assert_eq!(
            composites.request(key(1), || panic!("a running look starts no second")),
            BodyAtlas::Pending
        );
        composites.land(&mut images);
        assert_eq!(
            composites.running.len(),
            1,
            "nothing lands before it finishes"
        );

        tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while composites.running.contains_key(&key(1)) && Instant::now() < deadline {
            composites.land(&mut images);
            std::thread::yield_now();
        }
        let BodyAtlas::Ready(Some(landed)) = composites.request(key(1), || None) else {
            panic!("the landed atlas is ready");
        };
        assert_eq!(
            composites.request(key(1), || None),
            BodyAtlas::Ready(Some(landed))
        );
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_look_with_no_atlas_never_waits_and_is_not_retried() {
        let mut composites = SkinComposites::default();
        assert_eq!(composites.request(key(9), || None), BodyAtlas::Ready(None));
        assert_eq!(
            composites.request(key(9), || panic!("never retried")),
            BodyAtlas::Ready(None)
        );
    }

    // The armor columns count; the helm and the weapons never paint the skin.
    #[test]
    fn a_key_takes_body_slots_two_to_nine() {
        let npc = NpcAppearance {
            race: 1,
            sex: 1,
            skin: 2,
            face: 3,
            hair_style: 4,
            hair_color: 5,
            facial_hair: 6,
            equipment: [100, 101, 2, 3, 4, 5, 6, 7, 8, 9],
            bake_name: None,
        };
        let key = SkinKey::of(&npc);
        assert_eq!(key.equip, [2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!((key.race, key.sex, key.skin), (1, 1, 2));
    }
}
