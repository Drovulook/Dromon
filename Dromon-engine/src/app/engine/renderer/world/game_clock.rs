// pour plus tard: ajouter un système de tick (comme dans minecraft)

use crate::config::ClockParams;

pub struct GameClock {
    pub day: u64,             // jours écoulés depuis le début
    pub time_of_day: f64,     // fraction de journée dans [0, 1) : 0 = minuit, 0.5 = midi
    pub day_length_secs: f64, // durée d'un jour en secondes réelles (ex. 20 min = 1200)
    pub time_scale: f64,      // 1.0 = normal, 0.0 = pause, 60.0 = accéléré
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
        }
    }

    pub fn advance(&mut self, dt_real: f64) {
        self.time_of_day += dt_real * self.time_scale / self.day_length_secs;
        let whole_days = self.time_of_day.floor();
        self.day += whole_days as u64;
        self.time_of_day -= whole_days;
    }
}
