// Couture Transvoxel entre chunks de LOD différents : déformation appliquée au rendu.
// Le mesh d'un chunk ne dépend que de (coord, lod) ; ses voisins n'agissent que par
// `ChunkSeam`, poussé à chaque draw. Là où une dalle est dessinée, la dernière rangée de
// sommets MC est comprimée pour lui laisser la place.
// Méthode et atténuations : lod/transition_shrink.rs.
#pragma once

// ⚠ Synchronisé avec `CHUNK_SIZE` (terrain_generation/chunk/mod.rs).
static const float SEAM_CHUNK_SIZE = 64.0;

// Rôle d'un sommet. ⚠ Synchronisé avec `SEAM_VERTEX_*` (render_resources/terrain_mesh.rs).
static const uint SEAM_VERTEX_BODY = 0;      // surface MC
static const uint SEAM_VERTEX_FIXED = 1;     // dalle côté voisin, fond et parois
static const uint SEAM_VERTEX_SLAB_HIGH = 2; // dalle, côté chunk : +i pour la face i

// Faces indexées comme `Face::ALL` : 0 = NegX, 1 = PosX, 2 = NegY, 3 = PosY.
struct ChunkSeam {
  int2 origin;   // coin (x, y) du chunk, en voxels
  float lodStep; // pas du chunk (1 << lod)
  uint bits;     // bits 0-3 : faces à dalle ; bits 4-11 : atténuations (2 par face)
};

bool seamHasFace(ChunkSeam seam, uint face) {
  return ((seam.bits >> face) & 1u) != 0u;
}

// `end` : 0 = extrémité tangentielle basse, 1 = haute.
bool seamHasTaper(ChunkSeam seam, uint face, uint end) {
  return ((seam.bits >> (4u + 2u * face + end)) & 1u) != 0u;
}

// Coordonnée monde du plan frontière de `face`.
float seamPlane(ChunkSeam seam, uint face) {
  float low = float(face < 2u ? seam.origin.x : seam.origin.y);
  return (face & 1u) == 0u ? low : low + SEAM_CHUNK_SIZE;
}

// Retrait à la coordonnée tangente `t` : step/2, atténué sur `step` aux extrémités.
float seamAmount(ChunkSeam seam, uint face, float t) {
  float t0 = float(face < 2u ? seam.origin.y : seam.origin.x);
  float ramp = 1.0;
  if (seamHasTaper(seam, face, 0u)) {
    ramp = min(ramp, (t - t0) / seam.lodStep);
  }
  if (seamHasTaper(seam, face, 1u)) {
    ramp = min(ramp, (t0 + SEAM_CHUNK_SIZE - t) / seam.lodStep);
  }
  return 0.5 * seam.lodStep * saturate(ramp);
}

// Compression d'une coordonnée le long de son axe. `neg` : face négative de l'axe (0 pour
// X, 2 pour Y) ; `t` : coordonnée tangente d'origine. Les deux bandes ne se chevauchent pas.
float seamCompress(ChunkSeam seam, float c, uint neg, float t) {
  for (uint i = 0u; i < 2u; i++) {
    uint face = neg + i;
    if (!seamHasFace(seam, face)) {
      continue;
    }
    float side = i == 0u ? 1.0 : -1.0;
    float plane = seamPlane(seam, face);
    float d = (c - plane) * side;
    if (d < seam.lodStep) {
      float h = seamAmount(seam, face, t);
      return plane + side * (h + d * (seam.lodStep - h) / seam.lodStep);
    }
  }
  return c;
}

// Position déformée d'un sommet. Z n'est jamais touché : les chunks sont des colonnes.
float3 seamDisplace(ChunkSeam seam, float3 p, uint role) {
  if (role == SEAM_VERTEX_BODY) {
    if ((seam.bits & 0xFu) == 0u) {
      return p; // cas courant : aucune dalle dessinée
    }
    return float3(seamCompress(seam, p.x, 0u, p.y), seamCompress(seam, p.y, 2u, p.x), p.z);
  }
  if (role == SEAM_VERTEX_FIXED) {
    return p;
  }
  // Face haute réso : rentrée dans le chunk de l'épaisseur locale de la dalle.
  uint face = role - SEAM_VERTEX_SLAB_HIGH;
  float side = (face & 1u) == 0u ? 1.0 : -1.0;
  if (face < 2u) {
    p.x += side * seamAmount(seam, face, p.y);
  } else {
    p.y += side * seamAmount(seam, face, p.x);
  }
  return p;
}
