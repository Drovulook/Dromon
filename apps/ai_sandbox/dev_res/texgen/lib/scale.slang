// Échelle commune des textures de terrain : mêmes unités pour toutes les recettes, pour
// que brins, cailloux et fissures aient des tailles cohérentes à l'interface entre
// matériaux. ⚠ Suppose `texture_size: 10.0` dans materials.ron et 1 voxel ≈ 1 m.
// À 2048 px : 1 texel ≈ 0,5 cm, rien de net sous ~1 cm.
#pragma once

static const float TILE_CM = 1000.0; // côté de la tuile : 10 voxels

// Longueur en cm → unités de tuile (hauteurs, largeurs, amplitudes de warp).
float cm(float centimeters) { return centimeters / TILE_CM; }

// Période de bruit pour des motifs de `centimeters` de large.
int periodFor(float centimeters) { return max(1, int(round(TILE_CM / centimeters))); }
