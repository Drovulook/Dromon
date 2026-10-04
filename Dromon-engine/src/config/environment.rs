//! `environment.ron` : le ciel, la lumière du soleil et l'écoulement du temps.

use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::ConfigFile;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvironmentConfig {
    /// Latitude, en degrés : le soleil culmine à `90° − latitude`, plein sud.
    pub latitude_deg: f32,
    pub clock: ClockParams,
    pub sun: SunDisk,
    pub day_night: DayNightParams,
    pub atmosphere: AtmosphereParams,
    pub stars: StarsParams,
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        EnvironmentConfig {
            latitude_deg: 40.0,
            clock: ClockParams::default(),
            sun: SunDisk::default(),
            day_night: DayNightParams::default(),
            atmosphere: AtmosphereParams::default(),
            stars: StarsParams::default(),
        }
    }
}

impl ConfigFile for EnvironmentConfig {
    const FILE_NAME: &'static str = "environment.ron";

    fn validate(&self) -> Result<()> {
        ensure!(
            (-90.0..=90.0).contains(&self.latitude_deg),
            "latitude_deg doit être dans [-90, 90]"
        );

        let c = &self.clock;
        ensure!(c.day_length_secs > 0.0, "clock.day_length_secs doit être > 0");
        ensure!(c.time_scale >= 0.0, "clock.time_scale doit être ≥ 0");
        ensure!(
            (0.0..24.0).contains(&c.start_hour),
            "clock.start_hour doit être dans [0, 24["
        );

        let s = &self.sun;
        ensure!(s.angular_radius_deg > 0.0, "sun.angular_radius_deg doit être > 0");
        ensure!(
            (0.0..=1.0).contains(&s.edge_softness),
            "sun.edge_softness doit être dans [0, 1]"
        );

        let d = &self.day_night;
        ensure!(!d.keyframes.is_empty(), "day_night.keyframes est vide");
        ensure!(
            d.keyframes
                .windows(2)
                .all(|k| k[0].elevation_deg < k[1].elevation_deg),
            "day_night.keyframes doivent être triées par elevation_deg strictement croissante"
        );
        ensure!(
            d.sun_fade_start_deg < d.sun_fade_end_deg,
            "day_night : il faut sun_fade_start_deg < sun_fade_end_deg"
        );
        ensure!(
            d.stars_fade_start_deg < d.stars_fade_end_deg,
            "day_night : il faut stars_fade_start_deg < stars_fade_end_deg"
        );

        let a = &self.atmosphere;
        ensure!(
            (0.0..1.0).contains(&a.fog_anisotropy),
            "atmosphere.fog_anisotropy doit être dans [0, 1["
        );
        ensure!(a.fog_scale_height > 0.0, "atmosphere.fog_scale_height doit être > 0");
        Ok(())
    }
}

/// Écoulement du temps de jeu.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClockParams {
    /// Durée d'un jour de jeu, en secondes réelles (ex. 20 min = 1200).
    pub day_length_secs: f64,
    /// 1.0 = normal, 0.0 = pause, 60.0 = accéléré.
    pub time_scale: f64,
    /// Heure de départ, dans `[0, 24[` (12 = midi).
    pub start_hour: f64,
}

impl Default for ClockParams {
    fn default() -> Self {
        ClockParams {
            day_length_secs: 30.0,
            time_scale: 1.0,
            start_hour: 9.0,
        }
    }
}

/// Apparence du disque solaire dessiné dans le ciel (sky.slang). N'influe pas sur
/// l'éclairage de la scène.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SunDisk {
    /// Rayon angulaire, en degrés. Le vrai soleil fait ~0.27° ; en jeu on prend plus
    /// gros (~1°) pour la lisibilité.
    pub angular_radius_deg: f32,
    /// Fraction du rayon où commence le fondu du bord (1 = bord net).
    pub edge_softness: f32,
    /// Multiplicateur de `color * intensity` : le disque doit éclipser le halo. Sans
    /// tonemapping, tout ce qui dépasse 1 sature en blanc.
    pub intensity: f32,
}

impl Default for SunDisk {
    fn default() -> Self {
        SunDisk {
            angular_radius_deg: 1.0,
            edge_softness: 0.85,
            intensity: 10.0,
        }
    }
}

/// Cycle jour/nuit : couleurs du ciel et de la lumière en fonction de l'**élévation**
/// du soleil (pas de l'heure : reste juste si on change la latitude ou ajoute les
/// saisons).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DayNightParams {
    /// Triées par élévation croissante ; interpolées linéairement entre deux voisines,
    /// et hors bornes on garde la keyframe extrême.
    pub keyframes: Vec<SkyKeyframe>,
    /// Élévations (degrés) entre lesquelles l'éclairage direct s'allume. Sous -3°, le
    /// disque (rayon ~1°) est entièrement caché par l'horizon : plus de lumière directe,
    /// donc plus de halo ni d'éclairage par en dessous.
    pub sun_fade_start_deg: f32,
    pub sun_fade_end_deg: f32,
    /// Élévations (degrés) du soleil entre lesquelles les étoiles s'effacent : visibles
    /// en pleine nuit, disparues avant que le ciel ne s'éclaircisse franchement.
    pub stars_fade_start_deg: f32,
    pub stars_fade_end_deg: f32,
}

impl Default for DayNightParams {
    fn default() -> Self {
        let key = |elevation_deg, horizon, zenith, sun_color, ambient| SkyKeyframe {
            elevation_deg,
            horizon: Vec3::from_array(horizon),
            zenith: Vec3::from_array(zenith),
            sun_color: Vec3::from_array(sun_color),
            ambient: Vec3::from_array(ambient),
        };
        DayNightParams {
            keyframes: vec![
                // nuit noire (fin du crépuscule astronomique)
                key(-18.0, [0.010, 0.012, 0.025], [0.003, 0.005, 0.015], [1.0, 0.35, 0.10], [0.02, 0.025, 0.045]),
                // crépuscule civil : lueur violacée à l'horizon
                key(-6.0, [0.22, 0.13, 0.17], [0.03, 0.05, 0.14], [1.0, 0.35, 0.10], [0.07, 0.07, 0.11]),
                // coucher / lever
                key(0.0, [0.85, 0.45, 0.22], [0.12, 0.18, 0.42], [1.0, 0.45, 0.15], [0.17, 0.16, 0.20]),
                // heure dorée
                key(8.0, [0.72, 0.65, 0.62], [0.10, 0.23, 0.58], [1.0, 0.72, 0.45], [0.24, 0.26, 0.33]),
                // après-midi
                key(25.0, [0.55, 0.70, 0.88], [0.10, 0.25, 0.65], [1.0, 0.85, 0.65], [0.28, 0.32, 0.40]),
                // soleil haut : lumière plus blanche
                key(50.0, [0.55, 0.70, 0.90], [0.09, 0.24, 0.66], [1.0, 0.93, 0.83], [0.29, 0.33, 0.41]),
            ],
            sun_fade_start_deg: -7.0,
            sun_fade_end_deg: 1.0,
            stars_fade_start_deg: -14.0,
            stars_fade_end_deg: -5.0,
        }
    }
}

/// État du ciel pour une élévation solaire donnée. Couleurs linéaires.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyKeyframe {
    pub elevation_deg: f32,
    /// Couleur du ciel à l'horizon, aussi celle du brouillard.
    pub horizon: Vec3,
    pub zenith: Vec3,
    pub sun_color: Vec3,
    /// Lumière indirecte du ciel (ambiant du terrain).
    pub ambient: Vec3,
}

/// Réglages fixes de l'atmosphère. Ses couleurs viennent des keyframes jour/nuit.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AtmosphereParams {
    /// Part de l'ambiant renvoyée par le sol (∈ [0, 1]) : une face tournée vers le bas
    /// reçoit `ambient * ground_bounce`. Plus c'est bas, plus le relief ressort.
    pub ground_bounce: f32,
    /// Forme du dégradé horizon → zénith : `pow(élévation du regard, exposant)`.
    /// < 1 = transition resserrée près de l'horizon, comme un vrai ciel.
    pub sky_gradient_exponent: f32,
    /// σ₀ de la loi de Beer-Lambert (densité du brouillard au niveau 0).
    pub fog_density: f32,
    /// H — hauteur d'échelle : altitude à laquelle la densité est divisée par e.
    /// Il faut H ≈ 3 × l'étendue verticale du terrain.
    pub fog_scale_height: f32,
    /// g de Henyey-Greenstein ∈ [0, 1[ : plus proche de 1 = halo plus concentré autour
    /// du soleil, mais pic beaucoup plus intense (compenser avec `halo_strength`).
    pub fog_anisotropy: f32,
    /// Intensité du halo solaire, dans le ciel ET dans le brouillard du terrain.
    pub halo_strength: f32,
}

impl Default for AtmosphereParams {
    fn default() -> Self {
        AtmosphereParams {
            ground_bounce: 0.25,
            sky_gradient_exponent: 0.75,
            fog_density: 0.0005,
            fog_scale_height: 1200.0,
            fog_anisotropy: 0.9,
            halo_strength: 0.003,
        }
    }
}

/// Étoiles procédurales (sky.slang).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StarsParams {
    /// Luminosité (linéaire) des étoiles les plus brillantes.
    pub brightness: f32,
    /// Fraction des cases du ciel qui contiennent une étoile (grille : `STAR_CELLS`
    /// dans sky.slang).
    pub density: f32,
    /// Amplitude de la scintillation ∈ [0, 1] (doublée près de l'horizon).
    pub twinkle: f32,
}

impl Default for StarsParams {
    fn default() -> Self {
        StarsParams {
            brightness: 1.0,
            density: 0.03,
            twinkle: 0.25,
        }
    }
}
