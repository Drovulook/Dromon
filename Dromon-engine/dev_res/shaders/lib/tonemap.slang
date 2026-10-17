// Tone mapping : ramène la luminance linéaire (sans borne) dans [0, 1] avant l'écriture
// dans la cible sRGB, qui couperait sinon chaque canal à 1 (teinte faussée, blanc).
// Appelé en dernière étape par tous les shaders qui écrivent la couleur.
#pragma once

#include "bindings.slang"

// Khronos PBR Neutral (2024) : identité sous ~0,76, puis compression du canal le plus
// fort appliquée aux trois (teinte préservée), désaturation des très hautes lumières.
// Réf. : github.com/KhronosGroup/ToneMapping/tree/main/PBR_Neutral
// `toe` : applique le pied (cf. `tonemapAdditive` pour s'en passer).
float3 pbrNeutral(float3 color, bool toe) {
  const float startCompression = 0.8 - 0.04;
  const float desaturation = 0.15;

  // Pied : retire le ~4 % de reflet spéculaire du PBR (F₀ = 0,04) ; parabole sous 0,08
  // pour rester ≥ 0, raccordée sans cassure (valeur 0,04, pente nulle en 0,08).
  if (toe) {
    float x = min(color.r, min(color.g, color.b));
    float offset = x < 0.08 ? x - 6.25 * x * x : 0.04;
    color -= offset;
  }

  float peak = max(color.r, max(color.g, color.b));
  if (peak < startCompression) return color;

  // Épaule : hyperbole de valeur et pente 1 au départ, → 1 à l'infini.
  const float d = 1.0 - startCompression;
  float newPeak = 1.0 - d * d / (peak + d - startCompression);
  color *= newPeak / peak;

  // Plus la compression est forte, plus on tire vers le blanc (éblouissement).
  float g = 1.0 - 1.0 / (desaturation * (peak - newPeak) + 1.0);
  return lerp(color, float3(newPeak), g);
}

// Exposition puis courbe ; désactivé (`render.ron`) : couleur inchangée, coupée à 1
// par la cible comme avant.
float3 tonemap(float3 color) {
  if (ubo.tonemapParams.x == 0.0) return color;
  return pbrNeutral(color * ubo.tonemapParams.y, true);
}

// Pour une couleur **ajoutée** à un fond par mélange (eau, alpha prémultiplié) : à
// appliquer à la contribution déjà pondérée (`F · reflet`), et sans le pied, qui
// retirerait 0,04 à un terme faible (reflet du ciel ≈ 0,03 → effacé). Seule l'épaule
// agit : les forts reflets (soleil) sont compressés vers le blanc.
float3 tonemapAdditive(float3 color) {
  if (ubo.tonemapParams.x == 0.0) return color;
  return pbrNeutral(color * ubo.tonemapParams.y, false);
}
