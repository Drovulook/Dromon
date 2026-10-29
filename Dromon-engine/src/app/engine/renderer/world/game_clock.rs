// pour plus tard: ajouter un système de tick (comme dans minecraft)

use crate::config::ClockParams;

/// Seule source du temps de simulation : jour/nuit et animations (vagues, scintillement)
/// suivent la même vitesse et la même pause.
pub struct GameClock {
    pub day: u64,             // jours écoulés depuis le début
    pub time_of_day: f64,     // fraction de journée dans [0, 1) : 0 = minuit, 0.5 = midi
    pub day_length_secs: f64, // durée d'un jour en secondes réelles (ex. 20 min = 1200)
    pub time_scale: f64,      // 1.0 = normal, 0.0 = pause, 60.0 = accéléré
    /// Pause (commande). À part de `time_scale` pour que la reprise retrouve la vitesse
    /// d'avant, et qu'un hot reload de `time_scale` ne la lève pas.
    pub paused: bool,
    /// Temps de simulation écoulé (s), lu par les animations des shaders.
    pub sim_secs: f64,
}

impl GameClock {
    /// Horloge au jour 0, à `start_hour`. Durée du jour et vitesse restent modifiables
    /// en jeu (pause, accéléré).
    pub fn new(params: &ClockParams) -> GameClock {
        GameClock {
            day: 0,
            time_of_day: params.start_hour / 24.0,
            day_length_secs: params.day_length_secs,
            time_scale: params.time_scale,
            paused: false,
            sim_secs: 0.0,
        }
    }

    /// Hot reload : durée du jour et vitesse prises telles quelles ; une heure de départ
    /// modifiée fait sauter l'horloge à cette heure. La pause est gardée.
    pub fn hot_reload(&mut self, old: &ClockParams, new: &ClockParams) {
        self.day_length_secs = new.day_length_secs;
        self.time_scale = new.time_scale;
        if new.start_hour != old.start_hour {
            self.time_of_day = new.start_hour / 24.0;
        }
    }

    pub fn advance(&mut self, dt_real: f64) {
        if self.paused {
            return;
        }
        let dt = dt_real * self.time_scale;
        self.sim_secs += dt;
        self.time_of_day += dt / self.day_length_secs;
        let whole_days = self.time_of_day.floor();
        self.day += whole_days as u64;
        self.time_of_day -= whole_days;
    }

    /// Heure courante, dans [0, 24).
    pub fn hour(&self) -> f64 {
        self.time_of_day * 24.0
    }

    /// Saute à `hour` le même jour ; hors de [0, 24) → refusé.
    pub fn set_hour(&mut self, hour: f64) -> Result<(), String> {
        if !(0.0..24.0).contains(&hour) {
            return Err(format!("heure {hour} hors de [0, 24)"));
        }
        self.time_of_day = hour / 24.0;
        Ok(())
    }
}
