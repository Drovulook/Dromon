// Ombres portées : shadow map (set 2) et son échantillonnage filtré.
#pragma once
#include "bindings.slang"

// set 2, binding 0 : la shadow map. Sampler2DShadow = sampler de comparaison :
// SampleCmp(uv, ref) renvoie ~1.0 si ref <= profondeur stockée (fragment éclairé),
// ~0.0 sinon (fragment dans l'ombre), avec PCF 2×2 si filtrage linéaire.
[[vk::binding(0, 2)]]
Sampler2DShadow shadowMap;

// Taille d'un texel de la shadow map en coordonnées uv (= 1 / résolution).
// ⚠ doit rester synchronisé avec SHADOW_MAP_RESOLUTION (shadow_map.rs).
static const float SHADOW_MAP_TEXEL_SIZE = 1.0 / 2048.0;

// Interrupteur de debug : `false` = tout est éclairé (la passe d'ombre tourne quand
// même, seul son résultat est ignoré).
static const bool SHADOWS_ENABLED = true;

// Anti-acné, en texels de shadow map (donc valable quelle que soit la taille de
// la boîte d'ombre). Le normal offset doit couvrir l'étalement du PCF (±2 texels).
static const float NORMAL_OFFSET_TEXELS = 2.5;
static const float DEPTH_BIAS_TEXELS = 1.0;

// Renvoie un facteur d'éclairement ∈ [0,1] : 1 = pleinement éclairé, 0 = ombré.
// `N` normalisée, `NdotL` = max(dot(N, L), 0).
float computeShadow(float3 worldPos, float3 N, float NdotL) {
  if (!SHADOWS_ENABLED) {
    return 1.0;
  }

  // Normal offset : on échantillonne depuis un point décollé de la surface le long
  // de N, pour qu'elle ne se compare plus à sa propre profondeur discrétisée.
  // Décalage ∝ sin(angle N/L) : nul face à la lumière, maximal en rasant.
  float texel = ubo.shadowParams.y;
  float sinNL = sqrt(1.0 - NdotL * NdotL);
  float3 samplePos = worldPos + N * (texel * NORMAL_OFFSET_TEXELS * sinNL);
  float4 shadowCoord = mul(ubo.lightViewProj, float4(samplePos, 1.0));

  // Division perspective (triviale en orthographique, où w = 1).
  float3 proj = shadowCoord.xyz / shadowCoord.w;

  // En dehors du frustum de la lumière (au-delà du far) → considéré éclairé.
  if (proj.z > 1.0) {
    return 1.0;
  }

  // NDC xy ∈ [-1,1] → coordonnées de texture uv ∈ [0,1].
  float2 uv = proj.xy * 0.5 + 0.5;

  // Petit biais de profondeur constant (la pente est gérée par le normal offset),
  // unités monde → profondeur normalisée via l'épaisseur de la boîte.
  float bias = texel * DEPTH_BIAS_TEXELS / ubo.shadowParams.x;

  // PCF 5×5 : on moyenne 25 texels voisins. Chaque tap est déjà filtré 2×2 par le
  // hardware (sampler LINEAR) → bord lissé sans escalier.
  float sum = 0.0;
  for (int x = -2; x <= 2; x++) {
    for (int y = -2; y <= 2; y++) {
      float2 offset = float2(x, y) * SHADOW_MAP_TEXEL_SIZE;
      sum += shadowMap.SampleCmp(uv + offset, proj.z - bias);
    }
  }
  return sum / 25; // moyenne des 25 échantillons
}
