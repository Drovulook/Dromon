// Étoiles procédurales (pilotées par stars.rs). Le ciel est découpé en cases sur les
// 6 faces d'un cube (taille ~uniforme, contrairement à une grille lat/long qui se
// pince aux pôles) ; un hash par case décide s'il y a une étoile, où, de quel éclat
// et de quelle teinte.
#pragma once
#include "bindings.slang"

static const float STAR_CELLS = 256.0;   // cases par côté de face
static const float STAR_RADIUS_PX = 1.2; // rayon d'une étoile moyenne, en pixels

// Hash entier pcg3d (Jarzynski & Olano, 2020) : 3 sorties décorrélées.
uint3 pcg3d(uint3 v) {
  v = v * 1664525u + 1013904223u;
  v.x += v.y * v.z; v.y += v.z * v.x; v.z += v.x * v.y;
  v ^= v >> 16u;
  v.x += v.y * v.z; v.y += v.z * v.x; v.z += v.x * v.y;
  return v;
}

float3 hash33(uint3 p) {
  return float3(pcg3d(p)) * (1.0 / 4294967295.0);
}

// Lumière des étoiles dans la direction V (monde, normalisée).
float3 stars(float3 V) {
  // Direction dans le repère du ciel fixe : les étoiles tournent avec l'heure.
  float3 dir = mul((float3x3)ubo.skyRotation, V);

  // Taille d'un pixel, en radians. Calculée AVANT tout branchement (fwidth exige un
  // flot de contrôle uniforme) et sur `dir`, continu — `grid` saute d'une face à l'autre.
  float pixelAngle = length(fwidth(dir));

  // Projection sur le cube : on divise par la plus grande composante.
  float3 a = abs(dir);
  float amax = max(a.x, max(a.y, a.z));
  float2 uv;
  uint face;
  if (a.x >= a.y && a.x >= a.z) { uv = dir.yz / a.x; face = dir.x > 0.0 ? 0u : 1u; }
  else if (a.y >= a.z)          { uv = dir.xz / a.y; face = dir.y > 0.0 ? 2u : 3u; }
  else                          { uv = dir.xy / a.z; face = dir.z > 0.0 ? 4u : 5u; }

  float2 grid = (uv * 0.5 + 0.5) * STAR_CELLS;
  float2 cell = floor(grid);
  uint3 key = uint3(uint2(cell), face);

  float3 h = hash33(key);
  float density = ubo.stars.z;
  if (h.x >= density) return float3(0.0);
  float3 h2 = hash33(key + uint3(0u, 0u, 6u)); // 2e tirage : teinte, scintillation

  // h.x / density est uniforme dans [0, 1[ : la puissance donne beaucoup d'étoiles
  // faibles et quelques brillantes, comme le vrai ciel.
  float brightness = 0.05 + 0.95 * pow(h.x / density, 4.0);

  // Rayon ≥ ~1 pixel quelle que soit la résolution : plus petit, l'étoile clignote
  // quand la caméra tourne (elle tombe ou non sur le centre d'un pixel).
  // `STAR_CELLS / 2 / amax` convertit des radians en unités de grille (1er ordre).
  // Plafonné à 0.25 case : l'étoile ne déborde jamais de sa case.
  float radius = STAR_RADIUS_PX * (0.8 + 0.6 * brightness) * pixelAngle * STAR_CELLS * 0.5 / amax;
  radius = min(radius, 0.25);
  float2 center = cell + 0.25 + 0.5 * h.yz;
  float shape = smoothstep(radius, 0.0, length(grid - center));

  // Scintillation, doublée à l'horizon (plus d'air turbulent traversé).
  float horizon = 1.0 - saturate(V.z);
  float amp = ubo.stars.w * (1.0 + horizon);
  float twinkle = 1.0 + amp * sin(ubo.stars.y * (4.0 + 8.0 * h2.y) + h2.z * 6.2832);

  // Teinte : du bleu-blanc (étoiles chaudes) à l'orangé pâle (étoiles froides).
  float3 tint = lerp(float3(0.75, 0.85, 1.0), float3(1.0, 0.85, 0.70), h2.x);

  // Extinction atmosphérique : effacées dans les ~12 derniers degrés avant l'horizon.
  float fade = saturate(V.z * 5.0);

  return tint * (brightness * shape * twinkle * fade * ubo.stars.x);
}
