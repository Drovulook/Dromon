//! Driver du profiling d'init : enregistre tout le déroulé (une fois), puis envoie
//! l'arbre en un message `[PROFILE_INIT]`.

use crate::app::logger::Logger;

/// À appeler avant le premier `profile!` de l'init.
pub fn begin() {
    if super::enabled() {
        super::set_recording(true);
    }
}

/// À appeler après l'init (tous les gardes droppés). Envoie l'arbre puis coupe.
pub fn flush(logger: &Logger) {
    if !super::enabled() {
        return;
    }
    let payload = super::serialize_and_clear();
    if !payload.is_empty() {
        logger.profile("INIT", &payload);
    }
    super::set_recording(false);
}
