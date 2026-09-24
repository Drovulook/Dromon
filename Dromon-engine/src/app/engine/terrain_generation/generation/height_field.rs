//! Calcul de l'altitude du terrain : fBm (fractal Brownian motion) avec
//! **érosion par accumulation de gradient**.
//!
//! ## Principe
//! Un fBm classique empile des octaves de bruit de Perlin : à chaque octave la
//! fréquence est multipliée par `lacunarity` (> 1, détails plus fins) et
//! l'amplitude par `gain` (< 1, contribution décroissante). On somme le tout.
//!
//! L'**érosion** (technique d'Inigo Quilez, cf. <https://iquilezles.org/articles/morenoise/>)
//! ajoute une boucle de rétroaction : on accumule le **gradient** de la surface
//! octave après octave, et on atténue la contribution de chaque nouvelle octave
//! par `1 / (1 + k·‖grad‖)`. Là où la pente est déjà raide (gradient accumulé
//! grand), les détails fins ne s'ajoutent quasiment plus → vallées creusées et
//! crêtes nettes, comme une érosion hydraulique très simplifiée.
//!
//! ## Gradient
//! Le crate `noise` n'expose pas les dérivées analytiques du Perlin ; on les
//! estime par **différences finies** (forward). C'est ~3× plus d'évaluations par
//! octave qu'un fBm nu, mais ça marche avec n'importe quelle source de bruit. Le
//! jour où l'on voudra un bruit dérivable analytiquement, seul
//! [`HeightField::noise_with_grad`] sera à remplacer.
//!
//! ## Non-stationnarité
//! Un fBm seul a les mêmes statistiques partout (même relief à l'infini). Deux
//! ajouts, appliqués dans [`HeightField::height`] :
//! - **carte de massifs** : bruit très basse fréquence, tranché par `smoothstep`,
//!   qui module l'amplitude (plaine ↔ massif) ;
//! - **domain warping** : l'octave macro (et la carte de massifs) est évaluée en
//!   `p + w(p)`, `w` étant un petit champ de déplacement bruité → chaînes pliées
//!   et allongées, vallées qui serpentent (cf. <https://iquilezles.org/articles/warp/>).
//!   Les octaves fines restent non déformées pour garder un grain isotrope.

use noise::{NoiseFn, SuperSimplex};

use crate::app::engine::terrain_generation::utils::{smootherstep, smoothstep};

/// Paramètres du champ d'altitude. Regroupe le contrôle du fBm et de l'érosion.
#[derive(Clone, Copy)]
pub struct HeightParams {
    /// Altitude moyenne du terrain, en voxels (le bruit oscille autour).
    pub base_height: f64,
    /// Amplitude verticale totale du relief, en voxels.
    pub amplitude: f64,
    /// Fréquence de la **première** octave : plus petit = collines plus larges.
    pub frequency: f64,
    /// Nombre d'octaves (couches de détail superposées).
    pub octaves: usize,
    /// Multiplicateur de fréquence entre deux octaves (typiquement ~2.0).
    /// !!! ATTENTION : `lacunarity` doit être non-entière pour éviter un alignement des octaves
    pub lacunarity: f64,
    /// Multiplicateur d'amplitude entre deux octaves, dans `]0, 1[` (la
    /// « persistence » : ~0.5 donne un relief équilibré).
    pub gain: f64,
    /// Force de l'érosion `k`. `0.0` = fBm classique sans érosion ; plus grand =
    /// vallées plus creusées et crêtes plus marquées. Indépendante de `frequency`
    /// et de `amplitude` (gradient pris dans l'espace du bruit).
    pub erosion: f64,
    /// Mélange `[0, 1]` entre bruit doux et bruit « ridged ». `0.0` = collines
    /// arrondies (fBm classique) ; `1.0` = arêtes vives (`1 − |bruit|`), aspect
    /// montagne escarpée. Valeurs intermédiaires = entre les deux.
    pub ridge: f64,
    /// Aplatissement des basses terres `[0, 1]` (effet multifractal). `0.0` =
    /// même rugosité partout ; `1.0` = le détail fin et les arêtes ne sont ajoutés
    /// qu'en altitude, les vallées restent douces et peu pentues (plus réaliste).
    pub lowland_flatness: f64,
    /// Modulation du `ridge` par l'altitude `[0, 1]`. `0.0` = même mélange partout ;
    /// `1.0` = arêtes vives réservées aux hauteurs, vallées entièrement arrondies.
    /// L'amplitude du relief, elle, ne bouge pas (cf. [`value_std`]).
    pub ridge_altitude: f64,
    /// Arrondi des arêtes ridged, en unités de bruit (`0.0` = arête vive). Largeur
    /// en voxels = `ridge_smoothness / freq` : large aux grandes octaves (plus
    /// d'arête-lame kilométrique), négligeable aux fines. ~0.05.
    pub ridge_smoothness: f64,

    /// Fréquence de la carte de massifs. Longueur d'onde (`1/f`) de quelques fois
    /// celle de l'octave de base, mais inférieure à la taille du monde.
    pub massif_frequency: f64,
    /// Bande `smoothstep` appliquée au bruit de massifs (∈ ~`[-1, 1]`) : sous
    /// `massif_low` plaine, au-dessus de `massif_high` massif. Étroite = régions
    /// tranchées ; large = gradient mou.
    pub massif_low: f64,
    pub massif_high: f64,
    /// Facteur d'amplitude en plaine `[0, 1]`. `1.0` = carte de massifs désactivée.
    pub massif_min: f64,
    /// Fréquence du champ de déplacement du domain warping (~`frequency`).
    pub warp_frequency: f64,
    /// Déplacement maximal en voxels. `0.0` = warping désactivé. Trop fort (≫ la
    /// longueur d'onde de base) → aspect marbre tourbillonnant.
    pub warp_amplitude: f64,
}

impl Default for HeightParams {
    fn default() -> Self {
        HeightParams {
            base_height: 64.0,
            amplitude: 24.0,
            frequency: 0.02,
            octaves: 5,
            lacunarity: 2.5,
            gain: 0.5,
            erosion: 1.0,
            ridge: 0.0,
            lowland_flatness: 0.0,
            ridge_altitude: 0.0,
            ridge_smoothness: 0.0,
            massif_frequency: 0.002,
            massif_low: -0.1,
            massif_high: 0.2,
            massif_min: 1.0,
            warp_frequency: 0.02,
            warp_amplitude: 0.0,
        }
    }
}

// Décalage propre à chaque octave : les réseaux ne coïncident plus.
const OCT_OFF: [f64; 8] = [0.0, 137.3, 411.9, 79.1, 263.5, 521.7, 191.3, 347.9];
// Décalages (espace bruit) des champs auxiliaires, décorrélés des octaves.
const MASSIF_OFF: f64 = 877.3;
const WARP_OFF_X: f64 = 311.7;
const WARP_OFF_Y: f64 = 653.1;

/// Écart-type de `value = (1−r)·n + r·(1−|n|)`, en forme fermée :
/// `Cov[n, |n|] = 0` (fonction impaire, distribution symétrique) → les variances
/// s'additionnent. Constantes mesurées sur SuperSimplex 2D.
fn value_std(r: f64) -> f64 {
    const SD_N: f64 = 0.4196; // écart-type de n
    const SD_A: f64 = 0.2227; // écart-type de |n|
    (((1.0 - r) * SD_N).powi(2) + (r * SD_A).powi(2)).sqrt()
}

/// Moyenne de `value`. `E[n] = 0` → seule la part ridged compte, linéaire en `r`.
fn value_mean(r: f64) -> f64 {
    const MEAN_A: f64 = 0.6444; // E[1 − |n|]
    MEAN_A * r
}

/// Source de bruit + paramètres. Calcule une altitude continue, fonction des
/// seules coordonnées **monde** — donc identique des deux côtés d'une couture de
/// chunk (maillage sans fissure).
pub struct HeightField {
    noise: SuperSimplex,
    params: HeightParams,
}

impl HeightField {
    pub fn new(seed: u32, params: HeightParams) -> HeightField {
        HeightField {
            noise: SuperSimplex::new(seed),
            params,
        }
    }

    /// Altitude continue (en voxels) à la colonne monde `(wx, wy)`.
    ///
    /// `(wx, wy)` sont des coordonnées monde **brutes** (non multipliées par la
    /// fréquence) : la mise à l'échelle est appliquée en interne, octave par
    /// octave. C'est l'unique source de vérité de la hauteur du terrain.
    pub fn height(&self, wx: f64, wy: f64) -> f64 {
        // profile!(); pas possible (rayon)
        let warped = self.warp(wx, wy);
        let m = self.massif(warped.0, warped.1);
        let min = self.params.massif_min;
        let amp_factor = min + (1.0 - min) * m;
        self.params.base_height
            + self.fbm_eroded(wx, wy, warped, m) * self.params.amplitude * amp_factor
    }

    /// Coordonnées déformées `p + w(p)` (domain warping). Deux bruits décorrélés
    /// donnent le déplacement en x et en y.
    fn warp(&self, wx: f64, wy: f64) -> (f64, f64) {
        let a = self.params.warp_amplitude;
        if a == 0.0 {
            return (wx, wy);
        }
        let f = self.params.warp_frequency;
        let dx = self.noise.get([wx * f + WARP_OFF_X, wy * f + WARP_OFF_X]);
        let dy = self.noise.get([wx * f + WARP_OFF_Y, wy * f + WARP_OFF_Y]);
        (wx + dx * a, wy + dy * a)
    }

    /// Carte de massifs `m ∈ [0, 1]` : 0 = plaine, 1 = massif (1 si désactivée).
    fn massif(&self, wx: f64, wy: f64) -> f64 {
        if self.params.massif_min >= 1.0 {
            return 1.0;
        }
        let f = self.params.massif_frequency;
        let n = self.noise.get([wx * f + MASSIF_OFF, wy * f + MASSIF_OFF]);
        smoothstep(self.params.massif_low, self.params.massif_high, n)
    }

    /// Cosinus de la pente **moyenne** du relief sur un voisinage de rayon `r` autour
    /// de `(wx, wy)`, par différences centrées d'écart `2r`. Une différence de hauteurs
    /// sur `[x−r, x+r]` vaut la moyenne de la pente locale sur ce segment : les bosses
    /// plus étroites que `2r` s'annulent, seules les grandes pentes restent.
    pub fn macro_up(&self, wx: f64, wy: f64, r: f64) -> f64 {
        let gx = (self.height(wx + r, wy) - self.height(wx - r, wy)) / (2.0 * r);
        let gy = (self.height(wx, wy + r) - self.height(wx, wy - r)) / (2.0 * r);
        // ‖∇h‖ = tan θ → cos θ = 1 / √(1 + tan² θ), comparable à `normal.z`.
        1.0 / (1.0 + gx * gx + gy * gy).sqrt()
    }

    /// fBm érodé, normalisé dans ~`[-1, 1]`. L'octave macro est évaluée en `warped`
    /// (forme des chaînes pliée), les suivantes en `(wx, wy)` : warper les octaves
    /// fines les étire en stries parallèles. `massif` (carte de massifs) plafonne
    /// `alt` : en plaine, pas de rugosité ni d'arêtes de haute montagne.
    fn fbm_eroded(&self, wx: f64, wy: f64, warped: (f64, f64), massif: f64) -> f64 {
        let mut freq = self.params.frequency;
        let mut amp = 1.0;
        let mut sum = 0.0; // hauteur accumulée
        let mut norm = 0.0; // somme des amplitudes (PLEINE, sans rugosité) → renorm.
        let mut grad = [0.0f64; 2]; // gradient accumulé (dérivées en x et y)
        // Rugosité de l'octave courante (effet multifractal). Pleine (1.0) pour
        // l'octave macro ; ajustée ensuite selon l'altitude de cette octave de base.
        let mut roughness = 1.0;
        // Facteur d'altitude ∈ [0,1] déduit de l'octave macro : 0 en vallée, 1 en
        // hauteur. Pilote `roughness` ET le mélange ridged.
        let mut alt = 1.0;

        for i in 0..self.params.octaves {
            // Bruit et son gradient à la fréquence courante.
            let o = OCT_OFF[i % 8];
            let (px, py) = if i == 0 { warped } else { (wx, wy) };
            let (n, dx, dy) = self.noise_with_grad(px * freq + o, py * freq + o * 1.7);

            // Gradient dans l'espace du BRUIT (sans `× freq`), comme chez Quilez :
            // chaque octave pèse O(1), quelle que soit l'échelle du monde. La dérivée
            // monde rendait `erosion` dépendante de `frequency` (quasi nulle aux
            // basses fréquences).
            grad[0] += dx;
            grad[1] += dy;

            // Atténuation : ‖grad‖ grand (pente déjà raide) → facteur proche de 0,
            // l'octave n'ajoute presque rien. C'est le cœur de l'érosion.
            let slope = (grad[0] * grad[0] + grad[1] * grad[1]).sqrt();
            let erode = 1.0 / (1.0 + self.params.erosion * slope);

            // Version « ridged » : `1 − |n|` ∈ [0, 1], avec une arête vive (cusp)
            // là où le bruit s'annule → crêtes montagneuses. Gardée dans [0, 1]
            // (et non remappée en [-1, 1]) : la contribution est toujours positive,
            // donc le terrain ne fait que MONTER depuis le plancher de vallée
            // (`base_height`) — pas de creux négatifs qui passeraient sous z=0.
            // On interpole entre bruit doux (`n`) et ridged selon `ridge`.
            // `|n|` adouci (`√(n² + ε²) − ε`) : arrondit l'arête sur ~ε en unités de
            // bruit, donc proportionnellement à la longueur d'onde de l'octave.
            let eps = self.params.ridge_smoothness;
            let ridged = 1.0 - ((n * n + eps * eps).sqrt() - eps);
            // L'octave macro (i == 0) donne l'altitude grossière, d'où l'on tire
            // `alt`. Calculée sur le mélange PLEIN : la valeur modulée dépend de
            // `alt`, on tournerait en rond.
            if i == 0 {
                // Altitude grossière, PERTURBÉE par une octave de bruit décorrélée
                // (offset spatial) avant le seuil de rugosité. Sans cette
                // perturbation, le seuil suit une courbe de niveau — lisse et
                // régulière — de l'octave macro : la transition lisse→rugueux
                // devient une « ligne » nette dans le paysage, pile aux iso-valeurs
                // `edge0`/`edge1` du smoothstep. Le jitter déchiquette cette
                // frontière → transition naturelle, sans contour visible. (Il ne
                // touche QUE la modulation, pas la géométrie : `sum` n'en dépend pas.)
                const CONTROL_OFFSET: f64 = 2048.0;
                const CONTROL_JITTER: f64 = 0.18;
                let j = self
                    .noise
                    .get([(wx + CONTROL_OFFSET) * freq, (wy + CONTROL_OFFSET) * freq]);
                let nominal = n + (ridged - n) * self.params.ridge;
                let macro_alt = (nominal * erode + j * CONTROL_JITTER).clamp(0.0, 1.0);
                // smootherstep (C2) plutôt que smoothstep (C1) : pas de saut de
                // courbure aux seuils → on évite aussi le liseré d'éclairage (bande
                // de Mach) qui trahissait les bornes.
                // × massif : `macro_alt` ignore la baisse d'amplitude en plaine, une
                // colline s'y croirait en haute montagne.
                alt = smootherstep(0.25, 0.6, macro_alt) * massif;
            }

            // Mélange EFFECTIF : `ridge` plein en altitude, arrondi en vallée. Même
            // valeur à toutes les octaves — c'est l'altitude, et non l'échelle, qui
            // décide de l'angularité.
            let r_eff = self.params.ridge
                * ((1.0 - self.params.ridge_altitude) + self.params.ridge_altitude * alt);
            let raw = n + (ridged - n) * r_eff;

            // Renormalisation : `raw` reprend la moyenne et l'écart-type qu'il aurait
            // eus avec `ridge` plein, donc la modulation change la FORME sans toucher
            // à l'amplitude. ⚠ sans ça, arrondir les vallées y AUGMENTERAIT le relief
            // (|n| varie deux fois moins que n) et toute la calibration dériverait.
            let value = (raw - value_mean(r_eff))
                * (value_std(self.params.ridge) / value_std(r_eff))
                + value_mean(self.params.ridge);

            // Contribution pondérée par la rugosité (réduite en plaine). `norm`
            // accumule l'amplitude PLEINE (sans rugosité) : ainsi atténuer une
            // octave réduit réellement le relief des basses terres (plus plates),
            // au lieu d'être « rattrapé » par la normalisation.
            sum += amp * value * erode * roughness;
            norm += amp;

            // Rugosité des octaves suivantes : faible en vallée (détail et pentes
            // gommés), pleine en altitude. `lowland_flatness` dose l'effet.
            if i == 0 {
                roughness =
                    (1.0 - self.params.lowland_flatness) + self.params.lowland_flatness * alt;
            }

            amp *= self.params.gain;
            freq *= self.params.lacunarity;
        }

        // Renormalise : sans érosion la somme des `amp` ramènerait dans [-1, 1].
        // (Avec érosion le résultat est plus petit, donc toujours borné.)
        if norm > 0.0 { sum / norm } else { 0.0 }
    }

    /// Perturbation d'altitude (en voxels) à AJOUTER aux seuils de matériau de
    /// surface (sable/herbe/neige) pour casser leurs frontières rectilignes.
    ///
    /// C'est un bruit de Perlin propre, fonction des seules coordonnées **monde**
    /// (donc continu et sans couture entre chunks, comme [`HeightField::height`]),
    /// mais **décorrélé du relief** par un grand décalage spatial : il ne suit pas
    /// les bosses du terrain, il ondule indépendamment. Deux octaves : une basse
    /// fréquence (grandes ondulations de la frontière) + une plus fine (dentelure
    /// de bord), ce qui donne un contour naturel plutôt qu'une simple vague molle.
    ///
    /// On perturbe ainsi l'altitude *testée* (et non le seuil) : la frontière
    /// devient l'iso-courbe de `surface + jitter` au lieu de `surface = seuil`.
    pub fn material_jitter(&self, wx: f64, wy: f64, amp: f64) -> f64 {
        // Décalage monde qui décorrèle ce bruit de l'altitude (même source Perlin).
        const OFFSET: f64 = 4096.0;
        // Fréquence des grandes ondulations (longueur d'onde ≈ 1/FREQ voxels).
        const FREQ: f64 = 0.01;

        let x = wx + OFFSET;
        let y = wy + OFFSET;
        let macro_n = self.noise.get([x * FREQ, y * FREQ]);
        let detail_n = self.noise.get([x * FREQ * 4.0, y * FREQ * 4.0]);
        (macro_n + 0.35 * detail_n) * amp
    }

    /// Valeur du bruit + gradient `(∂/∂x, ∂/∂y)` en `(x, y)` (espace bruit), par
    /// différences finies forward. `EPS` est un compromis : trop petit → erreurs
    /// d'arrondi flottant, trop grand → gradient « moyenné » qui lisse les détails.
    fn noise_with_grad(&self, x: f64, y: f64) -> (f64, f64, f64) {
        const EPS: f64 = 1e-3;
        let n = self.noise.get([x, y]);
        let nx = self.noise.get([x + EPS, y]);
        let ny = self.noise.get([x, y + EPS]);
        (n, (nx - n) / EPS, (ny - n) / EPS)
    }
}
