// pour plus tard: ajouter un système de tick (comme dans minecraft)

pub struct GameClock {
    pub day: u64,             // jours écoulés depuis le début
    pub time_of_day: f64,     // fraction de journée dans [0, 1) : 0 = minuit, 0.5 = midi
    pub day_length_secs: f64, // durée d'un jour en secondes réelles (ex. 20 min = 1200)
    pub time_scale: f64,      // 1.0 = normal, 0.0 = pause, 60.0 = accéléré
}

impl GameClock {
    pub fn advance(&mut self, dt_real: f64) {
        self.time_of_day += dt_real * self.time_scale / self.day_length_secs;
        let whole_days = self.time_of_day.floor();
        self.day += whole_days as u64;
        self.time_of_day -= whole_days;
    }
}

impl Default for GameClock {
    fn default() -> Self {
        Self {
            day: 0,
            time_of_day: 12.0 / 24.0,
            day_length_secs: 30.0,
            time_scale: 1.0,
        }
    }
}
