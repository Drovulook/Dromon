// Bibliothèque des recettes de textures (texgen) : contrat d'une recette et bruits
// **périodiques** sur la tuile [0,1)² — la tuile se répète sans couture.
// Harnais qui appelle la recette : procgen/texgen.slang.
#pragma once
#include "common/noise.slang"

// Contrat d'une recette : inclure le harnais (`#include "procgen/texgen.slang"`, qui
// inclut cette bibliothèque), puis définir, pour chaque point p ∈ [0,1)² de la tuile :
//   Surface surface(float2 p, uint seed)
struct Surface {
  float height;  // en unités de tuile (côté = 1) : 0.01 = 1 % du côté. Pas de borne.
  float3 albedo; // linéaire, dans [0, 1]
};

// Périodicité : le réseau d'un bruit de période P (P cellules par côté de tuile) est lu
// modulo P → la cellule P est la cellule 0, le bord droit continue le bord gauche.
uint2 wrapCell(int2 cell, int period) {
  return uint2(((cell % period) + period) % period);
}

// Gradient unitaire aléatoire attaché au nœud `cell` du réseau.
float2 latticeGradient(int2 cell, int period, uint seed) {
  float a = hash33(uint3(wrapCell(cell, period), seed)).x * 6.2831853;
  return float2(cos(a), sin(a));
}

// Bruit de gradient (Perlin) périodique, ≈ [−1, 1], moyenne 0. `period` cellules par
// tuile : la taille des motifs est ~1/period. Plus isotrope que le bruit de valeur (pas
// de blocs alignés sur les axes) ; fondu quintique (C²) → normales sans arêtes.
float gradientNoiseP(float2 p, int period, uint seed) {
  float2 x = p * float(period);
  int2 i = int2(floor(x));
  float2 f = x - floor(x);
  float2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
  float a = dot(latticeGradient(i, period, seed), f);
  float b = dot(latticeGradient(i + int2(1, 0), period, seed), f - float2(1.0, 0.0));
  float c = dot(latticeGradient(i + int2(0, 1), period, seed), f - float2(0.0, 1.0));
  float d = dot(latticeGradient(i + int2(1, 1), period, seed), f - float2(1.0, 1.0));
  // Gradients unitaires : |bruit| ≤ √2/2 → ×√2 pour couvrir ≈ [−1, 1].
  return 1.4142136 * lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

// fBm : `octaves` couches de bruit, fréquence ×2 et amplitude ÷2 à chaque couche (la
// période double aussi → reste périodique). Normalisé : ≈ [−1, 1], en pratique ±0,5.
float fbmP(float2 p, int period, int octaves, uint seed) {
  float sum = 0.0, amplitude = 0.5, norm = 0.0;
  for (int k = 0; k < octaves; k++) {
    sum += amplitude * gradientNoiseP(p, period << k, seed + uint(k));
    norm += amplitude;
    amplitude *= 0.5;
  }
  return sum / norm;
}

// Résultat de `worleyP`. Distances en unités de cellule.
struct Cell {
  float f1;    // distance au point le plus proche
  float f2;    // au 2ᵉ plus proche (f2 − f1 ≈ 0 sur les frontières entre cellules)
  float3 id;   // 3 aléas dans [0, 1] propres à la cellule du point le plus proche
};

// Bruit cellulaire (Worley) périodique : un point par cellule, décalé au hasard de
// ±jitter/2 autour du centre (0 = grille régulière, 1 = maximum sans quitter la cellule).
// Base des cailloux, écailles, craquelures : une forme par cellule, couleur par `id`.
Cell worleyP(float2 p, int period, float jitter, uint seed) {
  float2 x = p * float(period);
  int2 i = int2(floor(x));
  float2 f = x - floor(x);
  Cell r = { 8.0, 8.0, float3(0.0) };
  for (int y = -1; y <= 1; y++) {
    for (int xo = -1; xo <= 1; xo++) {
      int2 o = int2(xo, y);
      float3 h = hash33(uint3(wrapCell(i + o, period), seed));
      float2 point = float2(o) + 0.5 + (h.xy - 0.5) * jitter;
      float d = length(point - f);
      if (d < r.f1) {
        r.f2 = r.f1;
        r.f1 = d;
        // Aléas de la cellule, indépendants de la position du point (hash décalé).
        r.id = hash33(uint3(wrapCell(i + o, period), seed + 0x9E37u));
      } else if (d < r.f2) {
        r.f2 = d;
      }
    }
  }
  return r;
}
