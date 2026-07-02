//! Driver du profiling par frame, **échantillonné** : on ne prélève qu'une frame
//! toutes les ~500 ms (sinon `profile!` retombe sur un test de flag, quasi gratuit).
//! La frame prélevée est envoyée en un message `[PROFILE_FRAME]`.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::app::logger::Logger;

const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

thread_local! {
    static LAST_FLUSH: Cell<Option<Instant>> = const { Cell::new(None) };
    static SAMPLING: Cell<bool> = const { Cell::new(false) }; // frame courante prélevée ?
}

/// À appeler avant le premier `profile!` de la frame. Décide du prélèvement (throttle).
pub fn begin() {
    if !super::enabled() {
        return;
    }
    let now = Instant::now();
    let due = LAST_FLUSH.with(|l| match l.get() {
        None => true,
        Some(t) => now.duration_since(t) >= FLUSH_INTERVAL,
    });
    SAMPLING.with(|s| s.set(due));
    super::set_recording(due);
}

/// À appeler en fin de frame. Si prélevée : envoie l'arbre, réarme le throttle, coupe.
pub fn end(logger: &Logger) {
    if !super::enabled() || !SAMPLING.with(|s| s.get()) {
        return;
    }
    let payload = super::serialize_and_clear();
    if !payload.is_empty() {
        logger.profile("FRAME", &payload);
    }
    LAST_FLUSH.with(|l| l.set(Some(Instant::now())));
    SAMPLING.with(|s| s.set(false));
    super::set_recording(false);
}
