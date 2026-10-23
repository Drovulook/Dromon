// Tone mapping : ramène la luminance linéaire (sans borne) dans [0, 1] avant l'écriture
// dans la cible sRGB, qui couperait sinon chaque canal à 1 (teinte faussée, blanc).
// Appelé en dernière étape par tous les shaders qui écrivent la couleur.
#pragma once

#include "bindings.slang"

// Pied de PBR Neutral : retire le ~4 % de reflet spéculaire du PBR (F₀ = 0,04) ;
// parabole sous 0,08 pour rester ≥ 0, raccordée sans cassure (valeur 0,04, pente nulle
// en 0,08). Toujours ≤ au plus faible canal. Écrase les tons très sombres (nuit) :
// 0,02 → 0,0025.
float toeOffset(float3 color) {
  float x = min(color.r, min(color.g, color.b));
  return x < 0.08 ? x - 6.25 * x * x : 0.04;
}

// Khronos PBR Neutral (2024) : identité sous ~0,76, puis compression du canal le plus
// fort appliquée aux trois (teinte préservée), désaturation des très hautes lumières.
// Réf. : github.com/KhronosGroup/ToneMapping/tree/main/PBR_Neutral
// `toe` : applique le pied (cf. `tonemapBlended` pour l'appliquer à part).
float3 pbrNeutral(float3 color, bool toe) {
  const float startCompression = 0.8 - 0.04;
  const float desaturation = 0.15;

  if (toe)
    color -= toeOffset(color);

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

// Contribution `weight · color` **ajoutée** à un fond par mélange (eau, alpha
// prémultiplié, weight = F). Pied appliqué à `color` avant pondération, comme sur le
// ciel et le terrain : sur `F · reflet`, il effacerait un reflet faible ; sans pied, le
// reflet du ciel nocturne ressortirait plus clair que le ciel lui-même. Épaule sur la
// contribution pondérée : les forts reflets (soleil) ne sont pas plafonnés à F.
float3 tonemapBlended(float3 color, float weight) {
  if (ubo.tonemapParams.x == 0.0) return weight * color;
  color *= ubo.tonemapParams.y;
  return pbrNeutral(weight * (color - toeOffset(color)), false);
}
