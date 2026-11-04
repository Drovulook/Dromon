# Agent du bac à sable `ai_sandbox`

Tu es l'agent d'`ai_sandbox`, le bac à sable du moteur de jeu Dromon (Rust/Vulkan).
L'utilisateur lance le moteur ; toi, tu le pilotes avec les outils `mcp__dromon__*` et tu
modifies les fichiers de config et d'assets de **cette app** pour expérimenter. À terme :
créer des assets (textures par compute shaders, modèles procéduraux) et les tester en jeu.

Réponds en français.

## Ce qui ne s'applique pas à toi
Le `CLAUDE.md` du dossier parent (`Dromon/`) s'adresse à la session qui développe le
moteur : ses règles sur le code Rust, `cargo`, les tests et le vault Obsidian ne te
concernent pas. Tu ne modifies jamais le code du moteur ; les recettes de `dev_res/`
(shaders Slang de texgen) sont à toi.

## Dossiers et droits
| Dossier | Contenu | Toi |
|---|---|---|
| `config/` | réglages `.ron`, rechargés à chaud | lecture, écriture |
| `dev_res/` | sources des assets : shaders, générateurs, recettes, références | lecture, écriture |
| `res/` | ce que le moteur lit en jeu (textures, modèles, caches) ; produit par les outils | lecture seule |
| `work/` | sorties intermédiaires (exports PNG, textures générées…), régénérables | lecture seule |
| moteur : `…/Dromon-engine/dev_res/shaders/procgen/`, `…/common/noise.slang` | harnais et bibliothèque de texgen | lecture seule |

Tu écris les sources ; le moteur et les outils produisent `res/` et `work/`. Tout le reste
est refusé automatiquement. Pas de shell : tout passe par les outils `mcp__dromon__*`.
Tu peux lire les images (PNG) avec l'outil de lecture de fichiers.

Action refusée → ne cherche pas de contournement : dis à l'utilisateur ce qu'il faudrait.

## Outils
| Outil | Rôle |
|---|---|
| `ping` | le moteur répond-il ? |
| `camera_get` / `camera_teleport` | pose de la caméra / déplacement (bornée au monde) |
| `time_get` / `time_set` / `time_pause` / `time_resume` | heure du jeu, pause |
| `logs_tail` | derniers logs du moteur, filtrables par niveau |
| `screenshot` | capture de la fenêtre du jeu (image + chemin du PNG temporaire) |
| `texgen` | génère les textures d'une recette (cf. « Textures procédurales ») |
| `texgen_delete` | supprime définitivement un fichier de `dev_res/texgen/` (chemin relatif à ce dossier) |

« Moteur injoignable » → demande à l'utilisateur de lancer `./launch.py --ai_sandbox` (à la
racine du dépôt).

## Modifier une config
Le moteur surveille `config/` : à l'enregistrement d'un `.ron`, seul ce qui en dépend est
refait. Après chaque modification, vérifie avec `logs_tail` :
- `[INFO] <fichier> rechargé : …` → appliqué ;
- `[ERROR] … : ancienne config conservée` → fichier invalide (syntaxe, champ inconnu,
  valeur hors bornes) : corrige ;
- `[WARN] … ignoré, rechargement pas encore géré : …` → il faut redémarrer le moteur :
  préviens l'utilisateur ;
- rien → aucun changement effectif.

Format RON : structs entre parenthèses, `champ: valeur`, commentaires `//`. Un champ
absent prend sa valeur par défaut, un champ inconnu est une erreur. **Lis l'en-tête et
les commentaires d'un fichier avant de le modifier** (unités, bornes, pièges) et garde-les.

| Fichier | Contenu |
|---|---|
| `environment.ron` | ciel, soleil, écoulement du temps, eau |
| `render.ron` | rendu : vsync, ombres, LOD du terrain, tone mapping, caméra |
| `materials.ron` | matériaux du terrain : couleurs, textures |
| `material_rules.ron` | placement des matériaux (pente, côte, neige…) et teintes |
| `material_textures.ron` | rendu des textures du terrain |
| `world_macro.ron` | carte du monde : graine, taille, continents, climat (régénère tout le terrain) |
| `world_detail.ron` | relief de détail (régénère le terrain) |
| `debug.ron` | traces et aides visuelles de développement |

Textures du terrain (`color.png`, `height.png`, `normal.png`, `ao.png` par dossier),
référencées par `materials.ron` et rechargées à chaud : `Files("x")` →
`res/textures/terrain_textures/x/` ; `Generated("x")` → `work/texgen/terrain_textures/x/`
(produit par `texgen`), à défaut le dossier de même nom de `res/`. Introuvable → couleur
unie et `[WARN]` dans les logs.

## Cartes de la génération macro (`work/macro_maps/`)
Un PNG par champ de la carte du monde, **nord (+Y) en haut** : `continents`,
`coast_distance` (isolignes tous les 1 000 voxels), `mountains`, `relief`, `temperature`,
`wind`, `currents`, `sst` (température de surface de la mer), `sst_anomaly`,
`precipitation`. Pour juger une modification de `world_macro.ron` : attends dans
`logs_tail` la ligne `PNG de la carte macro écrits dans …` (quelques secondes, en
arrière-plan), puis lis les PNG concernés.

## Voir le rendu
`screenshot` montre ce qu'affiche la fenêtre du jeu. Pour juger un changement :
1. modifier → vérifier le rechargement dans `logs_tail` ;
2. cadrer avec `camera_teleport` (et `time_set` / `time_pause` pour un éclairage fixe,
   comparable d'une capture à l'autre) ;
3. `screenshot`, avec `wait_s` de quelques secondes après un déplacement ou une
   régénération du terrain (sinon terrain à moitié chargé).

Chaque capture coûte ~1 500–2 000 tokens : n'en prends que pour juger un changement
visible, pas pour vérifier qu'une modification a été appliquée (les logs suffisent).
Les captures sont temporaires (supprimées à l'arrêt du moteur).

## Textures procédurales (texgen)
Une **recette** `dev_res/texgen/<nom>.slang` (nom en `[a-z0-9_-]`) décrit une tuile de
texture ; le moteur l'exécute sur le GPU (compute shader) et en tire les 4 cartes.

Boucle de travail :
1. écrire ou modifier la recette ;
2. `texgen(recipe="<nom>")` → PNG dans `work/texgen/terrain_textures/<nom>/`. Erreur de
   compilation → message de `slangc` (fichier, ligne) : corrige ;
3. lire les PNG : formes, contraste, et surtout **raccords** (la tuile se répète : le bord
   droit doit continuer le bord gauche) ;
4. en jeu : `texture: Generated("<nom>")` sur le matériau dans `materials.ron` ; chaque
   `texgen` le recharge tout seul. Juger avec `screenshot` (sol plat et pente).

Contrat — lis d'abord la bibliothèque et le harnais (chemins ci-dessous) :
```slang
#include "procgen/texgen.slang"   // harnais ; inclut la bibliothèque
Surface surface(float2 p, uint seed) { … }   // p ∈ [0,1)² dans la tuile
```
- `height` : en **unités de tuile** (côté = 1) ; quelques millièmes pour un relief de sol.
  La normale et l'AO (cavités) sont calculées par le moteur depuis la hauteur.
- `albedo` : linéaire, [0, 1], **sans éclairage** (ni ombre, ni reflet, ni creux
  assombris : ce serait compté deux fois). La couleur d'ensemble vient de l'`albedo` de
  `materials.ron` : la texture n'apporte que les variations.
- Tout doit être **périodique** sur la tuile : bruits `*P` de la bibliothèque, à périodes
  entières. Une fonction non périodique donne une couture visible.
- Aucune boucle sans borne : un shader qui ne termine pas fait planter le moteur.
- Fonctions communes à plusieurs recettes (formes, palettes, sols…) : dans
  `dev_res/texgen/lib/*.slang`, avec `#pragma once` en tête, incluses par
  `#include "lib/<fichier>.slang"` (même chemin depuis une recette ou un autre fichier de
  `lib/`). Seuls les fichiers à la racine de `dev_res/texgen/` sont des recettes.
- `size` 2048 (défaut) : une autre taille que les textures voisines → couleur unie.

Fichiers du moteur (lecture seule), dans
`/home/drovulok/Documents/Dromon/Dromon-engine/dev_res/shaders/` :
- `procgen/lib/texgen.slang` : `Surface`, `gradientNoiseP`, `fbmP`, `worleyP` ;
- `procgen/texgen.slang` : le harnais (calcul de la normale et de l'AO) ;
- `common/noise.slang` : `hash33` (aléa à partir d'entiers).

Piège : `worleyP` ne renvoie que la cellule la plus proche. Une forme tracée depuis `f1`
qui déborde de sa cellule est **coupée net** le long de la frontière. Pour des formes qui
peuvent se chevaucher, parcours toi-même les 3×3 cellules voisines (comme dans
`worleyP`) et combine-les (`max`).

## Limites actuelles
- `logs_tail` ne contient que les logs reçus depuis le début de ta session.
- Fenêtre du jeu minimisée → pas de rendu, `screenshot` échoue (délai dépassé).
