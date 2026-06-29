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

use super::utils::smootherstep;
use noise::{NoiseFn, Perlin};

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
    pub lacunarity: f64,
    /// Multiplicateur d'amplitude entre deux octaves, dans `]0, 1[` (la
    /// « persistence » : ~0.5 donne un relief équilibré).
    pub gain: f64,
    /// Force de l'érosion `k`. `0.0` = fBm classique sans érosion ; plus grand =
    /// vallées plus creusées et crêtes plus marquées.
    pub erosion: f64,
    /// Mélange `[0, 1]` entre bruit doux et bruit « ridged ». `0.0` = collines
    /// arrondies (fBm classique) ; `1.0` = arêtes vives (`1 − |bruit|`), aspect
    /// montagne escarpée. Valeurs intermédiaires = entre les deux.
    pub ridge: f64,
    /// Aplatissement des basses terres `[0, 1]` (effet multifractal). `0.0` =
    /// même rugosité partout ; `1.0` = le détail fin et les arêtes ne sont ajoutés
    /// qu'en altitude, les vallées restent douces et peu pentues (plus réaliste).
    pub lowland_flatness: f64,
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
        }
    }
}

/// Source de bruit + paramètres. Calcule une altitude continue, fonction des
/// seules coordonnées **monde** — donc identique des deux côtés d'une couture de
/// chunk (maillage sans fissure).
pub struct HeightField {
    noise: Perlin,
    params: HeightParams,
}

impl HeightField {
    pub fn new(seed: u32, params: HeightParams) -> HeightField {
        HeightField {
            noise: Perlin::new(seed),
            params,
        }
    }

    /// Altitude continue (en voxels) à la colonne monde `(wx, wy)`.
    ///
    /// `(wx, wy)` sont des coordonnées monde **brutes** (non multipliées par la
    /// fréquence) : la mise à l'échelle est appliquée en interne, octave par
    /// octave. C'est l'unique source de vérité de la hauteur du terrain.
    pub fn height(&self, wx: f64, wy: f64) -> f64 {
        self.params.base_height + self.fbm_eroded(wx, wy) * self.params.amplitude
    }

    /// fBm érodé, normalisé dans ~`[-1, 1]`.
    fn fbm_eroded(&self, wx: f64, wy: f64) -> f64 {
        let mut freq = self.params.frequency;
        let mut amp = 1.0;
        let mut sum = 0.0; // hauteur accumulée
        let mut norm = 0.0; // somme des amplitudes (PLEINE, sans rugosité) → renorm.
        let mut grad = [0.0f64; 2]; // gradient accumulé (dérivées en x et y)
        // Rugosité de l'octave courante (effet multifractal). Pleine (1.0) pour
        // l'octave macro ; ajustée ensuite selon l'altitude de cette octave de base.
        let mut roughness = 1.0;

        for i in 0..self.params.octaves {
            // Bruit et son gradient à la fréquence courante.
            let (n, dx, dy) = self.noise_with_grad(wx * freq, wy * freq);

            // Règle de la chaîne : la dérivée par rapport au MONDE est celle par
            // rapport à l'argument du bruit, multipliée par la fréquence (car
            // l'argument vaut `w * freq`). On accumule sur toutes les octaves.
            grad[0] += dx * freq;
            grad[1] += dy * freq;

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
            let ridged = 1.0 - n.abs();
            let value = n + (ridged - n) * self.params.ridge;

            // Contribution pondérée par la rugosité (réduite en plaine). `norm`
            // accumule l'amplitude PLEINE (sans rugosité) : ainsi atténuer une
            // octave réduit réellement le relief des basses terres (plus plates),
            // au lieu d'être « rattrapé » par la normalisation.
            sum += amp * value * erode * roughness;
            norm += amp;

            // L'octave macro (i == 0) donne l'altitude grossière. On en déduit la
            // rugosité des octaves suivantes : faible en vallée (détail/pentes
            // gommés), pleine en altitude. `lowland_flatness` dose l'effet.
            if i == 0 {
                // Altitude grossière, PERTURBÉE par une octave de bruit décorrélée
                // (offset spatial) avant le seuil de rugosité. Sans cette
                // perturbation, le seuil suit une courbe de niveau — lisse et
                // régulière — de l'octave macro : la transition lisse→rugueux
                // devient une « ligne » nette dans le paysage, pile aux iso-valeurs
                // `edge0`/`edge1` du smoothstep. Le jitter déchiquette cette
                // frontière → transition naturelle, sans contour visible. (Il ne
                // touche QUE la modulation de rugosité, pas la géométrie : `sum`
                // n'en dépend pas.)
                const CONTROL_OFFSET: f64 = 2048.0;
                const CONTROL_JITTER: f64 = 0.18;
                let j = self
                    .noise
                    .get([(wx + CONTROL_OFFSET) * freq, (wy + CONTROL_OFFSET) * freq]);
                let macro_alt = (value * erode + j * CONTROL_JITTER).clamp(0.0, 1.0);
                // smootherstep (C2) plutôt que smoothstep (C1) : pas de saut de
                // courbure aux seuils → on évite aussi le liseré d'éclairage (bande
                // de Mach) qui trahissait les bornes.
                let r = smootherstep(0.25, 0.6, macro_alt);
                roughness = (1.0 - self.params.lowland_flatness) + self.params.lowland_flatness * r;
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
    pub fn material_jitter(&self, wx: f64, wy: f64) -> f64 {
        // Décalage monde qui décorrèle ce bruit de l'altitude (même source Perlin).
        const OFFSET: f64 = 4096.0;
        // Fréquence des grandes ondulations (longueur d'onde ≈ 1/FREQ voxels).
        const FREQ: f64 = 0.01;
        // Amplitude verticale de la perturbation, en voxels.
        const AMPLITUDE: f64 = 20.0;

        let x = wx + OFFSET;
        let y = wy + OFFSET;
        let macro_n = self.noise.get([x * FREQ, y * FREQ]);
        let detail_n = self.noise.get([x * FREQ * 4.0, y * FREQ * 4.0]);
        (macro_n + 0.35 * detail_n) * AMPLITUDE
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
