//! Hot-swap of modules in the running client.
//!
//! `modules.version.available` → (for this user) → quiesce (wait for the module's running actions, max 10 s)
//! → load and verify the new version → unregister the old registrations → register the new ones → views
//! rebuild themselves from the new pack and keep their state (search, filters, selection). If loading fails,
//! the old version stays registered (rollback). Modules with `hotSwap: restartRequired` ask for a restart.

use crate::context::{AppContext, Paths};
use crate::modules::{self, LoadResult, LoadedModule};
use crate::runtime::Promise;
use redhive_contracts::modules::{HotSwapMode, RolloutStage, VersionAvailable};
use redhive_monads::Res;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum SwapOutcome {
    Swapped { module_id: String, from: String, to: String, rollback: bool },
    RestartRequired { module_id: String, version: String },
    Failed { module_id: String, error: redhive_monads::Error },
}

struct InProgress {
    notice: VersionAvailable,
    since: Instant,
    load: Option<Promise<Res<LoadResult>>>,
}

#[derive(Default)]
pub struct HotSwapManager {
    queue: VecDeque<VersionAvailable>,
    current: Option<InProgress>,
    pub history: Vec<(chrono::DateTime<chrono::Local>, String)>,
}

impl HotSwapManager {
    /// A version notice arrived; keeps it when it concerns this user.
    pub fn notice(&mut self, n: VersionAvailable, me: Uuid) {
        if n.users.is_empty() || n.users.contains(&me) {
            self.queue.retain(|q| q.module_id != n.module_id);
            self.queue.push_back(n);
        }
    }

    /// Requests a re-check of all modules ("apply updates").
    pub fn pending(&self) -> usize {
        self.queue.len() + usize::from(self.current.is_some())
    }

    pub fn tick(&mut self, ctx: &mut AppContext, paths: &Paths, pinned: Option<String>, loaded: &mut BTreeMap<String, LoadedModule>, busy: &dyn Fn(&str) -> bool) -> Vec<SwapOutcome> {
        let mut out = Vec::new();
        if self.current.is_none() {
            let Some(n) = self.queue.pop_front() else { return out };
            if loaded.get(&n.module_id).is_some_and(|m| m.manifest.version == n.version) {
                return out;
            }
            if n.hot_swap == HotSwapMode::RestartRequired && loaded.contains_key(&n.module_id) {
                out.push(SwapOutcome::RestartRequired { module_id: n.module_id.clone(), version: n.version.clone() });
                return out;
            }
            ctx.log.info("hotswap", format!("{} {} – quiescing", n.module_id, n.version));
            self.current = Some(InProgress { notice: n, since: Instant::now(), load: None });
        }
        let Some(cur) = self.current.as_mut() else { return out };
        // 1) quiesce: running actions of the module finish first (max 10 s)
        if cur.load.is_none() {
            if busy(&cur.notice.module_id) && cur.since.elapsed() < Duration::from_secs(10) {
                return out;
            }
            let api = ctx.api.clone();
            let paths = paths.clone();
            let module = cur.notice.module_id.clone();
            cur.load = Some(Promise::spawn(async move { modules::load_assigned(&api, &paths, pinned, Some(&module)).await }));
            return out;
        }
        // 2) load + verify the new version
        let Some(result) = cur.load.as_ref().and_then(|p| p.take()) else { return out };
        let notice = self.current.take().map(|c| c.notice).unwrap_or_else(|| unreachable!());
        let id = notice.module_id.clone();
        match result {
            Ok(r) => match r.modules.into_iter().find(|m| m.manifest.module_id == id) {
                Some(new) => {
                    // 3) switch registrations atomically (within this frame)
                    let from = loaded.get(&id).map(|m| m.manifest.version.clone()).unwrap_or_default();
                    modules::unregister(ctx, &id);
                    modules::register(ctx, &new);
                    let to = new.manifest.version.clone();
                    loaded.insert(id.clone(), new);
                    self.history.push((chrono::Local::now(), format!("{id}: {from} ⏵ {to}")));
                    out.push(SwapOutcome::Swapped { module_id: id, from, to, rollback: notice.stage == RolloutStage::Cancelled });
                }
                None => {
                    let error = r.rejected.into_iter().find(|(m, _)| m == &id).map(|(_, e)| e).unwrap_or_else(|| redhive_monads::Error::not_found("modules.package.missing", "modules:package.missing"));
                    out.push(SwapOutcome::Failed { module_id: id, error });
                }
            },
            Err(error) => out.push(SwapOutcome::Failed { module_id: id, error }),
        }
        out
    }
}
