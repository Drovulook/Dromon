// Atmosphère : brouillard et couleur de l'air. Partagé par terrain.slang (brouillard
// sur la géométrie) et sky.slang (fond) : les deux utilisent la MÊME couleur d'air,
// d'où un raccord sans couture à l'horizon.
#pragma once
#include "bindings.slang"

// Épaisseur optique entre la caméra et le fragment, atmosphère exponentielle.
float opticalDepth(float3 camPos, float3 worldPos, float sigma0, float H) {
  float3 v = worldPos - camPos;
  float d  = length(v);
  float dz = v.z;
  float eCam = exp(-camPos.z / H);
  // Rayon quasi horizontal : la formule dégénère en 0/0 → densité constante.
  if (abs(dz) < 1e-3) return sigma0 * d * eCam;
  return sigma0 * H * (d / dz) * (eCam - exp(-worldPos.z / H));
}

// le brouillard est teinté par le soleil, selon la direction; fonction Henyey-Greenstein
float3 fogSunTint(float3 V, float3 L, float3 sunCol, float g, float strength) {
  float cosT = dot(V, L);
  float denom = 1.0 + g*g - 2.0*g*cosT;
  // pic = (1+g)/(1-g)² : ≈ 18.9 pour g = 0.7, ≈ 190 pour g = 0.9
  float hg = (1.0 - g*g) / (denom * sqrt(denom));
  return sunCol * hg * strength;
}

// Couleur de l'air vu dans la direction V (normalisée) : ciel + halo solaire.
// C'est la couleur du ciel ET celle vers laquelle tend le brouillard.
float3 airColor(float3 V, float3 L, float3 sunCol) {
  return ubo.fog.rgb + fogSunTint(V, L, sunCol, ubo.fogParams.y, ubo.fogParams.z);
}

// Noie `color` (surface située en worldPos) dans le brouillard, selon la quantité
// d'air traversée. Le halo est dans `airColor`, donc pondéré par `fog` : sans air
// entre l'œil et le fragment, pas de diffusion, donc pas de halo.
float3 applyFog(float3 color, float3 worldPos, float3 L, float3 sunCol) {
  float3 camPos = ubo.cameraPosition.xyz;
  float tau = opticalDepth(camPos, worldPos, ubo.fog.w, ubo.fogParams.x);
  float fog = 1.0 - exp(-tau);
  float3 V = normalize(worldPos - camPos);
  return lerp(color, airColor(V, L, sunCol), fog);
}
